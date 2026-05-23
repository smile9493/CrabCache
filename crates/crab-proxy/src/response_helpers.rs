use bytes::Bytes;
use pingora_http::ResponseHeader;
use pingora_proxy::Session;
use tracing::warn;
use crate::debug_log::debug_agent_log;

use crab_cache::CacheEntry;
use crab_metrics::CacheTier;

async fn send_json_error_inner(
    session: &mut Session,
    status: http::StatusCode,
    body: &[u8],
    retry_after_secs: Option<u64>,
) -> bool {
    let mut header = match ResponseHeader::build(status, Some(8)) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let _ = header.insert_header("content-type", "application/json");
    let _ = header.insert_header("content-length", body.len().to_string());
    let _ = header.insert_header("connection", "close");
    if let Some(secs) = retry_after_secs {
        let _ = header.insert_header("retry-after", secs.to_string());
    }
    if session
        .downstream_session
        .write_response_header(Box::new(header))
        .await
        .is_err()
    {
        return false;
    }
    session
        .downstream_session
        .write_response_body(Bytes::copy_from_slice(body), true)
        .await
        .is_ok()
}

pub async fn send_json_error(session: &mut Session, status: http::StatusCode, body: &[u8]) -> bool {
    send_json_error_inner(session, status, body, None).await
}

pub async fn send_json_error_with_retry_after(
    session: &mut Session,
    status: http::StatusCode,
    body: &[u8],
    retry_after_secs: u64,
) -> bool {
    send_json_error_inner(session, status, body, Some(retry_after_secs)).await
}

pub async fn send_json_ok(session: &mut Session, body: &[u8]) -> bool {
    send_json_error_inner(session, http::StatusCode::OK, body, None).await
}

pub async fn send_cors_preflight(session: &mut Session) -> bool {
    let mut header = match ResponseHeader::build(http::StatusCode::NO_CONTENT, Some(8)) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let _ = header.insert_header("access-control-allow-origin", "*");
    let _ = header.insert_header("access-control-allow-methods", "GET, POST, OPTIONS");
    let _ = header.insert_header(
        "access-control-allow-headers",
        "Authorization, Content-Type, X-Request-Id, X-Conversation-Id, X-Consumer, X-Project-Id",
    );
    let _ = header.insert_header("access-control-max-age", "86400");
    let _ = header.insert_header("content-length", "0");
    session
        .downstream_session
        .write_response_header(Box::new(header))
        .await
        .is_ok()
}

fn cache_status_header(tier: CacheTier) -> &'static str {
    match tier {
        CacheTier::L0Moka => "HIT_L0",
        CacheTier::L1Redis => "HIT_L1",
        CacheTier::L2Semantic => "HIT_L2",
        CacheTier::Miss => "miss",
    }
}

fn insert_response_header(
    header: &mut pingora_http::ResponseHeader,
    name: &'static str,
    value: impl ToString,
) -> Option<()> {
    header.insert_header(name, value.to_string()).ok()
}

pub(crate) fn build_json_response_header(
    body_len: usize,
    cache_tier: CacheTier,
) -> Option<pingora_http::ResponseHeader> {
    let mut header = ResponseHeader::build(http::StatusCode::OK, Some(5)).ok()?;
    insert_response_header(&mut header, "content-type", "application/json")?;
    insert_response_header(&mut header, "content-length", body_len.to_string())?;
    insert_response_header(
        &mut header,
        "x-cache-status",
        cache_status_header(cache_tier),
    )?;
    insert_response_header(&mut header, "connection", "close")?;
    Some(header)
}

pub(crate) fn build_sse_response_header(
    body_len: usize,
    cache_tier: CacheTier,
) -> Option<pingora_http::ResponseHeader> {
    let mut header = ResponseHeader::build(http::StatusCode::OK, Some(5)).ok()?;
    insert_response_header(&mut header, "content-type", "text/event-stream")?;
    insert_response_header(&mut header, "content-length", body_len.to_string())?;
    insert_response_header(&mut header, "cache-control", "no-cache")?;
    insert_response_header(
        &mut header,
        "x-cache-status",
        cache_status_header(cache_tier),
    )?;
    insert_response_header(&mut header, "connection", "close")?;
    Some(header)
}

