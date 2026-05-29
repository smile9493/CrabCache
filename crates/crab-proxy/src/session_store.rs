//! MiMo transparent session store: Redis-backed canonical `messages[]` per stable session id.
//!
//! Shrinks upstream payload on append-only turns; does **not** change exact cache keys
//! (`original_request_body` remains the client JSON).

use bb8_redis::RedisConnectionManager;
use crab_metrics::global_metrics;
use crab_reasoning::message_signature;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

const KEY_PREFIX: &str = "crab:session:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMergeOutcome {
    Miss,
    Hit,
    PrefixBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMergeError {
    PrefixBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub messages: Vec<Value>,
    pub turn_seq: u64,
    pub prefix_sig: String,
    pub updated_at: u64,
}

pub struct SessionStore {
    pool: bb8::Pool<RedisConnectionManager>,
}

impl SessionStore {
    pub async fn new(redis_url: &str) -> anyhow::Result<Self> {
        let manager = RedisConnectionManager::new(redis_url)?;
        let pool = bb8::Pool::builder()
            .max_size(8)
            .connection_timeout(std::time::Duration::from_secs(2))
            .build(manager)
            .await?;
        Ok(Self { pool })
    }

    pub fn redis_key(namespace: Option<&str>, stable_session_id: &str) -> String {
        let ns = namespace.filter(|s| !s.is_empty()).unwrap_or("default");
        format!("{KEY_PREFIX}{ns}:{stable_session_id}")
    }

    pub async fn get(&self, key: &str) -> Option<SessionEntry> {
        let mut conn = self.pool.get().await.ok()?;
        let raw: Option<String> = conn.get(key).await.ok()?;
        let raw = raw?;
        serde_json::from_str(&raw).ok()
    }

    pub async fn set(&self, key: &str, entry: &SessionEntry, ttl_secs: u64) {
        let Ok(payload) = serde_json::to_string(entry) else {
            return;
        };
        let mut conn = match self.pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = %key, "session store SET: pool get failed");
                return;
            }
        };
        if let Err(e) = conn.set_ex::<_, _, ()>(key, payload, ttl_secs).await {
            warn!(error = %e, key = %key, "session store SET failed");
        }
    }

    /// Merge stored canonical messages with client messages (append-only by message signature).
    pub fn merge_messages(
        stored: &[Value],
        client: &[Value],
    ) -> Result<Vec<Value>, SessionMergeError> {
        if stored.is_empty() {
            return Ok(client.to_vec());
        }
        if client.len() < stored.len() {
            return Err(SessionMergeError::PrefixBreak);
        }
        for (stored_msg, client_msg) in stored.iter().zip(client.iter()) {
            if message_signature(stored_msg) != message_signature(client_msg) {
                return Err(SessionMergeError::PrefixBreak);
            }
        }
        let mut merged = stored.to_vec();
        merged.extend_from_slice(&client[stored.len()..]);
        Ok(merged)
    }

    pub fn prefix_sig(messages: &[Value]) -> String {
        messages.last().map(message_signature).unwrap_or_default()
    }

    pub fn cap_messages(messages: Vec<Value>, max_messages: usize) -> Vec<Value> {
        if max_messages == 0 || messages.len() <= max_messages {
            return messages;
        }
        let system: Vec<Value> = messages
            .iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
            .cloned()
            .collect();
        let non_system: Vec<Value> = messages
            .iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
            .cloned()
            .collect();
        let keep = max_messages.saturating_sub(system.len());
        let start = non_system.len().saturating_sub(keep);
        let mut out = system;
        out.extend(non_system.into_iter().skip(start));
        out
    }

    pub fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Extract assistant text from non-stream JSON or client-shaped SSE bytes.
pub fn extract_assistant_content(
    is_streaming: bool,
    accumulated_body: &[u8],
    client_sse_body: &[u8],
) -> Option<String> {
    if !is_streaming {
        let value: Value = serde_json::from_slice(accumulated_body).ok()?;
        return value
            .get("choices")?
            .get(0)?
            .get("message")?
            .get("content")?
            .as_str()
            .map(str::to_string);
    }
    let mut content = String::new();
    for line in client_sse_body.split(|b| *b == b'\n') {
        let stripped = line.trim_ascii();
        if !stripped.starts_with(b"data:") {
            continue;
        }
        let data = stripped[b"data:".len()..].trim_ascii();
        if data == b"[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(data) else {
            continue;
        };
        if let Some(delta) = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            content.push_str(delta);
        }
    }
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

