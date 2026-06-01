use serde_json::Value;

use crate::client_kind;

#[deprecated(note = "use ClientDetector::detect() instead")]
pub fn cursor_agent_signals(payload: Option<&Value>) -> bool {
    client_kind::has_cursor_payload_signals(payload)
}

#[deprecated(note = "use ClientDetector::detect() instead")]
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
        #[allow(deprecated)]
        let result = cursor_agent_signals(Some(&p));
        assert!(result);
    }

    #[test]
    fn reasoning_in_history_triggers() {
        let p = json!({
            "messages": [{"role": "assistant", "reasoning_content": "x", "content": ""}]
        });
        #[allow(deprecated)]
        let result = cursor_agent_signals(Some(&p));
        assert!(result);
    }

    #[test]
    fn plain_chat_no_signal() {
        let p = json!({
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"}
            ]
        });
        #[allow(deprecated)]
        let result = cursor_agent_signals(Some(&p));
        assert!(!result);
    }
}
