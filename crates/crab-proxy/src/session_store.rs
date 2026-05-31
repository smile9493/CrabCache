//! MiMo transparent session store: Redis-backed canonical `messages[]` per stable session id.
//!
//! **Chat Completions MiMo clients only.** Codex / `POST /v1/responses` clients must use
//! [`ResponsesChainStore`] (`previous_response_id`); merging here on converted `messages[]`
//! misaligns with that chain.

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

const PERSIST_TAIL_CAP: usize = 48;
const TAIL_ANCHOR_LEN: usize = 4;
const LONG_SESSION_SKIP_MERGE: usize = 96;

impl SessionStore {
    pub async fn new(redis_url: &str) -> anyhow::Result<Self> {
        let manager = RedisConnectionManager::new(redis_url)?;
        let pool = bb8::Pool::builder()
            .max_size(12)
            .connection_timeout(std::time::Duration::from_secs(5))
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

    fn is_ephemeral_session_system(message: &Value) -> bool {
        message.get("role").and_then(|r| r.as_str()) == Some("system")
            && message
                .get("content")
                .and_then(|c| c.as_str())
                .is_some_and(|s| s.contains("[crabcache]"))
    }

    fn strip_ephemeral_for_merge(messages: &[Value]) -> Vec<Value> {
        messages
            .iter()
            .filter(|m| !Self::is_ephemeral_session_system(m))
            .cloned()
            .collect()
    }

    fn messages_match(stored: &[Value], client_slice: &[Value]) -> bool {
        let stored = Self::strip_ephemeral_for_merge(stored);
        let client_slice = Self::strip_ephemeral_for_merge(client_slice);
        stored.len() == client_slice.len()
            && stored
                .iter()
                .zip(client_slice.iter())
                .all(|(a, b)| message_signature(a) == message_signature(b))
    }

    fn try_tail_anchor_merge(stored: &[Value], client: &[Value]) -> Option<Vec<Value>> {
        let n = stored.len().min(client.len()).min(TAIL_ANCHOR_LEN).max(1);
        if stored.len() < n || client.len() < n {
            return None;
        }
        if Self::messages_match(&stored[stored.len() - n..], &client[client.len() - n..]) {
            return Some(client.to_vec());
        }
        None
    }

    /// Merge stored canonical messages with client messages (append-only by message signature).
    ///
    /// Redis stores a capped **suffix** window; Codex sends full history from the start — try
    /// suffix alignment when prefix merge fails.
    pub fn merge_messages(
        stored: &[Value],
        client: &[Value],
    ) -> Result<Vec<Value>, SessionMergeError> {
        if stored.is_empty() {
            return Ok(client.to_vec());
        }
        if client.len() >= stored.len() && Self::messages_match(stored, &client[..stored.len()]) {
            let mut merged = stored.to_vec();
            merged.extend_from_slice(&client[stored.len()..]);
            return Ok(merged);
        }
        if client.len() >= stored.len() {
            let max_offset = client.len() - stored.len();
            for offset in (0..=max_offset).rev() {
                if Self::messages_match(stored, &client[offset..offset + stored.len()]) {
                    return Ok(client.to_vec());
                }
            }
        }
        if let Some(merged) = Self::try_tail_anchor_merge(stored, client) {
            return Ok(merged);
        }
        Err(SessionMergeError::PrefixBreak)
    }

    pub fn prefix_sig(messages: &[Value]) -> String {
        messages.last().map(message_signature).unwrap_or_default()
    }

    /// Shrink message history for upstream when session merge fails or payload is oversized.
    pub fn shrink_messages_for_upstream(
        messages: Vec<Value>,
        retire_prefix: bool,
        keep_recent_turns: usize,
        max_messages: usize,
    ) -> Vec<Value> {
        let mut msgs = messages;
        if retire_prefix && keep_recent_turns > 0 {
            let (trimmed, _, _) =
                crab_reasoning::retire_prefix_messages_by_turns(&msgs, keep_recent_turns);
            msgs = trimmed;
        }
        Self::cap_messages(msgs, max_messages)
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

    /// MiMo has ~1M context: shrink by **turn boundaries**, not byte/message caps that break tool chains.
    pub fn prepare_mimo_upstream_messages(
        messages: Vec<Value>,
        keep_recent_turns: usize,
    ) -> Vec<Value> {
        let (trimmed, _, _) =
            crab_reasoning::retire_prefix_messages_by_turns(&messages, keep_recent_turns);
        let mut sanitized = crate::responses_wire::sanitize_tool_message_chain(trimmed);
        Self::truncate_oversized_tool_content(&mut sanitized, 32_768);
        sanitized
    }

    fn truncate_oversized_tool_content(messages: &mut [Value], max_chars: usize) {
        if max_chars == 0 {
            return;
        }
        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|r| r.as_str()) != Some("tool") {
                continue;
            }
            let Some(content) = msg.get_mut("content") else {
                continue;
            };
            let Some(text) = content.as_str() else {
                continue;
            };
            if text.len() <= max_chars {
                continue;
            }
            let truncated = format!(
                "{}... [crabcache: {} chars truncated for upstream]",
                &text[..max_chars],
                text.len().saturating_sub(max_chars)
            );
            *content = Value::String(truncated);
        }
    }

