//! Codex SSE pipeline: Responses API stream → Chat Completions SSE for clients.

use bytes::Bytes;

use super::{ChunkResult, FlushResult, SsePipeline};
use crate::codex::CodexSseTranslator;
use crate::sse::parse_sse_chunk;

pub(crate) struct CodexTranslatePipeline {
    translator: CodexSseTranslator,
    sse_remainder: Vec<u8>,
}

impl CodexTranslatePipeline {
    pub fn new(client_model: &str, original_request: Option<Bytes>) -> Self {
        Self {
            translator: CodexSseTranslator::new(
                client_model,
                original_request.as_deref().map(|b| b.as_ref()),
            ),
            sse_remainder: Vec::new(),
        }
    }

    fn process_lines(&mut self, data: &[u8], client_sse_body: &mut Vec<u8>) -> ChunkResult {
        self.sse_remainder.extend_from_slice(data);
        let mut client_out = Vec::new();
        let mut usage = None;

        while let Some(pos) = self.sse_remainder.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.sse_remainder.drain(..=pos).collect();
            let line_trim = line.strip_suffix(&[b'\n']).unwrap_or(&line);
            if line_trim.is_empty() {
                continue;
            }
            for chunk in self.translator.translate_chunk(line_trim) {
                for event in parse_sse_chunk(&chunk) {
                    if let Some(u) = event.parse_usage() {
                        usage = Some(u);
                    }
                }
                client_sse_body.extend_from_slice(&chunk);
                client_out.extend_from_slice(&chunk);
            }
        }

        ChunkResult {
            client_bytes: if client_out.is_empty() {
                None
            } else {
                Some(Bytes::from(client_out))
            },
            usage,
        }
    }
}

impl SsePipeline for CodexTranslatePipeline {
    fn process_chunk(&mut self, data: Bytes, client_sse_body: &mut Vec<u8>) -> ChunkResult {
        self.process_lines(&data, client_sse_body)
    }

    fn flush_remainder(&mut self, client_sse_body: &mut Vec<u8>) -> FlushResult {
        if self.sse_remainder.is_empty() {
            return FlushResult {
                client_bytes: None,
                usage: None,
            };
        }
        let tail = std::mem::take(&mut self.sse_remainder);
        let result = self.process_lines(&tail, client_sse_body);
        FlushResult {
            client_bytes: result.client_bytes,
            usage: result.usage,
        }
    }

    fn reasoning_finalized(&self) -> bool {
        false
    }

    fn messages(&self) -> Vec<serde_json::Value> {
        Vec::new()
    }
}
