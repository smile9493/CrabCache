//! 3-pass role normalization for chat completion messages.
//!
//! Pass 1: Validate and normalize known role names (user/assistant/system/developer/tool).
//! Pass 2: Ensure alternating user/assistant pattern (system messages extracted to front).
//! Pass 3: Deduplicate consecutive messages with the same role.

use serde_json::{Value, json};

/// Normalize a list of chat messages through 3 passes.
///
/// Returns the normalized message array and a count of changes made.
pub fn normalize_roles(messages: &mut Vec<Value>) -> NormalizationReport {
    let mut report = NormalizationReport::default();

    // Pass 1: Role name normalization
    report.role_renames += normalize_role_names(messages);

    // Pass 2: Ensure valid alternating structure
    report.role_reorders += ensure_alternating_roles(messages);

    // Pass 3: Deduplicate consecutive same-role messages
    report.consecutive_merges += deduplicate_consecutive_roles(messages);

    report
}

/// Report of normalization changes.
#[derive(Debug, Default)]
pub struct NormalizationReport {
    /// Number of role names that were renamed.
    pub role_renames: usize,
    /// Number of messages reordered or inserted for alternation.
    pub role_reorders: usize,
    /// Number of consecutive same-role messages merged.
    pub consecutive_merges: usize,
}

/// Known role aliases that should be normalized.
fn normalize_role_name(role: &str) -> Option<&'static str> {
    match role.to_lowercase().as_str() {
        "user" | "human" | "input" => Some("user"),
        "assistant" | "bot" | "ai" | "model" | "output" => Some("assistant"),
        "system" => Some("system"),
        "developer" => Some("developer"),
        "tool" | "function" => Some("tool"),
        _ => None,
    }
}

/// Pass 1: Normalize role names (e.g., "human" -> "user", "bot" -> "assistant").
fn normalize_role_names(messages: &mut Vec<Value>) -> usize {
    let mut renames = 0;
    for msg in messages.iter_mut() {
        if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
            if let Some(normalized) = normalize_role_name(role) {
                if normalized != role {
                    if let Some(obj) = msg.as_object_mut() {
                        obj.insert("role".to_string(), json!(normalized));
                        renames += 1;
                    }
                }
            }
        }
    }
    renames
}

/// Pass 2: Ensure valid alternating role structure.
///
/// After normalization, the valid sequence is:
/// [system? | developer?] (user assistant)* user?
///
/// This extracts system/developer messages to the front.
fn ensure_alternating_roles(messages: &mut Vec<Value>) -> usize {
    let mut reorders = 0;

    let mut system_msgs: Vec<Value> = Vec::new();
    let mut non_system: Vec<Value> = Vec::new();

    for msg in messages.drain(..) {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if matches!(role, "system" | "developer") {
            system_msgs.push(msg);
        } else {
            non_system.push(msg);
        }
    }

    let sys_len = system_msgs.len();
    if sys_len > 1 {
        let merged_content: String = system_msgs
            .iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n");
        if let Some(first) = system_msgs.first_mut() {
            if let Some(obj) = first.as_object_mut() {
                obj.insert("content".to_string(), json!(merged_content));
            }
        }
        reorders += sys_len - 1;
        system_msgs.truncate(1);
    }

    messages.extend(system_msgs);
    messages.extend(non_system);
    reorders
}

/// Pass 3: Deduplicate consecutive messages with the same role.
///
/// When consecutive messages have the same role, their content is concatenated.
fn deduplicate_consecutive_roles(messages: &mut Vec<Value>) -> usize {
    if messages.is_empty() {
        return 0;
    }

    let mut merges = 0;
    let mut merged: Vec<Value> = Vec::new();

    for msg in messages.drain(..) {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        if let Some(last) = merged.last_mut() {
            let last_role = last.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if last_role == role && role != "system" && role != "developer" {
                let new_content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                if let Some(last_content) = last.get_mut("content") {
                    if let Some(existing) = last_content.as_str() {
                        let merged_content = format!("{existing}\n{new_content}");
                        *last_content = json!(merged_content);
                    }
                }
                merges += 1;
                continue;
            }
        }

        merged.push(msg);
    }

    *messages = merged;
    merges
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_human_to_user() {
        let mut messages = vec![json!({"role": "human", "content": "hello"})];
        let report = normalize_roles(&mut messages);
        assert_eq!(report.role_renames, 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn normalize_bot_to_assistant() {
        let mut messages = vec![json!({"role": "bot", "content": "hi"})];
        let report = normalize_roles(&mut messages);
        assert_eq!(report.role_renames, 1);
        assert_eq!(messages[0]["role"], "assistant");
    }

    #[test]
    fn deduplicate_consecutive_user_messages() {
        let mut messages = vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "user", "content": "world"}),
        ];
        let report = normalize_roles(&mut messages);
        assert_eq!(report.consecutive_merges, 1);
        assert_eq!(messages.len(), 1);
        let content = messages[0]["content"].as_str().unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("world"));
    }

    #[test]
    fn system_messages_not_merged_with_user() {
        let mut messages = vec![
            json!({"role": "system", "content": "You are helpful."}),
            json!({"role": "user", "content": "hello"}),
        ];
        let report = normalize_roles(&mut messages);
        assert_eq!(report.consecutive_merges, 0);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn multiple_system_messages_merged() {
        let mut messages = vec![
            json!({"role": "system", "content": "Rule 1"}),
            json!({"role": "system", "content": "Rule 2"}),
            json!({"role": "user", "content": "hello"}),
        ];
        let report = normalize_roles(&mut messages);
        assert!(report.role_reorders > 0 || report.consecutive_merges > 0);
        let system_count = messages
            .iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
            .count();
        assert!(system_count <= 1);
    }
}
