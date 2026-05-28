//! Passthrough SSE pipeline for MiMo, GenericRelay, and other non-reasoning pipelines.
//!
//! Forwards upstream SSE chunks directly to the client with no transformation.

use bytes::Bytes;

use super::{ChunkResult, FlushResult, SsePipeline};
use crate::sse::{UsageData, parse_sse_chunk};

pub(crate) struct PassthroughPipeline;

impl PassthroughPipeline {
    pub fn new() -> Self {
        Self
    }
}

impl SsePipeline for PassthroughPipeline {
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut Vec<u8>) -> ChunkResult {
        let usage = extract_usage_from_bytes(&data);
        client_sse_body.extend_from_slice(&data);
        ChunkResult {
            client_bytes: Some(data),
            usage,
        }
    }

    fn flush_remainder(&mut self, _client_sse_body: &mut Vec<u8>) -> FlushResult {
        FlushResult { client_bytes: None }
    }

    fn reasoning_finalized(&self) -> bool {
        false
    }

    fn messages(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }
}

fn extract_usage_from_bytes(bytes: &[u8]) -> Option<UsageData> {
    let events = parse_sse_chunk(bytes);
    for event in &events {
        if let Some(usage) = event.parse_usage() {
            return Some(usage);
        }
    }
    None
}
