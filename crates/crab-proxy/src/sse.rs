use bytes::Bytes;

pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

impl SseEvent {
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }

    pub fn parse_usage(&self) -> Option<UsageData> {
        if self.data.trim() == "[DONE]" {
            return None;
        }

        let value: serde_json::Value = serde_json::from_str(&self.data).ok()?;

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

pub fn parse_sse_chunk(chunk: &[u8]) -> Vec<SseEvent> {
    let text = String::from_utf8_lossy(chunk);
    let mut events = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            events.push(SseEvent {
                event: None,
                data: data.to_string(),
            });
        } else if let Some(event) = line.strip_prefix("event: ") {
            if let Some(last) = events.last_mut() {
                last.event = Some(event.to_string());
            }
        }
    }

    events
}

pub fn reconstruct_sse_data(events: &[SseEvent]) -> Bytes {
    let mut result = Vec::new();
    for event in events {
        if let Some(ref evt) = event.event {
            result.extend_from_slice(format!("event: {}\n", evt).as_bytes());
        }
        result.extend_from_slice(format!("data: {}\n\n", event.data).as_bytes());
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
            data: "[DONE]".to_string(),
        };
        assert!(event.is_done());
    }

    #[test]
    fn test_sse_event_parse_usage() {
        let event = SseEvent {
            event: None,
            data: r#"{"usage":{"prompt_tokens":100,"completion_tokens":50,"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}}"#.to_string(),
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
                data: "{\"choices\":[]}".to_string(),
            },
            SseEvent {
                event: None,
                data: "[DONE]".to_string(),
            },
        ];
        let bytes = reconstruct_sse_data(&events);
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("data: {\"choices\":[]}"));
        assert!(text.contains("data: [DONE]"));
    }
}
