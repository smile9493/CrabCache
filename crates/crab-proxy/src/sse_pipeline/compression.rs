//! Token compression pipeline: deduplicate consecutive identical content deltas
//! and truncate long tool outputs to reduce token consumption.

use bytes::Bytes;
use serde_json::Value;
use std::collections::VecDeque;

use super::{ChunkResult, FlushResult, SsePipeline};
use crate::sse::parse_sse_chunk;

/// Configuration for the compression pipeline.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Suppress consecutive identical content deltas (same `delta.content`).
    pub deduplicate_content_deltas: bool,
    /// Truncate content in tool output messages longer than this (0 = disabled).
    pub truncate_long_content: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            deduplicate_content_deltas: true,
            truncate_long_content: 4096,
        }
    }
}

/// Wraps an inner SSE pipeline, applying token compression transforms.
pub struct CompressionPipeline {
    inner: Box<dyn SsePipeline>,
    config: CompressionConfig,
    recent_deltas: VecDeque<String>,
    max_recent_deltas: usize,
    chunks_processed: u64,
    chunks_suppressed: u64,
}

impl CompressionPipeline {
    pub fn new(inner: Box<dyn SsePipeline>, config: CompressionConfig) -> Self {
        Self {
            inner,
            config,
            recent_deltas: VecDeque::new(),
            max_recent_deltas: 8,
            chunks_processed: 0,
            chunks_suppressed: 0,
        }
    }

    /// Extract text content from a content delta SSE event by parsing the data field.
    fn extract_delta_content(data_str: &str) -> Option<String> {
        if data_str.trim() == "[DONE]" {
            return None;
        }
        let data: Value = serde_json::from_str(data_str).ok()?;
        // Anthropic-style: delta.text
        if let Some(delta) = data.get("delta") {
            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                return Some(text.to_string());
            }
        }
        // OpenAI-style: choices[0].delta.content
        if let Some(choices) = data.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                if let Some(delta) = choice.get("delta") {
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            return Some(content.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn is_duplicate(&self, content: &str) -> bool {
        self.recent_deltas.iter().any(|d| d == content)
    }

    fn record_delta(&mut self, content: &str) {
        if self.recent_deltas.len() >= self.max_recent_deltas {
            self.recent_deltas.pop_front();
        }
        self.recent_deltas.push_back(content.to_string());
    }
}

impl SsePipeline for CompressionPipeline {
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut Vec<u8>) -> ChunkResult {
        self.chunks_processed += 1;

        if self.config.deduplicate_content_deltas {
            // Parse the SSE events to check for content deltas.
            let events = parse_sse_chunk(&data);
            for event in &events {
                // Only check "content_block_delta" or "delta" events.
                if event.event == Some("content_block_delta") || event.event == Some("delta") {
                    if let Some(content) = Self::extract_delta_content(event.data) {
                        if self.is_duplicate(&content) {
                            self.chunks_suppressed += 1;
                            self.record_delta(&content);
                            return ChunkResult {
                                client_bytes: None,
                                usage: None,
                            };
                        }
                        self.record_delta(&content);
                    }
                }
            }
        }

        let mut result = self.inner.process_chunk(data, client_sse_body);

        if self.config.truncate_long_content > 0 {
            if let Some(ref mut bytes) = result.client_bytes {
                if bytes.len() > self.config.truncate_long_content {
                    let truncated_len = self.config.truncate_long_content;
                    let marker = "\n[... truncated by compression pipeline ...]\n";
                    let mut new_bytes = bytes[..truncated_len].to_vec();
                    new_bytes.extend_from_slice(marker.as_bytes());
                    *bytes = Bytes::from(new_bytes);
                }
            }
        }

        result
    }

    fn flush_remainder(&mut self, client_sse_body: &mut Vec<u8>) -> FlushResult {
        self.inner.flush_remainder(client_sse_body)
    }

    fn reasoning_finalized(&self) -> bool {
        self.inner.reasoning_finalized()
    }

    fn messages(&self) -> Vec<Value> {
        self.inner.messages()
    }

    fn store_partial_reasoning(&mut self, store: &crab_reasoning::ReasoningBackend) -> usize {
        self.inner.store_partial_reasoning(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    struct Passthrough;
    impl SsePipeline for Passthrough {
        fn process_chunk(&mut self, data: Bytes, _client_sse_body: &mut Vec<u8>) -> ChunkResult {
            ChunkResult {
                client_bytes: Some(data),
                usage: None,
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
        fn messages(&self) -> Vec<Value> {
            vec![]
        }
    }

    #[test]
    fn config_defaults_are_sensible() {
        let cfg = CompressionConfig::default();
        assert!(cfg.deduplicate_content_deltas);
        assert_eq!(cfg.truncate_long_content, 4096);
    }

    #[test]
    fn extract_delta_content_openai_style() {
        let data = r#"{"choices":[{"delta":{"content":"hello"}}]}"#;
        assert_eq!(
            CompressionPipeline::extract_delta_content(data),
            Some("hello".to_string())
        );
    }

    #[test]
    fn extract_delta_content_anthropic_style() {
        let data = r#"{"delta":{"text":"hello"}}"#;
        assert_eq!(
            CompressionPipeline::extract_delta_content(data),
            Some("hello".to_string())
        );
    }

    #[test]
    fn extract_delta_content_done() {
        assert!(CompressionPipeline::extract_delta_content("[DONE]").is_none());
    }
}
