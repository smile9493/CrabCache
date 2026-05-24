use bytes::Bytes;
use pingora_http::ResponseHeader;
use pingora_proxy::Session;
use tracing::warn;

use crab_cache::CacheEntry;
use crab_metrics::CacheTier;

use crate::debug_log::debug_agent_log;

fn cache_status_header(tier: CacheTier) -> &'static str {
    match tier {
        CacheTier::L0Moka => "HIT_L0",
        CacheTier::L1Redis => "HIT_L1",
        CacheTier::L2Semantic => "HIT_L2",
        CacheTier::Miss => "miss",
    }
}

fn insert_response_header(
    header: &mut ResponseHeader,
    name: &'static str,
    value: impl ToString,
) -> Option<()> {
    header.insert_header(name, value.to_string()).ok()
}

fn build_json_response_header(body_len: usize, cache_tier: CacheTier) -> Option<ResponseHeader> {
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

fn build_sse_response_header(body_len: usize, cache_tier: CacheTier) -> Option<ResponseHeader> {
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

/// Cache hit streaming: prefer stored client-shaped `sse_body` from a prior miss.
pub async fn send_cached_response(
    session: &mut Session,
    entry: &CacheEntry,
    model: &str,
    is_streaming: bool,
    cache_tier: CacheTier,
    display_reasoning: bool,
) -> bool {
    let response_body = &entry.response_body;
    if is_streaming {
        let display_mismatch = entry.client_display_reasoning != display_reasoning;
        let used_legacy_regen = entry.sse_body.as_ref().is_some_and(|s| {
            s.windows(b"reasoning_content".len())
                .any(|w| w == b"reasoning_content")
        });
        let saved_empty_content = entry
            .sse_body
            .as_ref()
            .is_some_and(|s| !cached_sse_has_nonempty_content(s));
        let force_regen = display_mismatch || used_legacy_regen || saved_empty_content;
        let sse_source = if entry.sse_body.is_none() || force_regen {
            if display_mismatch {
                "display_mismatch_regen"
            } else if used_legacy_regen {
                "legacy_regen"
            } else if saved_empty_content {
                "empty_content_regen"
            } else {
                "json_regen"
            }
        } else {
            "saved_sse"
        };
        let sse_body = if entry.sse_body.is_some() && !force_regen {
            entry.sse_body.clone().unwrap_or_default()
        } else {
            json_to_sse_stream(response_body, model, display_reasoning)
        };
        let json_choices = serde_json::from_slice::<serde_json::Value>(response_body)
            .ok()
            .and_then(|v| v.get("choices").and_then(|c| c.as_array()).map(|a| a.len()))
            .unwrap_or(0);
        let has_done = sse_body.windows(6).any(|w| w == b"[DONE]");
        let has_nonempty = cached_sse_has_nonempty_content(&sse_body);
        debug_agent_log(
            "H1",
            "cache_response.rs:send_cached_response",
            "streaming cache hit SSE synthesis",
            serde_json::json!({
                "sse_source": sse_source,
                "sse_len": sse_body.len(),
                "response_body_len": response_body.len(),
                "display_mismatch": display_mismatch,
                "used_legacy_regen": used_legacy_regen,
                "saved_empty_content": saved_empty_content,
                "json_choices": json_choices,
                "has_done": has_done,
                "has_nonempty_content": has_nonempty,
                "entry_is_stream": entry.is_stream,
                "entry_client_display_reasoning": entry.client_display_reasoning,
                "request_display_reasoning": display_reasoning,
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

pub fn json_to_sse_stream(json_body: &[u8], model: &str, display_reasoning: bool) -> Vec<u8> {
    use serde_json::json;

    let value: serde_json::Value = match serde_json::from_slice(json_body) {
        Ok(v) => v,
        Err(_) => {
            debug_agent_log(
                "H3",
                "cache_response.rs:json_to_sse_stream",
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
            "delta": message_to_cursor_safe_delta(choice.get("message"), display_reasoning),
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
        "cache_response.rs:json_to_sse_stream",
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

pub fn message_to_cursor_safe_delta(
    message: Option<&serde_json::Value>,
    display_reasoning: bool,
) -> serde_json::Value {
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
    let effective = if display_reasoning
        && content_str.is_empty()
        && !reasoning_str.is_empty()
    {
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

pub fn cached_sse_has_nonempty_content(sse: &[u8]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crab_cache::{CacheEntry, UsageInfo};

    fn sample_entry(client_display: bool, sse: Option<Vec<u8>>) -> CacheEntry {
        CacheEntry {
            response_body: br#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#.to_vec(),
            model: "deepseek-v4-pro".into(),
            usage: UsageInfo::default(),
            created_at: 1,
            ttl_secs: 3600,
            sse_body: sse,
            is_stream: true,
            client_display_reasoning: client_display,
        }
    }

    #[test]
    fn display_mismatch_forces_regen_even_with_saved_sse() {
        let saved = br#"data: {"choices":[{"delta":{"content":"old fold"}}]}

data: [DONE]

"#
        .to_vec();
        let entry = sample_entry(true, Some(saved));
        let regen = if entry.client_display_reasoning != false {
            json_to_sse_stream(&entry.response_body, "deepseek-v4-pro", false)
        } else {
            entry.sse_body.clone().unwrap()
        };
        assert!(String::from_utf8(regen).unwrap().contains("ok"));
    }
}
