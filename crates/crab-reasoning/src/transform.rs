use crate::backend::ReasoningBackend;
use crate::streaming::{
    CursorReasoningDisplayAdapter, StreamAccumulator, fold_reasoning_into_content,
};
use serde_json::Value;

pub struct RecoveryNoticeContent(pub String);

/// Map `delta.reasoning_content` → incremental `delta.content` for OpenAI-compatible clients.
fn mirror_reasoning_delta_incremental(chunk: &mut Value) {
    let choices = match chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
        Some(c) => c,
        None => return,
    };
    for choice in choices {
        let delta = match choice.get_mut("delta").and_then(|d| d.as_object_mut()) {
            Some(d) => d,
            None => continue,
        };
        if let Some(rc) = delta.remove("reasoning_content") {
            let text = rc.as_str().unwrap_or("").to_string();
            if !text.is_empty() {
                delta.insert("content".into(), Value::String(text));
            } else if delta.get("role").is_some() && !delta.contains_key("content") {
                delta.insert("content".into(), Value::String(String::new()));
            }
        } else if delta.get("content").map(|v| v.is_null()).unwrap_or(false) {
            delta.insert("content".into(), Value::String(String::new()));
        }
        // Strip stray null/absent reasoning field if upstream re-inserted it.
        delta.remove("reasoning_content");
    }
}

pub fn record_response_reasoning(
    response_payload: &Value,
    store: Option<&ReasoningBackend>,
    _request_messages: &[Value],
    cache_namespace: &str,
    recording_contexts: &[(String, Vec<Value>)],
) -> usize {
    let store = match store {
        Some(s) => s,
        None => return 0,
    };
    let choices = match response_payload.get("choices").and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return 0,
    };
    let mut stored = 0;
    for choice in choices {
        if !choice.is_object() {
            continue;
        }
        if let Some(message) = choice.get("message") {
            for (scope, prior_messages) in recording_contexts {
                stored +=
                    store.store_assistant_message(message, scope, cache_namespace, prior_messages);
            }
        }
    }
    stored
}

pub fn rewrite_response_body(
    body: &[u8],
    original_model: &str,
    store: Option<&ReasoningBackend>,
    request_messages: &[Value],
    cache_namespace: &str,
    content_prefix: Option<&str>,
    recording_contexts: &[(String, Vec<Value>)],
    display_reasoning: bool,
    collapsible_reasoning: bool,
) -> Option<Vec<u8>> {
    let mut response_payload: Value = serde_json::from_slice(body).ok()?;
    if let Some(obj) = response_payload.as_object_mut() {
        if let Some(prefix) = content_prefix {
            prefix_response_content(obj, prefix);
        }
    }
    record_response_reasoning(
        &response_payload,
        store,
        request_messages,
        cache_namespace,
        recording_contexts,
    );
    if display_reasoning {
        fold_reasoning_into_content(&mut response_payload, collapsible_reasoning);
    }
    if let Some(obj) = response_payload.as_object_mut() {
        if let Some(model) = obj.get_mut("model") {
            *model = Value::String(original_model.to_string());
        }
    }
    Some(serde_json::to_vec(&response_payload).unwrap_or_default())
}

fn prefix_response_content(
    response_payload: &mut serde_json::Map<String, Value>,
    prefix: &str,
) -> bool {
    let choices = match response_payload
        .get_mut("choices")
        .and_then(|c| c.as_array_mut())
    {
        Some(c) => c,
        None => return false,
    };
    for choice in choices.iter_mut() {
        if !choice.is_object() {
            continue;
        }
        let message = match choice.get_mut("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };
        if let Some(obj) = message.as_object_mut() {
            let content = obj
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            obj.insert(
                "content".into(),
                Value::String(format!("{prefix}{content}")),
            );
            return true;
        }
    }
    false
}

pub struct SseRewriteResult {
    pub rewritten_line: Vec<u8>,
    pub finalized: bool,
    pub pending_recovery_notice: Option<String>,
    pub chunk_usage: Option<Value>,
}

fn sse_data(payload: &Value) -> Vec<u8> {
    let json = serde_json::to_string(payload).unwrap_or_default();
    format!("data: {json}\n\n").into_bytes()
}

fn recovery_notice_chunk(model: &str, notice: &str) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    serde_json::json!({
        "id": "chatcmpl-deepseek-cursor-proxy-recovery",
        "object": "chat.completion.chunk",
        "created": now,
        "model": model,
        "choices": [{"index": 0, "delta": {"content": notice}, "finish_reason": Value::Null}]
    })
}

fn inject_recovery_notice(chunk: &mut Value, notice: &str) -> bool {
    let choices = match chunk.get_mut("choices").and_then(|c| c.as_array_mut()) {
        Some(c) => c,
        None => return false,
    };
    for choice in choices.iter_mut() {
        if !choice.is_object() {
            continue;
        }
        let (has_content, has_tool_calls) = {
            let delta = match choice.get("delta") {
                Some(d) if d.is_object() => d,
                _ => continue,
            };
            (
                delta.get("content").is_some(),
                delta.get("tool_calls").is_some(),
            )
        };
        if !has_content && !has_tool_calls {
            continue;
        }
        if let Some(delta) = choice.get_mut("delta") {
            if let Some(obj) = delta.as_object_mut() {
                let existing = obj
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                obj.insert(
                    "content".into(),
                    Value::String(format!("{notice}{existing}")),
                );
                return true;
            }
        }
    }
    false
}

