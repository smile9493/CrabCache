//! Codex CLI tool surface normalization for Chat Completions upstream relays.
//!
//! Codex Responses API registers file-editing tools as `type: "custom"` (e.g. `apply_patch`).
//! MiMo and other OpenAI-compatible backends only accept `type: "function"` tools, so we map
//! known Codex custom tools and optionally inject missing file tools referenced in instructions.

use serde_json::{Map, Value, json};

pub const CODEX_FILE_TOOL_NAMES: &[&str] = &["apply_patch", "read_file", "list_dir"];

/// Convert a Codex `type: "custom"` tool into Chat Completions `function` shape.
pub fn convert_codex_custom_tool_to_chat_function(tool: &Value) -> Option<Value> {
    if tool.get("type").and_then(|t| t.as_str()) != Some("custom") {
        return None;
    }
    let name = tool.get("name").and_then(|n| n.as_str())?;
    default_codex_file_tool(name, tool.get("description").and_then(|d| d.as_str()))
}

/// Default Chat Completions function tool for a Codex file-editing tool name.
pub fn default_codex_file_tool(name: &str, description: Option<&str>) -> Option<Value> {
    let parameters = match name {
        "apply_patch" => json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "Patch body in Codex apply_patch format between *** Begin Patch and *** End Patch."
                }
            },
            "required": ["input"]
        }),
        "read_file" => json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or workspace-relative file path." },
                "offset": { "type": "integer", "description": "Optional 1-based start line." },
                "limit": { "type": "integer", "description": "Optional maximum number of lines." }
            },
            "required": ["path"]
        }),
        "list_dir" => json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path to list." }
            },
            "required": ["path"]
        }),
        _ => return None,
    };
    let description = description.unwrap_or(match name {
        "apply_patch" => "Apply a patch to create, update, or delete files using Codex apply_patch syntax.",
        "read_file" => "Read a file from the workspace.",
        "list_dir" => "List entries in a workspace directory.",
        _ => return None,
    });
    Some(json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    }))
}

fn is_dropped_upstream_tool_type(tool_type: &str) -> bool {
    tool_type.starts_with("tool_search") || tool_type == "web_search" || tool_type == "namespace"
}

fn wrap_flat_function_tool(tool: &Value) -> Value {
    let mut func = Map::new();
    if let Some(name) = tool.get("name") {
        func.insert("name".into(), name.clone());
    }
    if let Some(desc) = tool.get("description") {
        func.insert("description".into(), desc.clone());
    }
    if let Some(params) = tool.get("parameters") {
        func.insert("parameters".into(), params.clone());
    }
    json!({ "type": "function", "function": Value::Object(func) })
}

/// Normalize Codex/Responses `tools[]` into upstream-safe Chat Completions tools.
pub fn normalize_codex_tools_for_upstream(tools: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for tool in tools {
        let tool_type = tool.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if is_dropped_upstream_tool_type(tool_type) {
            continue;
        }
        if tool_type == "function" {
            if tool.get("function").is_some() {
                out.push(tool.clone());
            } else {
                out.push(wrap_flat_function_tool(tool));
            }
            continue;
        }
        if let Some(converted) = convert_codex_custom_tool_to_chat_function(tool) {
            out.push(converted);
        }
    }
    out
}

fn chat_tool_name(tool: &Value) -> Option<&str> {
    tool.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .or_else(|| tool.get("name").and_then(|n| n.as_str()))
}

fn context_text_mentions_file_tool(text: &str, name: &str) -> bool {
    text.contains(name)
}

fn mentioned_codex_file_tools<'a>(text: &'a str) -> impl Iterator<Item = &'static str> + 'a {
    CODEX_FILE_TOOL_NAMES
        .iter()
        .copied()
        .filter(move |name| context_text_mentions_file_tool(text, name))
}

/// Inject missing Codex file tools when instructions/system text references them.
pub fn ensure_codex_file_tools_from_context(map: &mut Map<String, Value>) {
    let mut context = String::new();
    if let Some(instr) = map.get("instructions").and_then(|v| v.as_str()) {
        context.push_str(instr);
        context.push('\n');
    }
    if let Some(messages) = map.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if msg.get("role").and_then(|r| r.as_str()) != Some("system") {
                continue;
            }
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                context.push_str(content);
                context.push('\n');
            }
        }
    }
    if context.is_empty() {
        return;
    }

    let tools = map
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(arr) = tools.as_array_mut() else {
        return;
    };

    let existing: std::collections::HashSet<String> = arr
        .iter()
        .filter_map(chat_tool_name)
        .map(str::to_string)
        .collect();

    for name in mentioned_codex_file_tools(&context) {
        if existing.contains(name) {
            continue;
        }
        if let Some(tool) = default_codex_file_tool(name, None) {
            arr.push(tool);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_apply_patch_converts_to_function() {
        let custom = json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply patch freeform",
            "format": { "type": "grammar", "syntax": "lark", "definition": "start: x" }
        });
        let func = convert_codex_custom_tool_to_chat_function(&custom).expect("converted");
        assert_eq!(func["type"], "function");
        assert_eq!(func["function"]["name"], "apply_patch");
        assert!(func["function"]["parameters"]["properties"]["input"].is_object());
    }

    #[test]
    fn normalize_keeps_function_and_converts_custom() {
        let tools = vec![
            json!({"type":"function","name":"exec_command","parameters":{"type":"object"}}),
            json!({"type":"custom","name":"apply_patch","description":"patch"}),
            json!({"type":"namespace","name":"multi_agent_v1"}),
            json!({"type":"web_search","external_web_access": true}),
        ];
        let out = normalize_codex_tools_for_upstream(&tools);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["function"]["name"], "exec_command");
        assert_eq!(out[1]["function"]["name"], "apply_patch");
    }

    #[test]
    fn ensure_injects_apply_patch_from_system_message() {
        let mut map = Map::from_iter([
            (
                "messages".into(),
                json!([{
                    "role": "system",
                    "content": "Use apply_patch to edit files."
                }]),
            ),
            (
                "tools".into(),
                json!([{"type":"function","function":{"name":"exec_command","parameters":{"type":"object"}}}]),
            ),
        ]);
        ensure_codex_file_tools_from_context(&mut map);
        let names: Vec<_> = map["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(chat_tool_name)
            .collect();
        assert!(names.contains(&"exec_command"));
        assert!(names.contains(&"apply_patch"));
    }
}
