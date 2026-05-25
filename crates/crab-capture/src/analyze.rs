use crate::types::*;
use serde_json::Value;

/// Analyze an OpenAI-compatible JSON payload into a `PacketStructureSummary`.
///
/// This is O(n) over `messages` and O(1) over `tools`. No network I/O.
pub fn analyze_packet(payload: &Value) -> PacketStructureSummary {
    let messages = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|m| m.as_slice())
        .unwrap_or_default();

    let tools = payload.get("tools");

    let message_count = messages.len() as u32;
    let mut roles = RoleCounts::default();
    let mut tool_turn_count: u32 = 0;
    let mut assistant_with_tool_calls_count: u32 = 0;
    let mut total_content_chars: u64 = 0;
    let mut total_reasoning_content_chars: u64 = 0;
    let mut has_reasoning_content = false;
    let mut has_thinking_markup = false;
    let mut message_structures = Vec::with_capacity(messages.len());

    // System prefix: count leading consecutive system messages once.
    let system_prefix_count = messages
        .iter()
        .take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .count();
    let mut system_chars: u64 = 0;
    let mut system_in_prefix: usize = 0;

    for msg in messages {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown")
            .to_string();

        match role.as_str() {
            "system" => roles.system += 1,
            "user" => roles.user += 1,
            "assistant" => {
                roles.assistant += 1;
                if msg.get("tool_calls").is_some() {
                    assistant_with_tool_calls_count += 1;
                }
            }
            "tool" => {
                roles.tool += 1;
                tool_turn_count += 1;
            }
            _ => {}
        }

        // Content analysis.
        let (content_kind, content_chars) = analyze_content(msg);
        total_content_chars += content_chars;

        // System prefix (only leading consecutive system messages).
        if role == "system" && system_in_prefix < system_prefix_count {
            system_chars += content_chars;
            system_in_prefix += 1;
        }

        // Reasoning content (DeepSeek V4).
        let reasoning_content_chars = msg
            .get("reasoning_content")
            .and_then(|r| r.as_str())
            .map(|s| s.len() as u64)
            .unwrap_or(0);
        if reasoning_content_chars > 0 {
            has_reasoning_content = true;
        }
        total_reasoning_content_chars += reasoning_content_chars;

        // Thinking markup detection.
        let thinking = detect_thinking_markup(msg);
        if thinking {
            has_thinking_markup = true;
        }

        let tool_calls_count = msg
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|a| a.len() as u32)
            .unwrap_or(0);
        let has_tool_calls = tool_calls_count > 0;

        let name = msg
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());

        message_structures.push(MessageStructure {
            role,
            content_kind,
            content_chars,
            reasoning_content_chars,
            has_tool_calls,
            tool_calls_count,
            name,
            has_thinking_markup: thinking,
        });
    }

    let tool_count: u32 = tools
        .and_then(|t| t.as_array())
        .map(|a| a.len() as u32)
        .unwrap_or(0);
    let has_tools = tool_count > 0;

    PacketStructureSummary {
        message_count,
        system_message_count: system_in_prefix as u32,
        system_chars,
        tool_count,
        has_tools,
        roles,
        tool_turn_count,
        assistant_with_tool_calls_count,
        total_content_chars,
        total_reasoning_content_chars,
        has_reasoning_content,
        has_thinking_markup,
        messages: message_structures,
    }
}

/// Compute structural diff between client and upstream packet summaries.
pub fn diff_structure(
    client: &PacketStructureSummary,
    upstream: &PacketStructureSummary,
) -> StructureDiff {
    StructureDiff {
        client: client.clone(),
        upstream: upstream.clone(),
        delta_message_count: upstream.message_count as i32 - client.message_count as i32,
        delta_content_chars: upstream.total_content_chars as i64
            - client.total_content_chars as i64,
        delta_reasoning_chars: upstream.total_reasoning_content_chars as i64
            - client.total_reasoning_content_chars as i64,
        delta_system_chars: upstream.system_chars as i64 - client.system_chars as i64,
        delta_tool_count: upstream.tool_count as i32 - client.tool_count as i32,
        upstream_has_more_messages: upstream.message_count > client.message_count,
        reasoning_was_injected: !client.has_reasoning_content && upstream.has_reasoning_content,
    }
}

/// Analyze the content field of a message: returns (kind, char_length).
fn analyze_content(msg: &Value) -> (String, u64) {
    match msg.get("content") {
        None => ("missing".into(), 0),
        Some(Value::Null) => ("null".into(), 0),
        Some(Value::String(s)) => ("text".into(), s.len() as u64),
        Some(Value::Array(parts)) => {
            let mut total = 0u64;
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    total += text.len() as u64;
                } else if let Some(text) = part.get("content").and_then(|t| t.as_str()) {
                    total += text.len() as u64;
                }
            }
            ("array".into(), total)
        }
        _ => ("other".into(), 0),
    }
}

