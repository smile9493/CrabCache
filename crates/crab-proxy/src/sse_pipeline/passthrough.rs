//! Passthrough SSE pipeline for MiMo, GenericRelay, and other non-reasoning pipelines.
//!
//! Forwards upstream SSE chunks directly to the client with no transformation.
//!
//! # Streaming cache limitation
//!
//! This pipeline is a **zero-buffer passthrough**: it does not accumulate SSE chunks to
//! reconstruct the final assistant message.  Consequently [`SsePipeline::messages`] always
//! returns an empty Vec, and the response-body cache write (L0/L1) in
//! `response_body.rs` is skipped for streaming MiMo/GenericRelay responses.
//!
//! Non-streaming MiMo requests still go through the normal (non-passthrough) EOS path
//! and are cached as usual.

use bytes::Bytes;

use super::{ChunkResult, FlushResult, SsePipeline, extract_usage_from_bytes};

pub(crate) struct PassthroughPipeline;

impl PassthroughPipeline {
    pub fn new() -> Self {
        Self
    }
}

impl SsePipeline for PassthroughPipeline {
    fn process_chunk(&mut self, data: Bytes, _client_sse_body: &mut Vec<u8>) -> ChunkResult {
        let usage = extract_usage_from_bytes(&data);
        ChunkResult {
            client_bytes: Some(data),
            usage,
        }
    }

    fn flush_remainder(&mut self, _client_sse_body: &mut Vec<u8>) -> FlushResult {
        FlushResult {
            client_bytes: None,
            usage: None,
        }
    }

    fn reasoning_finalized(&self) -> bool {
        false
    }

    fn messages(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }
}
