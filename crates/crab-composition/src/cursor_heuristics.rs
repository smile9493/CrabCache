use crate::hash::fingerprint_bytes;
use crate::types::{ComponentFingerprint, CursorComponents};
use serde_json::Value;

/// Patterns that indicate Cursor workspace rules / user rules.
const RULES_PATTERNS: &[&str] = &[
    "<user_rules>",
    "<always_applied_workspace_rules>",
    ".cursor/rules",
    "always_applied_workspace_rules",
    "AGENTS.md",
    "RULE.md",
];

/// Patterns that indicate Cursor skills.
const SKILLS_PATTERNS: &[&str] = &[
    "<available_skills>",
    "available_skills",
    "<agent_skill>",
    "agent_skill",
    "SKILL.md",
];

/// Patterns that indicate MCP server definitions.
const MCP_PATTERNS: &[&str] = &[
    "mcpServers",
    "CallMcpTool",
    "mcp_file_system",
    "<mcp_file_system_servers>",
    "mcp_file_system_servers",
];

/// Patterns that indicate sub-agent usage.
const SUBAGENT_PATTERNS: &[&str] = &[
    "subagent_type",
    "Task tool",
    "Launch.*agent",
    "best-of-n-runner",
    "ci-investigator",
];

/// Detect cursor-specific components in system message texts.
/// Scans all system messages for known patterns and returns component fingerprints.
/// Only stores presence + content hash, never raw text.
pub fn detect_cursor_components(system_contents: &[&str]) -> CursorComponents {
    let joined = system_contents.join("\n");
    CursorComponents {
        rules: detect_component(&joined, RULES_PATTERNS),
        skills: detect_component(&joined, SKILLS_PATTERNS),
        mcp: detect_component(&joined, MCP_PATTERNS),
        subagent: detect_component(&joined, SUBAGENT_PATTERNS),
    }
}

/// Detect cursor components from parsed messages array directly.
pub fn detect_cursor_components_from_messages(messages: &[Value]) -> CursorComponents {
    let system_contents: Vec<&str> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .filter_map(|m| m.get("content"))
        .filter_map(|c| c.as_str())
        .collect();
    detect_cursor_components(&system_contents)
}

fn detect_component(text: &str, patterns: &[&str]) -> ComponentFingerprint {
    let present = patterns.iter().any(|pat| text.contains(pat));
    let fingerprint = if present {
        Some(fingerprint_bytes(text.as_bytes()))
    } else {
        None
    };
    ComponentFingerprint { present, fingerprint }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_rules_found() {
        let text = "You have the following <user_rules> rules to follow. <always_applied_workspace_rules> applies too.";
        let comp = detect_component(text, RULES_PATTERNS);
        assert!(comp.present);
        assert!(comp.fingerprint.is_some());
    }

    #[test]
    fn test_detect_rules_not_found() {
        let text = "You are a helpful assistant.";
        let comp = detect_component(text, RULES_PATTERNS);
        assert!(!comp.present);
        assert!(comp.fingerprint.is_none());
    }

    #[test]
    fn test_detect_skills_found() {
        let text = "Use these <available_skills> to help with coding. SKILL.md describes the patterns.";
        let comp = detect_component(text, SKILLS_PATTERNS);
        assert!(comp.present);
    }

    #[test]
    fn test_detect_mcp_found() {
        let text = "Configure mcpServers to access the file system. Use CallMcpTool for MCP operations.";
        let comp = detect_component(text, MCP_PATTERNS);
        assert!(comp.present);
    }

    #[test]
    fn test_detect_subagent_found() {
        let text = "Launch a Task tool with subagent_type to execute the task.";
        let comp = detect_component(text, SUBAGENT_PATTERNS);
        assert!(comp.present);
    }

    #[test]
    fn test_detect_cursor_components_from_messages() {
        let messages = vec![
            json!({"role": "system", "content": "You have <user_rules>. Use SKILL.md and mcpServers."}),
            json!({"role": "user", "content": "Hello"}),
        ];
        let components = detect_cursor_components_from_messages(&messages);
        assert!(components.rules.present);
        assert!(components.skills.present);
        assert!(components.mcp.present);
        assert!(!components.subagent.present);
    }

    #[test]
    fn test_no_false_positives() {
        let messages = vec![
            json!({"role": "system", "content": "You are a helpful assistant."}),
            json!({"role": "user", "content": "Hi"}),
        ];
        let components = detect_cursor_components_from_messages(&messages);
        assert!(!components.rules.present);
        assert!(!components.skills.present);
        assert!(!components.mcp.present);
        assert!(!components.subagent.present);
    }
}
