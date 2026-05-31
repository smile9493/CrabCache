//! Codex / Responses API tool registry audit for MiMo relay paths.
//!
//! Detects mismatches such as instructions referencing `apply_patch` while the
//! registered tool surface is exec-only, and tracks `tool_search` drops across
//! Responses → Chat → MiMo prepare.

use crab_reasoning::CODEX_FILE_TOOL_NAMES;
use serde_json::Value;
use tracing::warn;

const NATIVE_FILE_TOOLS: &[&str] = &["apply_patch", "read_file", "list_dir"];

/// Audit of tool names exposed on a Responses API request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsesToolRegistryAudit {
    pub registered_tool_names: Vec<String>,
    pub tool_search_count: usize,
    pub instructions_mention_apply_patch: bool,
    pub has_apply_patch_tool: bool,
    pub missing_apply_patch: bool,
    pub exec_only_surface: bool,
}

/// Tools removed between chat payload and MiMo upstream payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimoToolPipelineAudit {
    pub upstream_tool_names: Vec<String>,
    pub stripped_non_function_tools: Vec<String>,
}

pub fn audit_responses_tool_registry(payload: &Value) -> ResponsesToolRegistryAudit {
    let tools = payload
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let registered_tool_names = tool_names_from_responses_tools(tools);
    let tool_search_count = count_tool_search_tools(tools);
    let instructions_mention_apply_patch = payload
        .get("instructions")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.contains("apply_patch"));
    build_registry_audit(
        registered_tool_names,
        tool_search_count,
        instructions_mention_apply_patch,
    )
}

pub fn audit_chat_tool_registry(payload: &Value) -> ResponsesToolRegistryAudit {
    let tools = payload
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let registered_tool_names = tool_names_from_chat_tools(tools);
    let tool_search_count = count_tool_search_tools(tools);
    let instructions_mention_apply_patch = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("system"))
        .filter_map(|msg| msg.get("content").and_then(|c| c.as_str()))
        .any(|s| s.contains("apply_patch"));
    build_registry_audit(
        registered_tool_names,
        tool_search_count,
        instructions_mention_apply_patch,
    )
}

pub fn audit_mimo_tool_pipeline(chat_before: &Value, chat_after: &Value) -> MimoToolPipelineAudit {
    let before_tools = chat_before
        .get("tools")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    let upstream_tool_names = tool_names_from_chat_tools(
        chat_after
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]),
    );
    let mut stripped_non_function_tools = Vec::new();
    for tool in &before_tools {
        let tool_type = tool
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");
        if tool_type == "function" {
            continue;
        }
        let label = tool_name_from_any_tool(tool)
            .map(|name| format!("{tool_type}:{name}"))
            .unwrap_or_else(|| tool_type.to_string());
        stripped_non_function_tools.push(label);
    }
    stripped_non_function_tools.sort_unstable();
    stripped_non_function_tools.dedup();
    MimoToolPipelineAudit {
        upstream_tool_names,
        stripped_non_function_tools,
    }
}

pub fn log_mimo_codex_tool_registry_warnings(
    request_id: &str,
    model: &str,
    registry: &ResponsesToolRegistryAudit,
    mimo: Option<&MimoToolPipelineAudit>,
) {
    if registry.missing_apply_patch {
        warn!(
            request_id = %request_id,
            model = %model,
            registered_tools = ?registry.registered_tool_names,
            exec_only_surface = registry.exec_only_surface,
            "Codex Responses: instructions reference apply_patch but tools[] does not register it; model may fall back to exec_command/shell"
        );
    }
    if registry.tool_search_count > 0 {
        warn!(
            request_id = %request_id,
            model = %model,
            tool_search_count = registry.tool_search_count,
            "Codex Responses: request includes tool_search tools (may be stripped before MiMo upstream)"
        );
    }
    if let Some(mimo) = mimo
        && !mimo.stripped_non_function_tools.is_empty()
    {
        warn!(
            request_id = %request_id,
            model = %model,
            stripped = ?mimo.stripped_non_function_tools,
            upstream_tools = ?mimo.upstream_tool_names,
            "MiMo prepare removed non-function tools from upstream payload"
        );
    }
}

fn build_registry_audit(
    mut registered_tool_names: Vec<String>,
    tool_search_count: usize,
    instructions_mention_apply_patch: bool,
) -> ResponsesToolRegistryAudit {
    registered_tool_names.sort_unstable();
    registered_tool_names.dedup();
    let has_apply_patch_tool = registered_tool_names.iter().any(|n| n == "apply_patch");
    let has_exec_command = registered_tool_names.iter().any(|n| n == "exec_command");
    let missing_native_file_tools = NATIVE_FILE_TOOLS
        .iter()
        .all(|name| !registered_tool_names.iter().any(|n| n == *name));
    let missing_apply_patch = instructions_mention_apply_patch && !has_apply_patch_tool;
    let exec_only_surface = has_exec_command && missing_native_file_tools;
    ResponsesToolRegistryAudit {
        registered_tool_names,
        tool_search_count,
        instructions_mention_apply_patch,
        has_apply_patch_tool,
        missing_apply_patch,
        exec_only_surface,
    }
}

fn count_tool_search_tools(tools: &[Value]) -> usize {
    tools
        .iter()
        .filter(|tool| {
            tool.get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t.starts_with("tool_search"))
        })
        .count()
}

fn tool_names_from_responses_tools(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| {
            let ty = tool.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if ty == "function" {
                tool_name_from_responses_tool(tool)
            } else if ty == "custom" {
                tool.get("name")
                    .and_then(|n| n.as_str())
                    .filter(|name| CODEX_FILE_TOOL_NAMES.contains(name))
                    .map(str::to_string)
            } else {
                None
            }
        })
        .collect()
}

