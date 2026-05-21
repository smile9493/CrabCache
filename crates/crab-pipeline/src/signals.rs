use serde_json::Value;

pub fn cursor_agent_signals(payload: Option<&Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };

    if payload.get("tools").is_some() {
        return true;
    }
    if payload
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
    {
        return true;
    }
    if payload
        .get("prompt_cache_key")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
    {
        return true;
    }

    let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };

    messages.iter().any(message_has_cursor_signals)
}

fn message_has_cursor_signals(msg: &Value) -> bool {
    if msg.get("tool_calls").is_some() {
        return true;
    }
    if msg.get("reasoning_content").is_some() {
        return true;
    }
    false
}

pub fn user_agent_suggests_cursor(user_agent: Option<&str>) -> bool {
    user_agent
        .map(|ua| ua.to_ascii_lowercase().contains("cursor"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tools_trigger_signal() {
        let p = json!({"model": "deepseek-v4-pro", "tools": []});
        assert!(cursor_agent_signals(Some(&p)));
    }

    #[test]
    fn reasoning_in_history_triggers() {
        let p = json!({
            "messages": [{"role": "assistant", "reasoning_content": "x", "content": ""}]
        });
        assert!(cursor_agent_signals(Some(&p)));
    }

    #[test]
    fn plain_chat_no_signal() {
        let p = json!({
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"}
            ]
        });
        assert!(!cursor_agent_signals(Some(&p)));
    }
}