/// Detect thinking markup in message content or reasoning_content.
fn detect_thinking_markup(msg: &Value) -> bool {
    // Check content field.
    if let Some(content) = msg.get("content").and_then(|c| c.as_str())
        && (content.contains("<thinking")
            || content.contains("</thinking>")
            || content.contains("<details")
            || content.contains("< tl;dr>")
            || content.contains("<summary>Thinking</summary>"))
    {
        return true;
    }
    // Check reasoning_content field (DeepSeek V4).
    if let Some(rc) = msg.get("reasoning_content").and_then(|r| r.as_str())
        && !rc.is_empty()
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_user_query() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let summary = analyze_packet(&payload);
        assert_eq!(summary.message_count, 1);
        assert_eq!(summary.roles.user, 1);
        assert_eq!(summary.total_content_chars, 5);
        assert!(!summary.has_tools);
        assert!(!summary.has_reasoning_content);
    }

    #[test]
    fn test_system_and_tools() {
        let payload = json!({
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hi"}
            ],
            "tools": [
                {"type": "function", "function": {"name": "search"}}
            ]
        });
        let summary = analyze_packet(&payload);
        assert_eq!(summary.system_message_count, 1);
        assert_eq!(summary.system_chars, 16); // "You are helpful."
        assert_eq!(summary.tool_count, 1);
        assert!(summary.has_tools);
    }

    #[test]
    fn test_reasoning_content() {
        let payload = json!({
            "messages": [
                {"role": "user", "content": "Explain X"},
                {"role": "assistant", "content": "Here is X.", "reasoning_content": "Let me think about X..."}
            ]
        });
        let summary = analyze_packet(&payload);
        assert!(summary.has_reasoning_content);
        assert_eq!(summary.total_reasoning_content_chars, 23); // "Let me think about X..."
        assert_eq!(summary.messages[1].reasoning_content_chars, 23);
    }

    #[test]
    fn test_thinking_markup_detection() {
        let payload = json!({
            "messages": [
                {"role": "assistant", "content": "Hello\n<thinking>I am thinking...</thinking>\nAnswer."}
            ]
        });
        let summary = analyze_packet(&payload);
        assert!(summary.has_thinking_markup);
        assert!(summary.messages[0].has_thinking_markup);
    }

    #[test]
    fn test_array_content() {
        let payload = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Look at this image"},
                    {"type": "image_url", "image_url": {"url": "http://example.com/img.png"}}
                ]}
            ]
        });
        let summary = analyze_packet(&payload);
        assert_eq!(summary.messages[0].content_kind, "array");
        assert_eq!(summary.messages[0].content_chars, 18); // "Look at this image"
    }

    #[test]
    fn test_tool_calls() {
        let payload = json!({
            "messages": [
                {"role": "assistant", "tool_calls": [
                    {"id": "c1", "function": {"name": "search", "arguments": "{}"}},
                    {"id": "c2", "function": {"name": "read", "arguments": "{}"}}
                ]},
                {"role": "tool", "content": "result", "tool_call_id": "c1"}
            ]
        });
        let summary = analyze_packet(&payload);
        assert_eq!(summary.assistant_with_tool_calls_count, 1);
        assert_eq!(summary.messages[0].tool_calls_count, 2);
        assert_eq!(summary.tool_turn_count, 1);
    }

    #[test]
    fn test_empty_payload() {
        let payload = json!({});
        let summary = analyze_packet(&payload);
        assert_eq!(summary.message_count, 0);
        assert!(!summary.has_tools);
    }

    #[test]
    fn test_diff_structure() {
        let client = PacketStructureSummary {
            message_count: 5,
            total_content_chars: 1000,
            total_reasoning_content_chars: 0,
            has_reasoning_content: false,
            ..Default::default()
        };
        let upstream = PacketStructureSummary {
            message_count: 7,
            total_content_chars: 3000,
            total_reasoning_content_chars: 1500,
            has_reasoning_content: true,
            ..Default::default()
        };
        let diff = diff_structure(&client, &upstream);
        assert_eq!(diff.delta_message_count, 2);
        assert_eq!(diff.delta_content_chars, 2000);
        assert_eq!(diff.delta_reasoning_chars, 1500);
        assert!(diff.upstream_has_more_messages);
        assert!(diff.reasoning_was_injected);
    }

    #[test]
    fn test_reasoning_content_preserved() {
        let client = PacketStructureSummary {
            has_reasoning_content: true,
            total_reasoning_content_chars: 500,
            ..Default::default()
        };
        let upstream = PacketStructureSummary {
            has_reasoning_content: true,
            total_reasoning_content_chars: 800,
            ..Default::default()
        };
        let diff = diff_structure(&client, &upstream);
        assert!(!diff.reasoning_was_injected); // client already had it
        assert_eq!(diff.delta_reasoning_chars, 300);
    }
}
