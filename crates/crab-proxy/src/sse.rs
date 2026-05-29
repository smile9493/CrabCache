use bytes::Bytes;

pub struct SseEvent<'a> {
    pub event: Option<&'a str>,
    pub data: &'a str,
}

impl SseEvent<'_> {
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }

    pub fn parse_usage(&self) -> Option<UsageData> {
        if self.data.trim() == "[DONE]" {
            return None;
        }

        let value: serde_json::Value = serde_json::from_str(self.data).ok()?;

        let usage = value.get("usage")?;

        Some(UsageData {
            prompt_tokens: usage.get("prompt_tokens")?.as_u64()?,
            completion_tokens: usage.get("completion_tokens")?.as_u64()?,
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
        // Verify the events borrow from the original chunk
        let chunk_ptr = chunk.as_ptr();
        let data0_ptr = events[0].data.as_ptr();
        let data1_ptr = events[1].data.as_ptr();
        assert!(data0_ptr >= chunk_ptr && data0_ptr < unsafe { chunk_ptr.add(chunk.len()) });
        assert!(data1_ptr >= chunk_ptr && data1_ptr < unsafe { chunk_ptr.add(chunk.len()) });
    }
}
