//! SSE pipeline abstraction: decouples per-pipeline SSE chunk processing from `proxy.rs`.
//!
//! Each pipeline variant implements the [`SsePipeline`] trait with its own chunk-rewriting
//! and finalization logic. The proxy creates the appropriate pipeline at the start of
//! streaming and delegates to it for each upstream chunk.

mod codex;
pub(crate) mod compression;
mod passthrough;
mod reasoning;
mod silent_strip;

use bytes::Bytes;
use crab_pipeline::RequestPipeline;
use crab_reasoning::ReasoningBackend;
use serde_json::Value;
use std::sync::Arc;

use crate::context::GatewayContext;
use crate::sse::{UsageData, parse_sse_chunk};

use codex::CodexTranslatePipeline;
use compression::CompressionPipeline;
use passthrough::PassthroughPipeline;
use reasoning::ReasoningRewritePipeline;
use silent_strip::SilentStripPipeline;

/// Result from processing a single upstream SSE chunk.
pub struct ChunkResult {
    /// Bytes to send downstream to the client. `None` means suppress this chunk.
    pub client_bytes: Option<Bytes>,
    /// Usage data extracted from this chunk (if any).
    pub usage: Option<UsageData>,
}

/// Result from flushing the remainder buffer at end-of-stream.
pub struct FlushResult {
    /// Final bytes from the remainder buffer.
    pub client_bytes: Option<Bytes>,
    /// Usage extracted while draining the remainder (e.g. Codex `response.completed`).
    pub usage: Option<UsageData>,
}

/// Trait for pipeline-specific SSE processing.
pub(crate) trait SsePipeline: Send {
    /// Process one upstream SSE chunk. Returns bytes to forward to the client.
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut crate::stream_capture::StreamCapture) -> ChunkResult;

    /// Flush the internal remainder buffer at end-of-stream.
    fn flush_remainder(&mut self, client_sse_body: &mut crate::stream_capture::StreamCapture) -> FlushResult;

    /// Whether reasoning was finalized during processing.
    fn reasoning_finalized(&self) -> bool;

    /// Extract accumulated messages for cache storage.
    fn messages(&self) -> Vec<Value>;

    /// Persist partial streaming reasoning on client disconnect before `[DONE]`.
    fn store_partial_reasoning(&mut self, _store: &ReasoningBackend) -> usize {
        0
    }
}

/// Enum dispatch for SSE pipelines — avoids trait-object lifetime issues.
pub(crate) enum StreamPipeline {
    ReasoningRewrite(Box<ReasoningRewritePipeline>),
    SilentStrip(SilentStripPipeline),
    Passthrough(PassthroughPipeline),
    CodexTranslate(CodexTranslatePipeline),
    Compressed(Box<CompressionPipeline>),
}

impl SsePipeline for StreamPipeline {
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut crate::stream_capture::StreamCapture) -> ChunkResult {
        match self {
            Self::ReasoningRewrite(p) => p.process_chunk(data, client_sse_body),
            Self::SilentStrip(p) => p.process_chunk(data, client_sse_body),
            Self::Passthrough(p) => p.process_chunk(data, client_sse_body),
            Self::CodexTranslate(p) => p.process_chunk(data, client_sse_body),
            Self::Compressed(p) => p.process_chunk(data, client_sse_body),
        }
    }

    fn flush_remainder(&mut self, client_sse_body: &mut crate::stream_capture::StreamCapture) -> FlushResult {
        match self {
            Self::ReasoningRewrite(p) => p.flush_remainder(client_sse_body),
            Self::SilentStrip(p) => p.flush_remainder(client_sse_body),
            Self::Passthrough(p) => p.flush_remainder(client_sse_body),
            Self::CodexTranslate(p) => p.flush_remainder(client_sse_body),
            Self::Compressed(p) => p.flush_remainder(client_sse_body),
        }
    }

    fn reasoning_finalized(&self) -> bool {
        match self {
            Self::ReasoningRewrite(p) => p.reasoning_finalized(),
            Self::SilentStrip(p) => p.reasoning_finalized(),
            Self::Passthrough(p) => p.reasoning_finalized(),
            Self::CodexTranslate(p) => p.reasoning_finalized(),
            Self::Compressed(p) => p.reasoning_finalized(),
        }
    }

    fn messages(&self) -> Vec<Value> {
        match self {
            Self::ReasoningRewrite(p) => p.messages(),
            Self::SilentStrip(p) => p.messages(),
            Self::Passthrough(p) => p.messages(),
            Self::CodexTranslate(p) => p.messages(),
            Self::Compressed(p) => p.messages(),
        }
    }

    fn store_partial_reasoning(&mut self, store: &ReasoningBackend) -> usize {
        match self {
            Self::ReasoningRewrite(p) => p.store_partial_reasoning(store),
            Self::SilentStrip(p) => p.store_partial_reasoning(store),
            Self::Passthrough(p) => p.store_partial_reasoning(store),
            Self::CodexTranslate(p) => p.store_partial_reasoning(store),
            Self::Compressed(p) => p.store_partial_reasoning(store),
        }
    }
}