/// Cache hit streaming: prefer stored `sse_body` (already client-shaped from a prior miss).
/// Fallback `json_to_sse_stream` strips `reasoning_content` from message deltas for Cursor.
pub async fn send_cached_response(
    session: &mut Session,
    entry: &CacheEntry,
    model: &str,
    is_streaming: bool,
    cache_tier: CacheTier,
) -> bool {
    let response_body = &entry.response_body;
    if is_streaming {
        let used_legacy_regen = entry
            .sse_body
            .as_ref()
            .is_some_and(|s| s.windows(b"reasoning_content".len()).any(|w| w == b"reasoning_content"));
        let saved_empty_content = entry
            .sse_body
            .as_ref()
            .is_some_and(|s| !cached_sse_has_nonempty_content(s));
        let sse_source = if entry.sse_body.is_none() {
            "json_regen"
        } else if used_legacy_regen || saved_empty_content {
            "legacy_regen"
        } else {
            "saved_sse"
        };
        let sse_body = if let Some(ref saved) = entry.sse_body {
            if used_legacy_regen || saved_empty_content {
                // Legacy or empty-content SSE: rebuild from JSON with reasoning folded into content.
                json_to_sse_stream(response_body, model)
            } else {
                saved.clone()
            }
        } else {
            json_to_sse_stream(response_body, model)
        };
        let json_choices = serde_json::from_slice::<serde_json::Value>(response_body)
            .ok()
            .and_then(|v| v.get("choices").and_then(|c| c.as_array()).map(|a| a.len()))
            .unwrap_or(0);
        let has_done = sse_body.windows(6).any(|w| w == b"[DONE]");
        let has_nonempty = cached_sse_has_nonempty_content(&sse_body);
        debug_agent_log(
            "H1",
            "proxy.rs:send_cached_response",
            "streaming cache hit SSE synthesis",
            serde_json::json!({
                "sse_source": sse_source,
                "sse_len": sse_body.len(),
                "response_body_len": response_body.len(),
                "used_legacy_regen": used_legacy_regen,
                "saved_empty_content": saved_empty_content,
                "json_choices": json_choices,
                "has_done": has_done,
                "has_nonempty_content": has_nonempty,
                "entry_is_stream": entry.is_stream,
                "cache_tier": cache_tier.as_str(),
                "model": model,
            }),
        );
        let Some(header) = build_sse_response_header(sse_body.len(), cache_tier) else {
            warn!("Failed to build SSE cache response header");
            return false;
        };
        let _ = session
            .downstream_session
            .write_response_header(Box::new(header))
            .await;
        let _ = session
            .downstream_session
            .write_response_body(Bytes::from(sse_body), true)
            .await;
    } else {
        let Some(header) = build_json_response_header(response_body.len(), cache_tier) else {
            warn!("Failed to build JSON cache response header");
            return false;
        };
        let _ = session
            .downstream_session
            .write_response_header(Box::new(header))
            .await;
        let _ = session
            .downstream_session
            .write_response_body(Bytes::from(response_body.clone()), true)
            .await;
    }
    true
}

pub(crate) fn json_to_sse_stream(json_body: &[u8], model: &str) -> Vec<u8> {
    use serde_json::json;

    let value: serde_json::Value = match serde_json::from_slice(json_body) {
        Ok(v) => v,
        Err(_) => {
            debug_agent_log(
                "H3",
                "proxy.rs:json_to_sse_stream",
                "json parse failed, returning raw bytes",
                serde_json::json!({
                    "json_len": json_body.len(),
                    "model": model,
                }),
            );
            return json_body.to_vec();
        }
    };

    let choices = value
        .get("choices")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let usage = value.get("usage").cloned();
    let has_usage = usage.is_some();

    let mut sse_output = Vec::new();

    for (idx, choice) in choices.iter().enumerate() {
        let delta = json!({
            "index": idx,
            "delta": message_to_cursor_safe_delta(choice.get("message")),
            "finish_reason": choice.get("finish_reason").cloned().unwrap_or(serde_json::Value::Null)
        });

        let event_data = json!({
            "id": format!("chatcmpl-cache-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion.chunk",
            "created": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "model": model,
            "choices": [delta]
        });

        sse_output.extend_from_slice(format!("data: {event_data}\n\n").as_bytes());
    }

    if let Some(usage_data) = usage {
        let usage_event = json!({
            "id": format!("chatcmpl-cache-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion.chunk",
            "created": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "model": model,
            "choices": [],
            "usage": usage_data
        });
        sse_output.extend_from_slice(format!("data: {usage_event}\n\n").as_bytes());
    }

    sse_output.extend_from_slice(b"data: [DONE]\n\n");

    debug_agent_log(
        "H3",
        "proxy.rs:json_to_sse_stream",
        "sse synthesized from json",
        serde_json::json!({
            "choices_count": choices.len(),
            "sse_len": sse_output.len(),
            "has_usage": has_usage,
            "model": model,
        }),
    );

    sse_output
}

/// OpenAI-style delta for cache-hit SSE synthesis: no `reasoning_content` (Cursor rejects it).
pub(crate) fn message_to_cursor_safe_delta(message: Option<&serde_json::Value>) -> serde_json::Value {
    use serde_json::{Value, json};
    let Some(msg) = message else {
        return json!({});
    };
    let Some(obj) = msg.as_object() else {
        return msg.clone();
    };
    let mut delta = serde_json::Map::new();
    if let Some(role) = obj.get("role") {
        delta.insert("role".into(), role.clone());
    }
    let content_str = obj
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let reasoning_str = obj
        .get("reasoning_content")
        .and_then(|r| r.as_str())
        .unwrap_or("");
    let effective = if content_str.is_empty() && !reasoning_str.is_empty() {
        reasoning_str
    } else {
        content_str
    };
    if !effective.is_empty() || obj.contains_key("content") || obj.contains_key("reasoning_content")
    {
        delta.insert("content".into(), Value::String(effective.to_string()));
    }
    if let Some(tool_calls) = obj.get("tool_calls") {
        delta.insert("tool_calls".into(), tool_calls.clone());
    }
    Value::Object(delta)
}

/// True if any SSE `data:` line has a non-empty `choices[].delta.content`.
pub(crate) fn cached_sse_has_nonempty_content(sse: &[u8]) -> bool {
    for line in sse.split(|b| *b == b'\n') {
        let stripped = line.trim_ascii();
        if !stripped.starts_with(b"data:") {
            continue;
        }
        let data = stripped[b"data:".len()..].trim_ascii();
        if data == b"[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
            continue;
        };
        let Some(choices) = value.get("choices").and_then(|c| c.as_array()) else {
            continue;
        };
        for choice in choices {
            if let Some(content) = choice
                .get("delta")
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                if !content.is_empty() {
                    return true;
                }
            }
        }
    }
    false
}