    fn set_upstream_messages(
        ctx: &mut crate::context::GatewayContext,
        payload: &mut Arc<Value>,
        upstream: Vec<Value>,
    ) -> usize {
        let upstream_bytes = serde_json::to_vec(&upstream).unwrap_or_default().len();
        ctx.session_upstream_messages_len = Some(upstream.len());
        let mut new_payload = (**payload).clone();
        if let Some(obj) = new_payload.as_object_mut() {
            obj.insert("messages".into(), Value::Array(upstream));
        }
        *payload = Arc::new(new_payload);
        ctx.parsed_request_payload = Some(payload.clone());
        upstream_bytes
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
    store: Arc<SessionStore>,
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
    let _client_bytes = serde_json::to_vec(&client_messages).unwrap_or_default();

    // Long Codex sessions rewrite history every turn — skip merge; shrink by turn boundary only.
    if client_messages.len() > LONG_SESSION_SKIP_MERGE {
        let upstream = SessionStore::prepare_mimo_upstream_messages(
            client_messages.clone(),
            features.mimo_keep_recent_turns,
        );
        let upstream_bytes = SessionStore::set_upstream_messages(ctx, payload, upstream.clone());
        let persist_tail = SessionStore::cap_messages(client_messages.clone(), PERSIST_TAIL_CAP);
        ctx.session_store_outcome = Some("long".into());
        ctx.session_persist_base = Some(persist_tail.clone());
        ctx.session_store_redis_key = Some(redis_key.clone());
        spawn_session_persist(
            store,
            redis_key,
            persist_tail,
            None::<String>,
            features.mimo_session_store_ttl_secs,
            features.mimo_session_store_max_messages,
        );
        warn!(
            request_id = %ctx.request_id,
            session_id = %session_id,
            client = client_messages.len(),
            upstream_messages = upstream.len(),
            upstream_bytes,
            "session store long-session turn-based upstream"
        );
        return;
    }

    match store.get(&redis_key).await {
        None => {
            global_metrics().record_session_store_miss();
            ctx.session_store_outcome = Some("miss".into());
            let n = client_messages.len();
            ctx.session_persist_base = Some(client_messages.clone());
            ctx.session_store_redis_key = Some(redis_key);
            if client_messages.len() > features.mimo_keep_recent_turns.saturating_mul(8) {
                let upstream = SessionStore::prepare_mimo_upstream_messages(
                    client_messages.clone(),
                    features.mimo_keep_recent_turns,
                );
                let _upstream_bytes =
                    SessionStore::set_upstream_messages(ctx, payload, upstream.clone());
            }
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
                    let upstream = if merged_len > LONG_SESSION_SKIP_MERGE {
                        SessionStore::prepare_mimo_upstream_messages(
                            merged.clone(),
                            features.mimo_keep_recent_turns,
                        )
                    } else {
                        crate::responses_wire::sanitize_tool_message_chain(merged.clone())
                    };
                    let upstream_bytes =
                        SessionStore::set_upstream_messages(ctx, payload, upstream.clone());
                    if merged_bytes.len() > upstream_bytes {
                        global_metrics().record_session_store_upstream_bytes_saved(
                            (merged_bytes.len() - upstream_bytes) as u64,
                        );
                    }
                    global_metrics().record_session_store_hit();
                    ctx.session_store_outcome = Some("hit".into());
                    ctx.session_persist_base = Some(merged);
                    ctx.session_store_redis_key = Some(redis_key);
                    debug!(
                        request_id = %ctx.request_id,
                        session_id = %session_id,
                        stored = stored_len,
                        client = client_len,
                        merged = merged_len,
                        upstream_messages = upstream.len(),
                        upstream_bytes,
                        "session store hit"
                    );
                }
                Err(SessionMergeError::PrefixBreak) => {
                    global_metrics().record_session_store_prefix_break();
                    ctx.session_store_outcome = Some("break".into());
                    let upstream = SessionStore::prepare_mimo_upstream_messages(
                        client_messages.clone(),
                        features.mimo_keep_recent_turns,
                    );
                    let persist_tail =
                        SessionStore::cap_messages(client_messages.clone(), PERSIST_TAIL_CAP);
                    let upstream_bytes =
                        SessionStore::set_upstream_messages(ctx, payload, upstream.clone());
                    ctx.session_persist_base = Some(persist_tail.clone());
                    ctx.session_store_redis_key = Some(redis_key.clone());
                    spawn_session_persist(
                        store,
                        redis_key,
                        persist_tail,
                        None::<String>,
                        features.mimo_session_store_ttl_secs,
                        features.mimo_session_store_max_messages,
                    );
                    warn!(
                        request_id = %ctx.request_id,
                        session_id = %session_id,
                        stored = stored_len,
                        client = client_len,
                        upstream_messages = upstream.len(),
                        upstream_bytes,
                        "session store prefix break; turn-based upstream and re-seeded client suffix"
                    );
                }
            }
        }
    }
    ctx.session_upstream_messages_len = ctx
        .session_upstream_messages_len
        .or_else(|| ctx.session_persist_base.as_ref().map(|m| m.len()));
}

