use crab_cache::{CacheEntry, TieredCache, UsageInfo};
use crab_metrics::CacheTier;
use crab_reasoning::sanitize_client_completion;

pub fn prepare_response_body_for_cache(body: Vec<u8>, display_reasoning: bool) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    sanitize_client_completion(&mut value, display_reasoning, true);
    serde_json::to_vec(&value).unwrap_or(body)
}

pub fn build_cache_entry(
    response_body: Vec<u8>,
    model: String,
    ttl_secs: u64,
    is_stream: bool,
    client_display_reasoning: bool,
) -> CacheEntry {
    CacheEntry {
        response_body,
        model,
        usage: UsageInfo::default(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ttl_secs,
        sse_body: None,
        is_stream,
        client_display_reasoning,
    }
}

pub fn build_cache_entry_with_sse(
    response_body: Vec<u8>,
    sse_body: Vec<u8>,
    model: String,
    ttl_secs: u64,
    is_stream: bool,
    client_display_reasoning: bool,
) -> CacheEntry {
    CacheEntry {
        response_body,
        sse_body: Some(sse_body),
        model,
        usage: UsageInfo::default(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        ttl_secs,
        is_stream,
        client_display_reasoning,
    }
}

/// Whether to persist raw SSE bytes alongside the synthesized JSON completion.
pub fn should_store_sse_body(sse_len: usize, max_sse_cache_bytes: usize) -> bool {
    max_sse_cache_bytes > 0 && sse_len <= max_sse_cache_bytes
}

/// Legacy Redis entries default `is_stream = false`; mismatch forces a miss for streaming clients.
pub fn cache_entry_matches_stream_mode(entry: &CacheEntry, is_streaming: bool) -> bool {
    entry.is_stream == is_streaming
}

/// Build a stable concatenated query text for semantic cache from request messages.
pub fn build_semantic_query_text(messages: &[serde_json::Value]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "system" || role == "user" || role == "assistant" {
            let content = msg.get("content");
            let text = match content {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(arr)) => {
                    let sub: Vec<String> = arr
                        .iter()
                        .filter_map(|part| {
                            part.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if sub.is_empty() {
                        continue;
                    }
                    sub.join(" ")
                }
                _ => continue,
            };
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

/// Exact tiered cache lookup that enforces stream-mode matching.
///
/// Returns `Some((entry, tier))` only when the cached entry's `is_stream` flag
/// matches the client's streaming preference. This prevents a non-streaming
/// cached response from being served to a streaming client (and vice versa).
///
/// This is the Session-independent core extracted from `try_early_mimo_exact_cache`
/// to enable standalone unit testing.
pub async fn tiered_exact_lookup(
    tiered: &TieredCache,
    cache_key: &str,
    consumer: Option<&str>,
    domain: Option<&str>,
    is_streaming: bool,
) -> Option<(CacheEntry, CacheTier)> {
    let (entry, tier) = tiered.get(cache_key, consumer, domain).await?;
    if !cache_entry_matches_stream_mode(&entry, is_streaming) {
        return None;
    }
    Some((entry, tier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_response_body_for_cache_strips_reasoning_field() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "<details>\n<summary>Thinking</summary>\n\nthink\n</details>\n\nhi",
                    "reasoning_content": "secret"
                },
                "finish_reason": "stop"
            }]
        });
        let out = prepare_response_body_for_cache(body.to_string().into_bytes(), false);
        let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let msg = &parsed["choices"][0]["message"];
        assert!(msg.get("reasoning_content").is_none());
        assert_eq!(msg["content"].as_str(), Some("hi"));
    }

    // --- cache_entry_matches_stream_mode ---

    #[test]
    fn stream_mode_matches_when_flags_equal() {
        let entry = CacheEntry {
            response_body: vec![],
            model: "m".into(),
            usage: UsageInfo::default(),
            created_at: 0,
            ttl_secs: 60,
            sse_body: None,
            is_stream: false,
            client_display_reasoning: false,
        };
        assert!(cache_entry_matches_stream_mode(&entry, false));
        assert!(!cache_entry_matches_stream_mode(&entry, true));

        let stream_entry = CacheEntry {
            is_stream: true,
            ..entry
        };
        assert!(cache_entry_matches_stream_mode(&stream_entry, true));
        assert!(!cache_entry_matches_stream_mode(&stream_entry, false));
    }

    // --- build_semantic_query_text ---

    #[test]
    fn semantic_query_extracts_text_content() {
        let messages = vec![
            serde_json::json!({"role":"system","content":"You are helpful."}),
            serde_json::json!({"role":"user","content":"What is Rust?"}),
        ];
        let result = build_semantic_query_text(&messages);
        assert_eq!(result, Some("You are helpful.\nWhat is Rust?".to_string()));
    }

    #[test]
    fn semantic_query_skips_empty_messages() {
        let messages = vec![
            serde_json::json!({"role":"user","content":""}),
            serde_json::json!({"role":"assistant","content":""}),
        ];
        assert!(build_semantic_query_text(&messages).is_none());
    }

    #[test]
    fn semantic_query_handles_array_content() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": [{"type":"text","text":"hello"},{"type":"text","text":"world"}]
        })];
        let result = build_semantic_query_text(&messages);
        assert_eq!(result, Some("hello world".to_string()));
    }

    // --- tiered_exact_lookup ---

    /// TieredCache not available without Redis; test the stream-mode guard in isolation.
    /// The Redis roundtrip for `tiered_exact_lookup` is covered by
    /// `crates/crab-gateway/tests/data_plane.rs` (exact_key_roundtrip_l0_hit + stream flag).
    #[test]
    fn stream_mode_guard_prevents_mismatched_entry() {
        let non_stream = CacheEntry {
            response_body: vec![],
            model: "m".into(),
            usage: UsageInfo::default(),
            created_at: 0,
            ttl_secs: 60,
            sse_body: None,
            is_stream: false,
            client_display_reasoning: false,
        };
        // Mismatch: entry is non-stream, client wants stream.
        assert!(!cache_entry_matches_stream_mode(&non_stream, true));

        let stream = CacheEntry {
            is_stream: true,
            ..non_stream
        };
        // Match: both stream.
        assert!(cache_entry_matches_stream_mode(&stream, true));
        // Mismatch: entry is stream, client wants non-stream.
        assert!(!cache_entry_matches_stream_mode(&stream, false));
    }
}
