/// Sanitized upstream error snippet for debug logs (no secrets).
pub fn upstream_error_preview(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
        if let Some(msg) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return msg.chars().take(300).collect();
        }
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return msg.chars().take(300).collect();
        }
    }
    s.chars().take(300).collect()
}

pub fn upstream_pool_exhausted_error_json() -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": "All upstream DeepSeek API keys are rate-limited or disabled. Retry after cooldown or add keys via CRABCACHE_UPSTREAM_KEYS.",
            "type": "upstream_key_exhausted",
            "code": "upstream_key_exhausted",
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}

pub fn client_concurrency_exceeded_error_json() -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": "Too many concurrent requests for this API key. Retry when in-flight requests complete or raise max_concurrent via the Management API.",
            "type": "rate_limit_error",
            "code": "client_concurrency_exceeded",
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}

pub fn coalesce_leader_failed_error_json() -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": "Upstream request failed while coalesced peers were waiting; no duplicate upstream call was made.",
            "type": "upstream_error",
            "code": "coalesce_leader_failed",
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}

pub fn missing_reasoning_error_json(missing_count: usize) -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": format!(
                "CrabCache cannot satisfy DeepSeek thinking-mode requirements: reasoning_content is still \
                 missing for {missing_count} assistant message(s) after ReasoningStore fill and history recover. \
                 Clear the reasoning cache (DELETE /v1/reasoning/cache), retry once so the gateway can store \
                 reasoning from a successful response, or temporarily set missing_reasoning_strategy to \"recover\". \
                 Ensure [reasoning].backend = \"redis\" for multi-instance/sub-agent retries."
            ),
            "type": "missing_reasoning_content",
            "code": "missing_reasoning_content",
            "missing_reasoning_messages": missing_count,
        }
    });
    serde_json::to_vec(&body).unwrap_or_default()
}
