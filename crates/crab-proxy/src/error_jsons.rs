use crate::upstream_pool::PoolAcquireFailure;

/// OpenAI Chat Completions SSE for `stream: true` clients (Cursor reads assistant deltas).
pub fn format_openai_error_sse_for_client(error_json: &[u8], model: &str) -> Vec<u8> {
    let parsed: serde_json::Value =
        serde_json::from_slice(error_json).unwrap_or(serde_json::json!({}));
    let msg = parsed
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("Upstream request failed");
    let code = parsed
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_str())
        .unwrap_or("upstream_error");
    let display = format!("[CrabCache] {msg} (code: {code})");
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let id = format!("chatcmpl-err-{}", &uuid::Uuid::new_v4().to_string()[..8]);

    let content_chunk = serde_json::json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": display },
            "finish_reason": null,
        }],
    });
    let finish_chunk = serde_json::json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop",
        }],
    });

    format!(
        "data: {content_chunk}\n\ndata: {finish_chunk}\n\ndata: [DONE]\n\n"
    )
    .into_bytes()
}

/// OpenAI Responses API SSE error (Codex CLI wire).
pub fn format_responses_error_sse_for_client(error_json: &[u8]) -> Vec<u8> {
    let line = String::from_utf8_lossy(error_json).trim().to_string();
    format!("event: error\ndata: {line}\n\n").into_bytes()
}

/// Pick SSE error shape for downstream wire API.
pub fn format_client_error_sse(error_json: &[u8], model: &str, wire: crate::context::ClientWireApi) -> Vec<u8> {
    match wire {
        crate::context::ClientWireApi::Responses => format_responses_error_sse_for_client(error_json),
        crate::context::ClientWireApi::ChatCompletions => {
            format_openai_error_sse_for_client(error_json, model)
        }
    }
}

/// Upstream body → OpenAI error JSON → SSE chunks visible in Cursor.
pub fn format_upstream_error_sse_for_client(body: &[u8], status: u16, model: &str) -> Vec<u8> {
    let json = format_upstream_error_for_client(body, status);
    format_openai_error_sse_for_client(&json, model)
}

/// Rewrite upstream JSON (Codex `detail`, OpenAI `error`, etc.) into OpenAI Chat Completions error JSON.
pub fn format_upstream_error_for_client(body: &[u8], status: u16) -> Vec<u8> {
    let code = match status {
        401 => "invalid_api_key",
        403 => "permission_denied",
        429 => "rate_limit_exceeded",
        _ => "invalid_request_error",
    };
    let fallback_type = if status == 429 {
        "rate_limit_error"
    } else if status >= 500 {
        "server_error"
    } else {
        "invalid_request_error"
    };

    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
        if v.get("error").is_some() {
            return body.to_vec();
        }
        if let Some(detail) = v.get("detail").and_then(|d| d.as_str()) {
            return serde_json::to_vec(&serde_json::json!({
                "error": {
                    "message": detail,
                    "type": fallback_type,
                    "code": code,
                }
            }))
            .unwrap_or_else(|_| body.to_vec());
        }
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return serde_json::to_vec(&serde_json::json!({
                "error": {
                    "message": msg,
                    "type": fallback_type,
                    "code": code,
                }
            }))
            .unwrap_or_else(|_| body.to_vec());
        }
    }

    let preview = upstream_error_preview(body);
    if preview.is_empty() {
        body.to_vec()
    } else {
        serde_json::to_vec(&serde_json::json!({
            "error": {
                "message": preview,
                "type": fallback_type,
                "code": code,
            }
        }))
        .unwrap_or_else(|_| body.to_vec())
    }
}

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
        if let Some(detail) = v.get("detail").and_then(|d| d.as_str()) {
            return detail.chars().take(300).collect();
        }
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return msg.chars().take(300).collect();
        }
    }
    s.chars().take(300).collect()
}

#[inline]
pub fn upstream_pool_exhausted_error_json() -> Vec<u8> {
    let (body, _, _) = upstream_pool_exhausted_error_details(PoolAcquireFailure::Unavailable);
    body
}

/// Returns `(body, code, retry_after_secs)` for a `PoolAcquireFailure`.
pub fn upstream_pool_exhausted_error_details(
    failure: PoolAcquireFailure,
) -> (Vec<u8>, &'static str, u64) {
    let (code, message, retry_after) = match failure {
        PoolAcquireFailure::Empty => (
            "upstream_key_exhausted",
            "No upstream API keys configured. Add keys via the Management API (PUT /v1/upstream/keys) or CRABCACHE_UPSTREAM_KEYS.",
            60,
        ),
        PoolAcquireFailure::AllDisabled => (
            "upstream_keys_disabled",
            "All upstream API keys are disabled. Re-enable keys via the Management API (PATCH /v1/upstream/keys/{id}).",
            60,
        ),
        PoolAcquireFailure::AllInCooldown { min_retry_secs } => (
            "upstream_keys_in_cooldown",
            "All upstream API keys are rate-limited and in cooldown. Retry after the cooldown expires.",
            min_retry_secs.max(1),
        ),
        PoolAcquireFailure::Unavailable => (
            "upstream_key_exhausted",
            "No upstream API keys available. Keys may be disabled, in cooldown, or missing. Check the Management API (GET /v1/upstream/keys).",
            60,
        ),
    };
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": "upstream_key_exhausted",
            "code": code,
        }
    });
    (
        serde_json::to_vec(&body).unwrap_or_default(),
        code,
        retry_after,
    )
}

pub fn deepseek_user_concurrency_exceeded_error_json() -> Vec<u8> {
    let body = serde_json::json!({
        "error": {
            "message": "Too many concurrent DeepSeek requests for this project_id (user_id). Retry when in-flight requests complete or raise limits in [upstream.deepseek_user_concurrency].",
            "type": "rate_limit_error",
            "code": "deepseek_user_concurrency_exceeded",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_responses_error_sse_uses_event_error() {
        let json = br#"{"error":{"message":"bad key","type":"invalid_request_error"}}"#;
        let out = format_responses_error_sse_for_client(json);
        let s = String::from_utf8_lossy(&out);
        assert!(s.starts_with("event: error"));
        assert!(s.contains("invalid_request_error"));
    }

    #[test]
    fn format_upstream_error_sse_includes_assistant_content() {
        let body = br#"{"detail":"Unsupported parameter: conversation_id"}"#;
        let out = format_upstream_error_sse_for_client(body, 400, "gpt-5.5");
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("chat.completion.chunk"));
        assert!(s.contains("Unsupported parameter"));
        assert!(s.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn format_codex_detail_error_for_client() {
        let body = br#"{"detail":"The 'gpt-5.3-codex' model is not supported when using Codex with a ChatGPT account."}"#;
        let out = format_upstream_error_for_client(body, 400);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["error"]["type"], "invalid_request_error");
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not supported"));
    }

    #[test]
    fn missing_reasoning_error_json_shape() {
        let body = missing_reasoning_error_json(2);
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value["error"]["code"].as_str(),
            Some("missing_reasoning_content")
        );
        assert_eq!(
            value["error"]["missing_reasoning_messages"].as_u64(),
            Some(2)
        );
    }
}