fn tool_names_from_chat_tools(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter(|tool| {
            tool.get("type")
                .and_then(|t| t.as_str())
                .map_or(true, |t| t == "function")
        })
        .filter_map(tool_name_from_chat_tool)
        .collect()
}

fn tool_name_from_any_tool(tool: &Value) -> Option<String> {
    tool_name_from_responses_tool(tool).or_else(|| tool_name_from_chat_tool(tool))
}

fn tool_name_from_responses_tool(tool: &Value) -> Option<String> {
    if tool.get("type").and_then(|t| t.as_str()) == Some("function") {
        if let Some(name) = tool
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
        {
            return Some(name.to_string());
        }
        return tool
            .get("name")
            .and_then(|n| n.as_str())
            .map(str::to_string);
    }
    tool.get("name")
        .and_then(|n| n.as_str())
        .map(str::to_string)
}

fn tool_name_from_chat_tool(tool: &Value) -> Option<String> {
    tool.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
        .or_else(|| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::responses_wire::responses_payload_to_chat_completions;
    use crab_reasoning::prepare_mimo_request;
    use serde_json::json;

    fn codex_exec_only_responses_payload() -> Value {
        json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "instructions": "Use apply_patch to edit files. Do not use shell for file edits.",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "fix the bug" }],
            }],
            "tools": [
                {
                    "type": "function",
                    "name": "exec_command",
                    "description": "Run a shell command",
                    "parameters": { "type": "object", "properties": { "cmd": { "type": "string" } } }
                },
                {
                    "type": "function",
                    "name": "write_stdin",
                    "description": "Write to shell stdin",
                    "parameters": { "type": "object", "properties": { "text": { "type": "string" } } }
                },
                {
                    "type": "tool_search",
                    "name": "tool_search_local",
                }
            ]
        })
    }

    #[test]
    fn audit_detects_missing_apply_patch_when_instructions_require_it() {
        let audit = audit_responses_tool_registry(&codex_exec_only_responses_payload());
        assert!(audit.instructions_mention_apply_patch);
        assert!(!audit.has_apply_patch_tool);
        assert!(audit.missing_apply_patch);
        assert!(audit.exec_only_surface);
        assert_eq!(audit.tool_search_count, 1);
        assert_eq!(
            audit.registered_tool_names,
            vec!["exec_command".to_string(), "write_stdin".to_string()]
        );
    }

    #[test]
    fn audit_ok_when_apply_patch_present_as_custom() {
        let mut payload = codex_exec_only_responses_payload();
        payload["tools"].as_array_mut().unwrap().push(json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply a patch",
            "format": { "type": "grammar", "syntax": "lark", "definition": "start: x" }
        }));
        let audit = audit_responses_tool_registry(&payload);
        assert!(audit.has_apply_patch_tool);
        assert!(!audit.missing_apply_patch);
    }

    #[test]
    fn exec_only_pipeline_keeps_exec_command_only_upstream_tools() {
        let payload = codex_exec_only_responses_payload();
        let chat = responses_payload_to_chat_completions(&payload);
        let mimo = prepare_mimo_request(&chat, "xiaomi/mimo-v2.5-pro", false, 6);
        let names: Vec<_> = mimo.payload["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"exec_command"));
        assert!(
            !names.contains(&"apply_patch"),
            "exec-only Codex sessions must not inject apply_patch upstream"
        );
    }

    #[test]
    fn audit_ok_when_apply_patch_present() {
        let mut payload = codex_exec_only_responses_payload();
        payload["tools"].as_array_mut().unwrap().push(json!({
            "type": "function",
            "name": "apply_patch",
            "description": "Apply a patch",
            "parameters": { "type": "object" }
        }));
        let audit = audit_responses_tool_registry(&payload);
        assert!(audit.has_apply_patch_tool);
        assert!(!audit.missing_apply_patch);
        assert!(!audit.exec_only_surface);
    }

    #[test]
    fn responses_to_chat_preserves_tool_search_for_audit() {
        let payload = codex_exec_only_responses_payload();
        let chat = responses_payload_to_chat_completions(&payload);
        let tools = chat["tools"].as_array().unwrap();
        assert!(
            !tools
                .iter()
                .any(|t| { t.get("type").and_then(|ty| ty.as_str()) == Some("tool_search") }),
            "tool_search is dropped before upstream relay"
        );
        assert!(
            tools
                .iter()
                .any(|t| t["function"]["name"].as_str() == Some("exec_command")),
            "exec_command should remain for exec-only Codex sessions"
        );
        let audit = audit_chat_tool_registry(&chat);
        assert_eq!(audit.tool_search_count, 0);
        assert!(!audit.has_apply_patch_tool);
    }

    #[test]
    fn mimo_prepare_strips_non_function_tools() {
        let chat = responses_payload_to_chat_completions(&codex_exec_only_responses_payload());
        let mimo = prepare_mimo_request(&chat, "xiaomi/mimo-v2.5-pro", false, 6);
        let pipeline = audit_mimo_tool_pipeline(&chat, &mimo.payload);
        assert!(pipeline.stripped_non_function_tools.is_empty());
        let mut names = pipeline.upstream_tool_names.clone();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["exec_command".to_string(), "write_stdin".to_string(),]
        );
    }

    #[test]
    fn audit_chat_registry_reads_system_instructions() {
        let chat = responses_payload_to_chat_completions(&codex_exec_only_responses_payload());
        let audit = audit_chat_tool_registry(&chat);
        assert!(audit.instructions_mention_apply_patch);
        assert!(audit.has_apply_patch_tool);
        assert!(!audit.missing_apply_patch);
    }
}
