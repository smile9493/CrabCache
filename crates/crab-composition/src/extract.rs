use crate::cursor_heuristics::detect_cursor_components_from_messages;
use crate::hash::{immutable_prefix_block_hash, tool_names_hash};
use crate::types::{CursorComponents, RequestComposition, RoleCounts};
use serde_json::Value;

/// Hints passed from the gateway context that supplement the payload-derived
/// composition.
#[derive(Debug, Clone, Default)]
pub struct CompositionHints {
    pub consumer: String,
    pub domain: String,
    pub project_id: Option<String>,
    pub pipeline: String,
    pub user_agent: Option<String>,
    pub upstream_model: Option<String>,
}

/// Extract `RequestComposition` from an OpenAI-compatible JSON payload and
/// gateway context hints.
///
/// This function is O(n) over `messages` and O(1) over `tools`. It performs
/// no network I/O and allocates only for the fingerprint structures and counts.
pub fn extract_composition(
    payload: &Value,
    hints: &CompositionHints,
) -> RequestComposition {
    let messages = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|m| m.as_slice())
        .unwrap_or_default();

    let tools = payload.get("tools");

    // --- Role counts and message stats ---
    let message_count = messages.len() as u32;
    let mut roles = RoleCounts::default();
    let mut tool_turn_count: u32 = 0;
    let mut assistant_with_tool_calls_count: u32 = 0;

    for msg in messages {
        match msg.get("role").and_then(|r| r.as_str()) {
            Some("system") => roles.system += 1,
            Some("user") => roles.user += 1,
            Some("assistant") => {
                roles.assistant += 1;
                if msg.get("tool_calls").is_some() {
                    assistant_with_tool_calls_count += 1;
                }
            }
            Some("tool") => {
                roles.tool += 1;
                tool_turn_count += 1;
            }
            _ => {}
        }
    }

    // --- System prefix stats ---
    let system_messages: Vec<&Value> = messages
        .iter()
        .take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .collect();
    let system_message_count = system_messages.len() as u32;
    let system_chars: u32 = system_messages
        .iter()
        .filter_map(|m| m.get("content"))
        .filter_map(|c| c.as_str())
        .map(|s| s.len() as u32)
        .sum();

    let system_prefix_hash = if system_message_count > 0 || tools.is_some() {
        Some(immutable_prefix_block_hash(messages, tools))
    } else {
        None
    };

    // --- Tool stats ---
    let tool_count: u32 = tools
        .and_then(|t| t.as_array())
        .map(|a| a.len() as u32)
        .unwrap_or(0);
    let has_tools = tool_count > 0;
    let tool_names_hash = if has_tools {
        if let Some(arr) = tools.and_then(|t| t.as_array()) {
            Some(tool_names_hash(arr))
        } else {
            None
        }
    } else {
        None
    };

    // --- Cursor components ---
    let components: CursorComponents = if roles.system > 0 {
        detect_cursor_components_from_messages(messages)
    } else {
        CursorComponents::default()
    };

    // --- User agent truncation ---
    let user_agent = hints
        .user_agent
        .as_ref()
        .map(|ua| if ua.len() > 128 { &ua[..128] } else { ua.as_str() })
        .map(|s| s.to_string());

    RequestComposition {
        consumer: hints.consumer.clone(),
        domain: hints.domain.clone(),
        project_id: hints.project_id.clone(),
        pipeline: hints.pipeline.clone(),
        user_agent,
        client_model: payload
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string(),
        upstream_model: hints.upstream_model.clone(),
        system_prefix_hash,
        system_message_count,
        system_chars,
        tool_count,
        tool_names_hash,
        has_tools,
        message_count,
        roles,
        tool_turn_count,
        assistant_with_tool_calls_count,
        components,
    }
}

/// Extract concatenated system message text from the request payload,
/// truncated to `max_chars`. Returns the raw text content (unhashed).
pub fn extract_system_text(payload: &Value, max_chars: usize) -> Option<String> {
    let messages = payload.get("messages")?.as_array()?;
    let texts: Vec<&str> = messages
        .iter()
        .take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .filter_map(|m| m.get("content"))
        .filter_map(|c| c.as_str())
        .collect();
    if texts.is_empty() {
        return None;
    }
    let joined = texts.join("\n");
    if joined.len() > max_chars {
        let end = joined.floor_char_boundary(max_chars);
        let mut truncated = joined[..end].to_string();
        truncated.push_str("…<truncated>");
        Some(truncated)
    } else {
        Some(joined)
    }
}

