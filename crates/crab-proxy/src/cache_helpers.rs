use crab_cache::{CacheEntry, UsageInfo};
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
}
