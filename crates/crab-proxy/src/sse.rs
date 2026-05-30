use bytes::Bytes;

pub struct SseEvent<'a> {
    pub event: Option<&'a str>,
    pub data: &'a str,
}

impl SseEvent<'_> {
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }

    pub fn is_rate_limit_error(&self) -> bool {
        // SSE `event: "error"` is a strong signal from LLM providers (DeepSeek, OpenAI)
        // that the upstream considers this an error worth surfacing. Even without JSON data,
        // this is safe to treat as a rate-limit hint for key cooldown purposes.
        if self.event == Some("error") {
            return true;
        }
        let data = self.data.trim();
        if data == "[DONE]" || data.len() < 10 {
            return false;
        }
        let val: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let Some(error) = val.get("error") else {
            return false;
        };
        // Check the structured `type` / `code` fields first (most reliable).
        if let Some(err_type) = error.get("type").and_then(|t| t.as_str()) {
            let t = err_type.to_lowercase();
            if t.contains("rate_limit") || t == "insufficient_quota" {
                return true;
            }
        }
        if let Some(code) = error.get("code").and_then(|c| c.as_str()) {
            let c = code.to_lowercase();
            if c.contains("rate_limit") {
                return true;
            }
        }
        // Fall back to message heuristics (narrower than before — no broad "quota"/"capacity").
        if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
            let m = msg.to_lowercase();
            return m.contains("rate limit")
                || m.contains("rate_limit")
                || m.contains("too many requests");
        }
        false
    }

    /// Extract usage data from an SSE event.
    ///
    /// Supports multiple upstream response formats:
    /// - `{"usage": {"prompt_tokens": N, "completion_tokens": N, ...}}` (OpenAI legacy)
    /// - `{"usage": {"input_tokens": N, "output_tokens": N, ...}}` (OpenAI newer / MiMo)
    /// - Mixed: both pairs present; prefer `prompt_tokens`/`completion_tokens` if non-zero.
    ///
    /// Returns `None` only when no usage data is present at all (e.g. intermediate SSE chunks).
    pub fn parse_usage(&self) -> Option<UsageData> {
        if self.data.trim() == "[DONE]" {
            return None;
        }

        let value: serde_json::Value = serde_json::from_str(self.data).ok()?;

        // Top-level usage object.
        let usage = value.get("usage")?;

        // prompt_tokens / completion_tokens (OpenAI canonical).
        let prompt = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let completion = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // input_tokens / output_tokens (OpenAI Responses API / MiMo).
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // Prefer canonical names; fall back to alternatives.
        let prompt_tokens = if prompt > 0 { prompt } else { input };
        let completion_tokens = if completion > 0 { completion } else { output };

        if prompt_tokens == 0 && completion_tokens == 0 {
            return None;
        }

        Some(UsageData {
            prompt_tokens,
            completion_tokens,
            prompt_cache_hit_tokens: usage
                .get("prompt_cache_hit_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            prompt_cache_miss_tokens: usage
                .get("prompt_cache_miss_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone)]
pub struct UsageData {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub prompt_cache_hit_tokens: u64,
    pub prompt_cache_miss_tokens: u64,
}

/// Parse SSE events from a byte chunk using zero-copy borrowing and memchr-accelerated line splitting.
///
/// Returns events that borrow from the input buffer — no heap allocations for event data.
/// Only allocates the `Vec<SseEvent>` container itself.
pub fn parse_sse_chunk(chunk: &[u8]) -> Vec<SseEvent<'_>> {
    // Fast path: validate UTF-8 once for the entire chunk.
    let text = match std::str::from_utf8(chunk) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut events = Vec::new();
    let bytes = text.as_bytes();
    let mut line_start = 0;

    while line_start < bytes.len() {
        // Use memchr for SIMD-accelerated newline search.
        let line_end = memchr::memchr(b'\n', &bytes[line_start..])
            .map(|pos| line_start + pos)
            .unwrap_or(bytes.len());

        let line = &text[line_start..line_end].trim_end_matches('\r');
        line_start = line_end + 1; // skip the '\n'

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            events.push(SseEvent { event: None, data });
        } else if let Some(event) = line.strip_prefix("event: ")
            && let Some(last) = events.last_mut()
        {
            last.event = Some(event);
        }
    }

    events
}

pub fn reconstruct_sse_data(events: &[SseEvent<'_>]) -> Bytes {
    let mut result = Vec::new();
    for event in events {
        if let Some(evt) = event.event {
            result.extend_from_slice(b"event: ");
            result.extend_from_slice(evt.as_bytes());
            result.push(b'\n');
        }
        result.extend_from_slice(b"data: ");
        result.extend_from_slice(event.data.as_bytes());
        result.extend_from_slice(b"\n\n");
    }
    Bytes::from(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_chunk() {
        let chunk = b"data: {\"choices\":[]}\n\ndata: [DONE]\n\n";
        let events = parse_sse_chunk(chunk);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].data, "[DONE]");
    }

    #[test]
    fn test_sse_event_is_done() {
        let event = SseEvent {
            event: None,
            data: "[DONE]",
        };
        assert!(event.is_done());
    }

    #[test]
    fn test_sse_event_parse_usage() {
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"prompt_tokens":100,"completion_tokens":50,"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}}"#,
        };
        let usage = event.parse_usage().unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.prompt_cache_hit_tokens, 80);
        assert_eq!(usage.prompt_cache_miss_tokens, 20);
    }

    #[test]
    fn test_reconstruct_sse_data() {
        let events = vec![
            SseEvent {
                event: None,
                data: "{\"choices\":[]}",
            },
            SseEvent {
                event: None,
                data: "[DONE]",
            },
        ];
        let bytes = reconstruct_sse_data(&events);
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("data: {\"choices\":[]}"));
        assert!(text.contains("data: [DONE]"));
    }

    #[test]
    fn test_parse_with_event_field() {
        // Note: event: must come after data: (matching upstream SSE producer convention).
        let chunk = b"data: {\"content\":\"hello\"}\nevent: message\n\n";
        let events = parse_sse_chunk(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, Some("message"));
        assert_eq!(events[0].data, "{\"content\":\"hello\"}");
    }

    #[test]
    fn test_parse_carriage_return_newlines() {
        let chunk = b"data: {\"choices\":[]}\r\n\r\ndata: [DONE]\r\n\r\n";
        let events = parse_sse_chunk(chunk);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].data, "[DONE]");
    }

    #[test]
    fn test_zero_allocation_borrowing() {
        let chunk = b"data: hello\ndata: world\n\n";
        let events = parse_sse_chunk(chunk);
        assert_eq!(events.len(), 2);
        let chunk_ptr = chunk.as_ptr();
        let data0_ptr = events[0].data.as_ptr();
        let data1_ptr = events[1].data.as_ptr();
        assert!(data0_ptr >= chunk_ptr && data0_ptr < unsafe { chunk_ptr.add(chunk.len()) });
        assert!(data1_ptr >= chunk_ptr && data1_ptr < unsafe { chunk_ptr.add(chunk.len()) });
    }

    #[test]
    fn test_is_rate_limit_error_event_type() {
        let event = SseEvent {
            event: Some("error"),
            data: "something",
        };
        assert!(event.is_rate_limit_error());
    }

    #[test]
    fn test_is_rate_limit_error_json_message() {
        let event = SseEvent {
            event: None,
            data: r#"{"error":{"message":"User API Key Rate limit exceeded","type":"rate_limit_error"}}"#,
        };
        assert!(event.is_rate_limit_error());
    }

    #[test]
    fn test_is_rate_limit_error_quota() {
        let event = SseEvent {
            event: None,
            data: r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#,
        };
        assert!(event.is_rate_limit_error());
    }

    #[test]
    fn test_is_rate_limit_error_normal_chunk() {
        let event = SseEvent {
            event: None,
            data: r#"{"choices":[{"delta":{"content":"hello"}}]}"#,
        };
        assert!(!event.is_rate_limit_error());
    }

    #[test]
    fn test_is_rate_limit_error_done() {
        let event = SseEvent {
            event: None,
            data: "[DONE]",
        };
        assert!(!event.is_rate_limit_error());
    }

    #[test]
    fn test_parse_usage_mimo_input_output_tokens() {
        // MiMo uses input_tokens/output_tokens (OpenAI Responses API style).
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"input_tokens":200,"output_tokens":80,"prompt_cache_hit_tokens":0,"prompt_cache_miss_tokens":0}}"#,
        };
        let usage = event.parse_usage().unwrap();
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 80);
    }

    #[test]
    fn test_parse_usage_prefers_canonical_over_alternative() {
        // When both are present, prefer prompt_tokens/completion_tokens.
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"prompt_tokens":100,"completion_tokens":50,"input_tokens":999,"output_tokens":999}}"#,
        };
        let usage = event.parse_usage().unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
    }

    #[test]
    fn test_parse_usage_mixed_canonical_zero_falls_back() {
        // If prompt_tokens=0 but input_tokens>0, fall back.
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"prompt_tokens":0,"completion_tokens":0,"input_tokens":150,"output_tokens":60}}"#,
        };
        let usage = event.parse_usage().unwrap();
        assert_eq!(usage.prompt_tokens, 150);
        assert_eq!(usage.completion_tokens, 60);
    }

    #[test]
    fn test_parse_usage_no_usage_returns_none() {
        let event = SseEvent {
            event: None,
            data: r#"{"choices":[{"delta":{"content":"hello"}}]}"#,
        };
        assert!(event.parse_usage().is_none());
    }

    #[test]
    fn test_parse_usage_all_zeros_returns_none() {
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"prompt_tokens":0,"completion_tokens":0,"input_tokens":0,"output_tokens":0}}"#,
        };
        assert!(event.parse_usage().is_none());
    }

    #[test]
    fn test_parse_usage_mimo_total_only() {
        // Some APIs only return total_tokens; no prompt/completion breakdown.
        // Should return None since we can't determine individual counts.
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"total_tokens":280}}"#,
        };
        assert!(event.parse_usage().is_none());
    }
}
