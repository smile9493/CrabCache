//! CursorDeepSeekV4 reasoning rewrite SSE pipeline.
//!
//! Uses [`rewrite_upstream_sse_bytes`] to fold reasoning content into the assistant's
//! `content` field, with optional collapsible `<details>` display.

use bytes::Bytes;
use crab_reasoning::{
    CursorReasoningDisplayAdapter, PreparedRequest, ReasoningBackend, StreamAccumulator,
};
use std::sync::Arc;

use super::{ChunkResult, FlushResult, SsePipeline};
use crate::sse::{UsageData, parse_sse_chunk};
use crate::sse_rewrite::rewrite_upstream_sse_bytes;

pub(crate) struct ReasoningRewritePipeline {
    prepared: PreparedRequest,
    accumulator: StreamAccumulator,
    display_reasoning: bool,
    display_adapter: Option<CursorReasoningDisplayAdapter>,
    pending_recovery_notice: Option<String>,
    store: Arc<ReasoningBackend>,
    sse_remainder: Vec<u8>,
    reasoning_finalized: bool,
}

impl ReasoningRewritePipeline {
    pub fn new(
        prepared: PreparedRequest,
        accumulator: StreamAccumulator,
        display_reasoning: bool,
        display_adapter: Option<CursorReasoningDisplayAdapter>,
        pending_recovery_notice: Option<String>,
        store: Arc<ReasoningBackend>,
    ) -> Self {
        Self {
            prepared,
            accumulator,
            display_reasoning,
            display_adapter,
            pending_recovery_notice,
            store,
            sse_remainder: Vec::new(),
            reasoning_finalized: false,
        }
    }
}

impl SsePipeline for ReasoningRewritePipeline {
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut Vec<u8>) -> ChunkResult {
        let (rewritten, finalized) = rewrite_upstream_sse_bytes(
            &data,
            &mut self.sse_remainder,
            &self.prepared,
            &mut self.accumulator,
            self.display_reasoning,
            &mut self.display_adapter,
            &mut self.pending_recovery_notice,
            &self.store,
            false,
        );
        if finalized {
            self.reasoning_finalized = true;
        }
        if !rewritten.is_empty() {
            client_sse_body.extend_from_slice(&rewritten);
            let usage = extract_usage_from_bytes(&rewritten);
            ChunkResult {
                client_bytes: Some(Bytes::from(rewritten)),
                usage,
            }
        } else {
            ChunkResult {
                client_bytes: None,
                usage: None,
            }
        }
    }

    fn flush_remainder(&mut self, client_sse_body: &mut Vec<u8>) -> FlushResult {
        let (rewritten, finalized) = rewrite_upstream_sse_bytes(
            b"",
            &mut self.sse_remainder,
            &self.prepared,
            &mut self.accumulator,
            self.display_reasoning,
            &mut self.display_adapter,
            &mut self.pending_recovery_notice,
            &self.store,
            true,
        );
        if finalized {
            self.reasoning_finalized = true;
        }
        if !rewritten.is_empty() {
            client_sse_body.extend_from_slice(&rewritten);
            FlushResult {
                client_bytes: Some(Bytes::from(rewritten)),
            }
        } else {
            FlushResult { client_bytes: None }
        }
    }

    fn reasoning_finalized(&self) -> bool {
        self.reasoning_finalized
    }

    fn messages(&self) -> Vec<serde_json::Value> {
        self.accumulator.messages()
    }

    fn store_partial_reasoning(&mut self, store: &ReasoningBackend) -> usize {
        if self.reasoning_finalized || self.accumulator.messages().is_empty() {
            return 0;
        }
        self.prepared
            .record_response_contexts
            .iter()
            .map(|(scope, prior_messages)| {
                self.accumulator.store_reasoning(
                    store,
                    scope,
                    &self.prepared.cache_namespace,
                    prior_messages,
                )
            })
            .sum()
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