/// Create the appropriate SSE pipeline for the current request context.
///
/// - If the request has a `prepared_request` with an accumulator, use the full reasoning rewrite.
/// - If the pipeline is CursorDeepSeekV4 but reasoning display is disabled, use silent strip.
/// - Otherwise, use passthrough.
///
/// # Note on MiMo streaming cache
///
/// MiMo / GenericRelay requests fall through to [`PassthroughPipeline`], which is a
/// zero-buffer design.  Streaming responses therefore skip L0/L1 cache — the pipeline
/// cannot reconstruct the full assistant message at EOS.  Non-streaming MiMo requests
/// (where this function returns `None`) still write to cache via the normal EOS path.
pub(crate) fn select_sse_pipeline(
    ctx: &mut GatewayContext,
    store: Arc<ReasoningBackend>,
) -> Option<StreamPipeline> {
    if !ctx.is_streaming {
        return None;
    }

    if let (Some(prepared), Some(accumulator)) =
        (ctx.prepared_request.take(), ctx.stream.accumulator.take())
    {
        ctx.retired_prefix_messages = Some(prepared.retired_prefix_messages);
        Some(StreamPipeline::ReasoningRewrite(Box::new(
            ReasoningRewritePipeline::new(
                prepared,
                accumulator,
                ctx.cached_reasoning_config.display_reasoning,
                ctx.stream.display_adapter.take(),
                ctx.stream.pending_recovery_notice.take(),
                store,
            ),
        )))
    } else if ctx.request_pipeline == Some(RequestPipeline::CodexRelay)
        && ctx.client_wire_api != crate::context::ClientWireApi::Responses
    {
        Some(StreamPipeline::CodexTranslate(CodexTranslatePipeline::new(
            &ctx.model,
            ctx.original_request_body.clone(),
        )))
    } else if ctx.request_pipeline == Some(RequestPipeline::CodexRelay)
        && ctx.client_wire_api == crate::context::ClientWireApi::Responses
    {
        Some(StreamPipeline::Passthrough(PassthroughPipeline::new()))
    } else if matches!(
        ctx.request_pipeline,
        Some(RequestPipeline::CodexDeepSeek | RequestPipeline::CodexMimo)
    ) {
        // Codex bridge pipelines: passthrough SSE chunks; responses_wire translation in
        // response_body.rs converts Chat Completions SSE → Responses API SSE.
        Some(StreamPipeline::Passthrough(PassthroughPipeline::new()))
    } else if ctx.request_pipeline == Some(RequestPipeline::CursorDeepSeekV4)
        && !ctx.cached_reasoning_config.display_reasoning
    {
        Some(StreamPipeline::SilentStrip(SilentStripPipeline::new()))
    } else {
        Some(StreamPipeline::Passthrough(PassthroughPipeline::new()))
    }
}

/// Extract [`UsageData`] from an SSE chunk by parsing events and looking for a usage event.
/// Shared by all pipeline variants to avoid three copies of the same logic.
pub(crate) fn extract_usage_from_bytes(bytes: &[u8]) -> Option<UsageData> {
    let events = parse_sse_chunk(bytes);
    for event in &events {
        if let Some(usage) = event.parse_usage() {
            return Some(usage);
        }
    }
    None
}