pub fn spawn_session_persist(
    store: Arc<SessionStore>,
    redis_key: String,
    mut base_messages: Vec<Value>,
    assistant_content: impl Into<Option<String>>,
    ttl_secs: u64,
    max_messages: usize,
) {
    if let Some(content) = assistant_content.into().filter(|c| !c.is_empty()) {
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
    fn merge_tail_anchor_when_prefix_differs_but_recent_tail_matches() {
        let tail = [
            msg("user", "c"),
            msg("assistant", "d"),
            msg("user", "e"),
            msg("assistant", "f"),
        ];
        let mut stored = vec![msg("user", "old-prefix")];
        stored.extend_from_slice(&tail);
        let mut client = vec![msg("user", "new-prefix")];
        client.extend_from_slice(&tail);
        let merged = SessionStore::merge_messages(&stored, &client).expect("tail anchor");
        assert_eq!(merged.len(), 5);
    }

    #[test]
    fn merge_suffix_window_when_client_sends_full_history() {
        let full: Vec<Value> = (0..10)
            .map(|i| {
                msg(
                    if i % 2 == 0 { "user" } else { "assistant" },
                    &i.to_string(),
                )
            })
            .collect();
        let stored = full[6..].to_vec();
        let mut client = full.clone();
        client.push(msg("user", "new"));
        let merged = SessionStore::merge_messages(&stored, &client).expect("suffix merge");
        assert_eq!(merged.len(), 11);
    }

    #[test]
    fn shrink_messages_for_upstream_applies_keep_recent_turns() {
        let messages: Vec<Value> = (0..20)
            .map(|i| {
                msg(
                    if i % 2 == 0 { "user" } else { "assistant" },
                    &i.to_string(),
                )
            })
            .collect();
        let shrunk = SessionStore::shrink_messages_for_upstream(messages, true, 3, 200);
        assert!(shrunk.len() < 20);
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
    fn prepare_mimo_upstream_sanitizes_tool_chain() {
        let messages = vec![
            msg("assistant", "call"),
            serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_orphan",
                    "type": "function",
                    "function": { "name": "read_file", "arguments": "{}" }
                }]
            }),
            msg("user", "next"),
        ];
        let out = SessionStore::prepare_mimo_upstream_messages(messages, 6);
        assert!(
            !out.iter().any(|m| {
                m.get("tool_calls")
                    .and_then(|v| v.as_array())
                    .is_some_and(|a| !a.is_empty())
            }),
            "dangling tool_calls should be stripped"
        );
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