pub async fn apply_mimo_session_store(
    store: &SessionStore,
    features: &crate::context::FeaturesConfig,
    ctx: &mut crate::context::GatewayContext,
    payload: &mut Arc<Value>,
    stable_session: Option<&str>,
    namespace: Option<&str>,
) {
    if !features.mimo_session_store {
        return;
    }
    let Some(session_id) = stable_session.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(client_messages) = payload.get("messages").and_then(|m| m.as_array()).cloned() else {
        return;
    };
    let redis_key = SessionStore::redis_key(namespace, session_id);
    let client_bytes = serde_json::to_vec(&client_messages).unwrap_or_default();

    match store.get(&redis_key).await {
        None => {
            global_metrics().record_session_store_miss();
            ctx.session_store_outcome = Some("miss".into());
            let n = client_messages.len();
            ctx.session_persist_base = Some(client_messages);
            ctx.session_store_redis_key = Some(redis_key);
            debug!(
                request_id = %ctx.request_id,
                session_id = %session_id,
                messages = n,
                "session store miss"
            );
        }
        Some(entry) => {
            let stored_len = entry.messages.len();
            let client_len = client_messages.len();
            match SessionStore::merge_messages(&entry.messages, &client_messages) {
                Ok(merged) => {
                    let merged_len = merged.len();
                    let merged_bytes = serde_json::to_vec(&merged).unwrap_or_default();
                    if merged_bytes.len() < client_bytes.len() {
                        global_metrics().record_session_store_upstream_bytes_saved(
                            (client_bytes.len() - merged_bytes.len()) as u64,
                        );
                    }
                    global_metrics().record_session_store_hit();
                    ctx.session_store_outcome = Some("hit".into());
                    let mut new_payload = (**payload).clone();
                    if let Some(obj) = new_payload.as_object_mut() {
                        obj.insert("messages".into(), Value::Array(merged.clone()));
                    }
                    *payload = Arc::new(new_payload);
                    ctx.parsed_request_payload = Some(payload.clone());
                    ctx.session_persist_base = Some(merged);
                    ctx.session_store_redis_key = Some(redis_key);
                    debug!(
                        request_id = %ctx.request_id,
                        session_id = %session_id,
                        stored = stored_len,
                        client = client_len,
                        merged = merged_len,
                        "session store hit"
                    );
                }
                Err(SessionMergeError::PrefixBreak) => {
                    global_metrics().record_session_store_prefix_break();
                    ctx.session_store_outcome = Some("break".into());
                    ctx.session_persist_base = Some(client_messages);
                    ctx.session_store_redis_key = Some(redis_key);
                    warn!(
                        request_id = %ctx.request_id,
                        session_id = %session_id,
                        stored = stored_len,
                        client = client_len,
                        "session store prefix break; using client messages"
                    );
                }
            }
        }
    }
    ctx.session_upstream_messages_len = ctx.session_persist_base.as_ref().map(|m| m.len());
}

pub fn spawn_session_persist(
    store: Arc<SessionStore>,
    redis_key: String,
    mut base_messages: Vec<Value>,
    assistant_content: Option<String>,
    ttl_secs: u64,
    max_messages: usize,
) {
    if let Some(content) = assistant_content.filter(|c| !c.is_empty()) {
        base_messages.push(serde_json::json!({
            "role": "assistant",
            "content": content,
        }));
    }
    base_messages = SessionStore::cap_messages(base_messages, max_messages);
    let entry = SessionEntry {
        prefix_sig: SessionStore::prefix_sig(&base_messages),
        turn_seq: base_messages.len() as u64,
        updated_at: SessionStore::now_unix(),
        messages: base_messages,
    };
    tokio::spawn(async move {
        store.set(&redis_key, &entry, ttl_secs).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Value {
        serde_json::json!({"role": role, "content": content})
    }

    #[test]
    fn merge_append_only_suffix() {
        let stored = vec![msg("user", "a"), msg("assistant", "b")];
        let client = vec![msg("user", "a"), msg("assistant", "b"), msg("user", "c")];
        let merged = SessionStore::merge_messages(&stored, &client).expect("merge");
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[2].get("content").and_then(|c| c.as_str()), Some("c"));
    }

    #[test]
    fn merge_prefix_break_on_edit() {
        let stored = vec![msg("user", "a")];
        let client = vec![msg("user", "b")];
        assert_eq!(
            SessionStore::merge_messages(&stored, &client),
            Err(SessionMergeError::PrefixBreak)
        );
    }

    #[test]
    fn cap_messages_preserves_system() {
        let messages: Vec<Value> = (0..10)
            .map(|i| {
                msg(
                    if i % 2 == 0 { "user" } else { "assistant" },
                    &i.to_string(),
                )
            })
            .collect();
        let mut with_system = vec![msg("system", "sys")];
        with_system.extend(messages);
        let capped = SessionStore::cap_messages(with_system, 5);
        assert!(
            capped
                .iter()
                .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        );
        assert!(capped.len() <= 5);
    }

    #[test]
    fn extract_assistant_from_json() {
        let body = br#"{"choices":[{"message":{"role":"assistant","content":"hi"}}]}"#;
        assert_eq!(
            extract_assistant_content(false, body, &[]).as_deref(),
            Some("hi")
        );
    }
}
