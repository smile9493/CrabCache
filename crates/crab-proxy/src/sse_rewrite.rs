use crab_reasoning::{
    CursorReasoningDisplayAdapter, ReasoningBackend, StreamAccumulator, rewrite_sse_chunk,
    strip_silent_sse_chunk_for_client,
};
use crab_reasoning::PreparedRequest;
use serde_json::Value;

/// Best-effort silent strip when streaming bypasses `prepare_upstream_request` (no accumulator).
pub fn apply_silent_strip_to_sse_chunk(chunk: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in chunk.split_inclusive(|&b| b == b'\n') {
        let start = line
            .iter()
            .position(|&b| !b" \t\r\n".contains(&b))
            .unwrap_or(0);
        let end = line
            .iter()
            .rposition(|&b| !b" \t\r\n".contains(&b))
            .map(|p| p + 1)
            .unwrap_or(line.len());
        let stripped_line = &line[start..end];
        if stripped_line.starts_with(b"data:") {
            let data = stripped_line[b"data:".len()..].trim_ascii_start();
            if data != b"[DONE]" {
                if let Ok(mut payload) = serde_json::from_slice::<Value>(data) {
                    if payload.is_object() {
                        strip_silent_sse_chunk_for_client(&mut payload);
                        let ending = if line.ends_with(b"\r\n") {
                            "\r\n"
                        } else {
                            "\n"
                        };
                        let json = serde_json::to_string(&payload).unwrap_or_default();
                        out.extend_from_slice(format!("data: {json}{ending}").as_bytes());
                        continue;
                    }
                }
            }
        }
        out.extend_from_slice(line);
    }
    out
}

/// Rewrite upstream SSE lines for OpenAI-compatible clients (mirror reasoning into `content`).
pub fn rewrite_upstream_sse_bytes(
    chunk: &[u8],
    remainder: &mut Vec<u8>,
    prepared: &PreparedRequest,
    accumulator: &mut StreamAccumulator,
    display_reasoning: bool,
    display_adapter: &mut Option<CursorReasoningDisplayAdapter>,
    pending_recovery_notice: &mut Option<String>,
    store: &ReasoningBackend,
    flush_remainder: bool,
) -> (Vec<u8>, bool) {
    remainder.extend_from_slice(chunk);
    let mut out = Vec::new();
    let mut finalized = false;

    while let Some(pos) = remainder.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = remainder.drain(..=pos).collect();
        if line.iter().all(|&b| b == b'\n' || b == b'\r') {
            out.extend_from_slice(&line);
            continue;
        }
        let result = rewrite_sse_chunk(
            &line,
            &prepared.original_model,
            accumulator,
            &prepared.cache_namespace,
            &prepared.record_response_contexts,
            display_reasoning,
            display_adapter,
            pending_recovery_notice.as_deref(),
            Some(store),
        );
        *pending_recovery_notice = result.pending_recovery_notice;
        if result.finalized {
            finalized = true;
        }
        out.extend_from_slice(&result.rewritten_line);
    }

    if flush_remainder && !remainder.is_empty() {
        let mut tail = std::mem::take(remainder);
        if !tail.ends_with(b"\n") {
            tail.push(b'\n');
        }
        let result = rewrite_sse_chunk(
            &tail,
            &prepared.original_model,
            accumulator,
            &prepared.cache_namespace,
            &prepared.record_response_contexts,
            display_reasoning,
            display_adapter,
            pending_recovery_notice.as_deref(),
            Some(store),
        );
        *pending_recovery_notice = result.pending_recovery_notice;
        if result.finalized {
            finalized = true;
        }
        out.extend_from_slice(&result.rewritten_line);
    }

    (out, finalized)
}

/// Persist accumulated streaming reasoning when the client disconnects or stops before `[DONE]`.
pub fn flush_streaming_reasoning(
    ctx: &mut crate::GatewayContext,
    store: &ReasoningBackend,
) -> usize {
    if ctx.stream.reasoning_finalized {
        return 0;
    }
    let (prepared, accumulator) = match (&ctx.prepared_request, &mut ctx.stream.accumulator) {
        (Some(p), Some(a)) => (p, a),
        _ => return 0,
    };
    if accumulator.messages().is_empty() {
        return 0;
    }
    prepared
        .record_response_contexts
        .iter()
        .map(|(scope, prior_messages)| {
            accumulator.store_reasoning(store, scope, &prepared.cache_namespace, prior_messages)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_silent_strip_removes_reasoning_field_and_thinking_markup() {
        let payload = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "reasoning_content": "secret",
                    "content": "<details>\n<summary>Thinking</summary>\n\nx\n</details>\n\nhi"
                }
            }]
        });
        let line = format!("data: {payload}\n\n");
        let out = apply_silent_strip_to_sse_chunk(line.as_bytes());
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("reasoning_content"));
        assert!(!text.contains("<summary>Thinking"));
        assert!(text.contains("hi"));
    }
}