pub fn rewrite_sse_chunk(
    line: &[u8],
    original_model: &str,
    accumulator: &mut StreamAccumulator,
    cache_namespace: &str,
    response_contexts: &[(String, Vec<Value>)],
    display_adapter: &mut Option<CursorReasoningDisplayAdapter>,
    pending_recovery_notice: Option<&str>,
    store: Option<&ReasoningBackend>,
) -> SseRewriteResult {
    let stripped = {
        let start = line
            .iter()
            .position(|&b| !b" \t\r\n".contains(&b))
            .unwrap_or(0);
        let end = line
            .iter()
            .rposition(|&b| !b" \t\r\n".contains(&b))
            .map(|p| p + 1)
            .unwrap_or(line.len());
        &line[start..end]
    };
    if !stripped.starts_with(b"data:") {
        return SseRewriteResult {
            rewritten_line: line.to_vec(),
            finalized: false,
            pending_recovery_notice: pending_recovery_notice.map(String::from),
            chunk_usage: None,
        };
    }
    let data = &stripped[b"data:".len()..];
    let data = data.trim_ascii_start();

    if data == b"[DONE]" {
        if let Some(store) = store {
            let stored: usize = response_contexts
                .iter()
                .map(|(scope, prior_messages)| {
                    accumulator.store_reasoning(store, scope, cache_namespace, prior_messages)
                })
                .sum();
            if stored > 0 {
                tracing::debug!(
                    stored = stored,
                    "Stored streaming reasoning cache keys on [DONE]"
                );
            }
        }
        let mut prefix = Vec::new();
        if display_adapter.is_none() {
            if let Some(notice) = pending_recovery_notice {
                prefix.extend_from_slice(&sse_data(&recovery_notice_chunk(original_model, notice)));
            }
            prefix.extend_from_slice(b"data: [DONE]\n\n");
            return SseRewriteResult {
                rewritten_line: prefix,
                finalized: true,
                pending_recovery_notice: None,
                chunk_usage: None,
            };
        }
        let closing_chunk = display_adapter
            .as_mut()
            .and_then(|a| a.flush_chunk(original_model));
        if let Some(chunk) = closing_chunk {
            prefix.extend_from_slice(&sse_data(&chunk));
        }
        if let Some(notice) = pending_recovery_notice {
            prefix.extend_from_slice(&sse_data(&recovery_notice_chunk(original_model, notice)));
        }
        prefix.extend_from_slice(b"data: [DONE]\n\n");
        return SseRewriteResult {
            rewritten_line: prefix,
            finalized: true,
            pending_recovery_notice: None,
            chunk_usage: None,
        };
    }

    let mut chunk: Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => {
            return SseRewriteResult {
                rewritten_line: line.to_vec(),
                finalized: false,
                pending_recovery_notice: pending_recovery_notice.map(String::from),
                chunk_usage: None,
            };
        }
    };
    if !chunk.is_object() {
        return SseRewriteResult {
            rewritten_line: line.to_vec(),
            finalized: false,
            pending_recovery_notice: pending_recovery_notice.map(String::from),
            chunk_usage: None,
        };
    }

    let mut notice = pending_recovery_notice.map(String::from);
    if notice.is_some() && inject_recovery_notice(&mut chunk, notice.as_deref().unwrap_or("")) {
        notice = None;
    }
    let chunk_usage = chunk.get("usage").cloned();
    if let Some(adapter) = display_adapter.as_mut() {
        adapter.rewrite_chunk(&mut chunk);
    } else {
        mirror_reasoning_delta_incremental(&mut chunk);
    }
    // Ingest after client-shaped rewrite so stream cache JSON includes mirrored content.
    accumulator.ingest_chunk(&chunk);
    if let Some(store) = store {
        let stored: usize = response_contexts
            .iter()
            .map(|(scope, prior_messages)| {
                accumulator.store_ready_reasoning(store, scope, cache_namespace, prior_messages)
            })
            .sum();
        if stored > 0 {
            tracing::debug!(
                stored = stored,
                "Stored streaming reasoning cache keys mid-stream"
            );
        }
    }
    if let Some(obj) = chunk.get_mut("model") {
        *obj = Value::String(original_model.to_string());
    }
    let ending = if line.ends_with(b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let json = serde_json::to_string(&chunk).unwrap_or_default();
    let rewritten = format!("data: {json}{ending}").into_bytes();

    SseRewriteResult {
        rewritten_line: rewritten,
        finalized: false,
        pending_recovery_notice: notice,
        chunk_usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_response_body_basic() {
        let body = r#"{"model":"deepseek-v4-pro","choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        let result = rewrite_response_body(
            body.as_bytes(),
            "deepseek-v4-pro",
            None,
            &[],
            "",
            None,
            &[],
            false,
            false,
        );
        assert!(result.is_some());
        let parsed: Value = serde_json::from_slice(&result.unwrap()).unwrap();
        assert_eq!(
            parsed.get("model").unwrap().as_str(),
            Some("deepseek-v4-pro")
        );
    }

    #[test]
    fn rewrite_sse_strips_reasoning_content_without_display_adapter() {
        let payload = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": "think", "role": "assistant"}
            }]
        });
        let line = format!("data: {payload}\n\n");
        let mut acc = StreamAccumulator::new();
        let result = rewrite_sse_chunk(
            line.as_bytes(),
            "deepseek-v4-pro",
            &mut acc,
            "",
            &[],
            &mut None,
            None,
            None,
        );
        let body = std::str::from_utf8(&result.rewritten_line).unwrap();
        assert!(!body.contains("reasoning_content"));
        assert!(body.contains(r#""content":"think""#) || body.contains(r#""content": "think""#));
    }

    #[test]
    fn test_rewrite_sse_done() {
        let mut acc = StreamAccumulator::new();
        let result = rewrite_sse_chunk(
            b"data: [DONE]\n\n",
            "deepseek-v4-pro",
            &mut acc,
            "",
            &[],
            &mut None,
            None,
            None,
        );
        assert!(result.finalized);
    }
}
