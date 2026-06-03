//! Silent strip SSE pipeline for CursorDeepSeekV4 when reasoning display is disabled.
//!
//! Strips `reasoning_content` fields and `<thinking>` markup from SSE chunks
//! without using the full reasoning accumulator.

use bytes::Bytes;

use super::{ChunkResult, FlushResult, SsePipeline, extract_usage_from_bytes};
use crate::sse_rewrite::apply_silent_strip_to_sse_chunk;

pub(crate) struct SilentStripPipeline {
    warned: bool,
}

impl SilentStripPipeline {
    pub fn new() -> Self {
        Self { warned: false }
    }
}

impl SsePipeline for SilentStripPipeline {
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut crate::stream_capture::StreamCapture) -> ChunkResult {
        if !self.warned {
            self.warned = true;
            tracing::warn!(
                "CursorDeepSeekV4 stream without prepared_request; applying silent reasoning strip"
            );
        }
        let client_bytes = apply_silent_strip_to_sse_chunk(&data);
        client_sse_body.extend_from_slice(&client_bytes);
        let usage = extract_usage_from_bytes(&client_bytes);
        ChunkResult {
            client_bytes: Some(Bytes::from(client_bytes)),
            usage,
        }
    }

    fn flush_remainder(&mut self, _client_sse_body: &mut crate::stream_capture::StreamCapture) -> FlushResult {
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