/// Extract tools definition JSON string from the request payload,
/// truncated to `max_chars`. Returns the raw JSON text (unhashed).
pub fn extract_tools_json(payload: &Value, max_chars: usize) -> Option<String> {
    let tools = payload.get("tools")?;
    let json_str = serde_json::to_string(tools).ok()?;
    if json_str == "null" {
        return None;
    }
    if json_str.len() > max_chars {
        let end = json_str.floor_char_boundary(max_chars);
        let mut truncated = json_str[..end].to_string();
        truncated.push_str("…<truncated>");
        Some(truncated)
    } else {
        Some(json_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_simple_user_query() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let hints = CompositionHints {
            consumer: "test-consumer".into(),
            domain: "test-domain".into(),
            ..Default::default()
        };
        let comp = extract_composition(&payload, &hints);
        assert_eq!(comp.consumer, "test-consumer");
        assert_eq!(comp.domain, "test-domain");
        assert_eq!(comp.client_model, "deepseek-v4-pro");
        assert_eq!(comp.message_count, 1);
        assert_eq!(comp.roles.user, 1);
        assert!(!comp.has_tools);
        assert!(!comp.components.rules.present);
        assert!(comp.system_prefix_hash.is_none());
    }

    #[test]
    fn test_extract_with_system_and_tools() {
        let payload = json!({
            "model": "deepseek-v4-max",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is Rust?"}
            ],
            "tools": [
                {"type": "function", "function": {"name": "search"}},
                {"type": "function", "function": {"name": "compute"}}
            ]
        });
        let hints = CompositionHints {
            consumer: "cursor-user".into(),
            domain: "default".into(),
            pipeline: "cursor_deepseek_v4".into(),
            ..Default::default()
        };
        let comp = extract_composition(&payload, &hints);
        assert_eq!(comp.message_count, 2);
        assert_eq!(comp.roles.system, 1);
        assert_eq!(comp.roles.user, 1);
        assert_eq!(comp.tool_count, 2);
        assert!(comp.has_tools);
        assert!(comp.system_prefix_hash.is_some());
        assert!(comp.tool_names_hash.is_some());
        assert_eq!(comp.pipeline, "cursor_deepseek_v4");
    }

    #[test]
    fn test_extract_with_cursor_components() {
        let payload = json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "system", "content": "Use <user_rules> and mcpServers for tool access."},
                {"role": "user", "content": "Do the thing"}
            ]
        });
        let hints = CompositionHints::default();
        let comp = extract_composition(&payload, &hints);
        assert!(comp.components.rules.present);
        assert!(comp.components.mcp.present);
        assert!(!comp.components.skills.present);
        assert!(!comp.components.subagent.present);
    }

    #[test]
    fn test_extract_conversation_roles() {
        let payload = json!({
            "model": "deepseek-chat",
            "messages": [
                {"role": "system", "content": "Be helpful."},
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"},
                {"role": "user", "content": "Search for X"},
                {"role": "assistant", "tool_calls": [{"id": "call1", "function": {"name": "search", "arguments": "{}"}}]},
                {"role": "tool", "content": "Result", "tool_call_id": "call1"},
                {"role": "assistant", "content": "Here is the result."}
            ]
        });
        let hints = CompositionHints::default();
        let comp = extract_composition(&payload, &hints);
        assert_eq!(comp.message_count, 7);
        assert_eq!(comp.roles.system, 1);
        assert_eq!(comp.roles.user, 2);
        assert_eq!(comp.roles.assistant, 3);
        assert_eq!(comp.roles.tool, 1);
        assert_eq!(comp.tool_turn_count, 1);
        assert_eq!(comp.assistant_with_tool_calls_count, 1);
    }

    #[test]
    fn test_extract_empty_payload() {
        let payload = json!({});
        let hints = CompositionHints::default();
        let comp = extract_composition(&payload, &hints);
        assert_eq!(comp.message_count, 0);
        assert_eq!(comp.client_model, "unknown");
        assert!(!comp.has_tools);
    }
}
