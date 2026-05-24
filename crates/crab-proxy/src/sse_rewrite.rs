use crab_reasoning::{
    CursorReasoningDisplayAdapter, ReasoningBackend, StreamAccumulator, rewrite_sse_chunk,
};
use crab_reasoning::PreparedRequest;

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
