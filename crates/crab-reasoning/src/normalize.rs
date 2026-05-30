use crate::backend::ReasoningBackend;
use crate::keys::{
    message_signature, resolve_reasoning_scope, tool_call_ids, tool_call_names, tool_call_signature,
};
use crab_metrics::global_metrics;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tracing::debug;

static CURSOR_THINKING_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:<(?:think|thinking)\b[^>]*>[\s\S]*?(?:</(?:think|thinking)>|$)|<details\b[^>]*>\s*<summary\b[^>]*>\s*Thinking\s*</summary>[\s\S]*?(?:</details>|$))\s*").unwrap()
});

const SUPPORTED_REQUEST_FIELDS: &[&str] = &[
    "model",
    "messages",
    "stream",
    "stream_options",
    "max_tokens",
    "response_format",
    "stop",
    "tools",
    "tool_choice",
    "thinking",
    "reasoning_effort",
    "temperature",
    "top_p",
    "presence_penalty",
    "frequency_penalty",
    "logprobs",
    "top_logprobs",
    "user",
    "seed",
    "n",
    "logit_bias",
    "user_id",
];

const MESSAGE_FIELDS: &[&str] = &[
    "role",
    "content",
    "name",
    "tool_call_id",
    "tool_calls",
    "reasoning_content",
    "prefix",
];

const ROLE_MESSAGE_FIELDS: &[(&str, &[&str])] = &[
    ("system", &["role", "content", "name"]),
    ("user", &["role", "content", "name"]),
    (
        "assistant",
        &[
            "role",
            "content",
            "name",
            "tool_calls",
            "reasoning_content",
            "prefix",
        ],
    ),
    ("tool", &["role", "content", "tool_call_id"]),
];

const EFFORT_ALIASES: &[(&str, &str)] = &[
    ("low", "high"),
    ("medium", "high"),
    ("high", "high"),
    ("max", "max"),
    ("xhigh", "max"),
];

pub const RECOVERY_NOTICE_TEXT: &str = "[crabcache] Refreshed reasoning_content history.";
pub const RECOVERY_NOTICE_CONTENT: &str = "[crabcache] Refreshed reasoning_content history.\n\n";
/// Legacy prefix from deepseek-cursor-proxy; still recognized in Cursor-echoed history.
pub const LEGACY_RECOVERY_NOTICE_TEXT: &str =
    "[deepseek-cursor-proxy] Refreshed reasoning_content history.";
pub const RECOVERY_SYSTEM_CONTENT: &str = "CrabCache recovered this request because older DeepSeek thinking-mode tool-call reasoning_content was unavailable. Older unrecoverable tool-call history was omitted; continue using only the remaining recovered context.";

fn content_starts_with_recovery_notice(content: &str) -> bool {
    content.starts_with(RECOVERY_NOTICE_TEXT) || content.starts_with(LEGACY_RECOVERY_NOTICE_TEXT)
}

fn recovery_notice_strip_prefix_len(content: &str) -> Option<usize> {
    if content.starts_with(RECOVERY_NOTICE_TEXT) {
        Some(RECOVERY_NOTICE_TEXT.len())
    } else if content.starts_with(LEGACY_RECOVERY_NOTICE_TEXT) {
        Some(LEGACY_RECOVERY_NOTICE_TEXT.len())
    } else {
        None
    }
}

fn get_role_fields(role: &str) -> &'static [&'static str] {
    ROLE_MESSAGE_FIELDS
        .iter()
        .find(|(r, _)| *r == role)
        .map(|(_, fields)| *fields)
        .unwrap_or(MESSAGE_FIELDS)
}

pub fn normalize_reasoning_effort(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    for (alias, target) in EFFORT_ALIASES {
        if *alias == lower {
            return target.to_string();
        }
    }
    "high".to_string()
}

pub fn extract_text_content(content: &Value) -> Option<String> {
    match content {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for item in arr {
                match item {
                    Value::String(s) => parts.push(s.clone()),
                    Value::Object(obj) => {
                        let item_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        let text = obj
                            .get("text")
                            .or_else(|| obj.get("content"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if item_type == "text" || item_type == "input_text" || !text.is_empty() {
                            parts.push(text.to_string());
                        }
                    }
                    other => parts.push(other.to_string()),
                }
            }
            let joined: String = parts
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        Value::Object(_) | Value::Number(_) | Value::Bool(_) => {
            Some(serde_json::to_string(content).unwrap_or_default())
        }
    }
}

pub fn strip_cursor_thinking_blocks(content: &str) -> String {
    let result = CURSOR_THINKING_BLOCK_RE
        .replace_all(content, "")
        .to_string();
    result.trim_start_matches(['\r', '\n']).to_string()
}

/// MiMo rejects tool `arguments` with trailing JSON (`unexpected content after document`).
pub fn repair_tool_arguments_json(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "{}".to_string();
    }
    let mut de = serde_json::Deserializer::from_str(trimmed);
    match Value::deserialize(&mut de) {
        Ok(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.retain(|_, val| {
                    !(val.as_str().is_some_and(|s| s.is_empty())
                        || val.as_array().is_some_and(|a| a.is_empty()))
                });
            }
            serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string())
        }
        Err(_) => "{}".to_string(),
    }
}

fn normalize_tool_call(tool_call: &Value) -> Value {
    let tc = tool_call.as_object().cloned().unwrap_or_default();
    let function = tc.get("function").and_then(|f| f.as_object()).cloned();
    let func_obj = if let Some(func) = function {
        let arguments = func
            .get("arguments")
            .map(|a| {
                let raw = if a.is_string() {
                    a.as_str().unwrap_or("").to_string()
                } else {
                    serde_json::to_string(a).unwrap_or_default()
                };
                repair_tool_arguments_json(&raw)
            })
            .unwrap_or_else(|| "{}".to_string());
        let mut m = serde_json::Map::new();
        m.insert(
            "name".into(),
            Value::String(
                func.get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
        );
        m.insert("arguments".into(), Value::String(arguments));
        m
    } else {
        let mut m = serde_json::Map::new();
        m.insert("name".into(), Value::String(String::new()));
        m.insert("arguments".into(), Value::String(String::new()));
        m
    };

    let mut normalized = serde_json::Map::new();
    let id = tc
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !id.is_empty() {
        normalized.insert("id".into(), Value::String(id));
    }
    normalized.insert(
        "type".into(),
        tc.get("type")
            .cloned()
            .unwrap_or(Value::String("function".into())),
    );
    normalized.insert("function".into(), Value::Object(func_obj));
    Value::Object(normalized)
}

/// Normalize assistant `tool_calls[].function.arguments` for MiMo upstream.
pub fn sanitize_mimo_tool_calls_in_messages(messages: Vec<Value>) -> (Vec<Value>, usize) {
    let mut repaired = 0usize;
    let out = messages
        .into_iter()
        .map(|mut msg| {
            let Some(tcs) = msg.get("tool_calls").and_then(|tc| tc.as_array()).cloned() else {
                return msg;
            };
            let normalized: Vec<Value> = tcs
                .iter()
                .map(|tc| {
                    let before = tc
                        .pointer("/function/arguments")
                        .and_then(|a| a.as_str())
                        .unwrap_or("");
                    let norm = normalize_tool_call(tc);
                    let after = norm
                        .pointer("/function/arguments")
                        .and_then(|a| a.as_str())
                        .unwrap_or("");
                    if before != after {
                        repaired += 1;
                    }
                    norm
                })
                .collect();
            if let Some(obj) = msg.as_object_mut() {
                obj.insert("tool_calls".into(), Value::Array(normalized));
            }
            msg
        })
        .collect();
    (out, repaired)
}

fn normalize_tool(tool: &Value) -> Value {
    let mut normalized = tool.as_object().cloned().unwrap_or_default();
    normalized.insert(
        "type".into(),
        normalized
            .get("type")
            .cloned()
            .unwrap_or(Value::String("function".into())),
    );
    Value::Object(normalized)
}

fn legacy_function_to_tool(function: &Value) -> Value {
    let func = function.as_object().cloned().unwrap_or_default();
    let mut m = serde_json::Map::new();
    m.insert("type".into(), Value::String("function".into()));
    m.insert("function".into(), Value::Object(func));
    Value::Object(m)
}

/// DeepSeek does not support selecting a specific function; downgrade to `auto` (cursor-deepseek parity).
pub fn normalize_tool_choice_for_deepseek(tool_choice: &Value) -> Option<Value> {
    match tool_choice {
        Value::Object(obj) if obj.get("type").and_then(|t| t.as_str()) == Some("function") => {
            Some(Value::String("auto".into()))
        }
        other => normalize_tool_choice(other),
    }
}

fn normalize_tool_choice(tool_choice: &Value) -> Option<Value> {
    match tool_choice {
        Value::String(s) => {
            if ["auto", "none", "required"].contains(&s.as_str()) {
                Some(tool_choice.clone())
            } else {
                None
            }
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("function")
                && let Some(func) = obj.get("function").and_then(|f| f.as_object())
                && func.get("name").is_some()
            {
                let mut m = serde_json::Map::new();
                m.insert("type".into(), Value::String("function".into()));
                let mut fm = serde_json::Map::new();
                fm.insert(
                    "name".into(),
                    func.get("name").cloned().unwrap_or(Value::Null),
                );
                m.insert("function".into(), Value::Object(fm));
                return Some(Value::Object(m));
            }
            Some(tool_choice.clone())
        }
        _ => None,
    }
}

fn convert_function_call(function_call: &Value) -> Option<Value> {
    match function_call {
        Value::String(s) => {
            if ["auto", "none", "required"].contains(&s.as_str()) {
                Some(function_call.clone())
            } else {
                None
            }
        }
        Value::Object(obj) => {
            if obj.get("name").is_some() {
                let mut m = serde_json::Map::new();
                m.insert("type".into(), Value::String("function".into()));
                let mut fm = serde_json::Map::new();
                fm.insert(
                    "name".into(),
                    obj.get("name").cloned().unwrap_or(Value::Null),
                );
                m.insert("function".into(), Value::Object(fm));
                Some(Value::Object(m))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn assistant_needs_reasoning_for_tool_context(message: &Value, prior_messages: &[Value]) -> bool {
    if message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    for prior in prior_messages.iter().rev() {
        let role = prior.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "tool" {
            return true;
        }
        if role == "user" || role == "system" {
            return false;
        }
    }
    false
}

/// Placeholder when ReasoningStore has no entry yet; avoids latest_user truncation loops.
const REASONING_PLACEHOLDER: &str = ".";

fn reasoning_lookup_keys(
    message: &Value,
    scope: &str,
    cache_namespace: &str,
    prior_messages: &[Value],
    prefer_portable_first: bool,
) -> Vec<serde_json::Value> {
    let mut keys = Vec::new();

    keys.push(serde_json::json!({
        "kind": "message_signature",
        "key": format!("scope:{}:signature:{}", scope, message_signature(message)),
        "portable": false,
    }));

    for tc_id in tool_call_ids(message) {
        keys.push(serde_json::json!({
            "kind": "tool_call_id",
            "tool_call_id": tc_id,
            "key": format!("scope:{}:tool_call:{}", scope, tc_id),
            "portable": false,
        }));
    }

    for tc in message
        .get("tool_calls")
        .and_then(|tcs| tcs.as_array())
        .unwrap_or(&Vec::new())
    {
        if tc.is_object() {
            let func_name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            keys.push(serde_json::json!({
                "kind": "tool_call_signature",
                "function_name": func_name,
                "key": format!("scope:{}:tool_call_signature:{}", scope, tool_call_signature(tc)),
                "portable": false,
            }));
        }
    }

    for name in tool_call_names(message) {
        keys.push(serde_json::json!({
            "kind": "tool_name",
            "function_name": name,
            "key": format!("scope:{}:tool_name:{}", scope, name),
            "portable": false,
        }));
    }

    if !cache_namespace.is_empty() && !prior_messages.is_empty() {
        use crate::keys::turn_context_signature;
        let turn_sig = turn_context_signature(prior_messages);
        keys.push(serde_json::json!({
            "kind": "portable_message_signature",
            "key": format!("namespace:{}:turn:{}:signature:{}", cache_namespace, turn_sig, message_signature(message)),
            "turn_context_signature": turn_sig,
            "portable": true,
        }));
        for tc_id in tool_call_ids(message) {
            keys.push(serde_json::json!({
                "kind": "portable_tool_call_id",
                "tool_call_id": tc_id,
                "key": format!("namespace:{}:turn:{}:tool_call:{}", cache_namespace, turn_sig, tc_id),
                "portable": true,
            }));
        }
        for tc in message
            .get("tool_calls")
            .and_then(|tcs| tcs.as_array())
            .unwrap_or(&Vec::new())
        {
            if tc.is_object() {
                let func_name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                keys.push(serde_json::json!({
                    "kind": "portable_tool_call_signature",
                    "function_name": func_name,
                    "key": format!("namespace:{}:turn:{}:tool_call_signature:{}", cache_namespace, turn_sig, tool_call_signature(tc)),
                    "portable": true,
                }));
            }
        }
        for name in tool_call_names(message) {
            keys.push(serde_json::json!({
                "kind": "portable_tool_name",
                "function_name": name,
                "key": format!("namespace:{}:turn:{}:tool_name:{}", cache_namespace, turn_sig, name),
                "portable": true,
            }));
        }
    }

    if prefer_portable_first {
        let (portable, other): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .partition(|k| k.get("portable").and_then(|p| p.as_bool()).unwrap_or(false));
        portable.into_iter().chain(other).collect()
    } else {
        keys
    }
}

fn try_restore_reasoning_from_store(
    msg: &serde_json::Map<String, Value>,
    store: &ReasoningBackend,
    prior_messages: &[Value],
    stable_session_id: Option<&str>,
    cache_namespace: &str,
) -> Option<String> {
    let lookup_scope = resolve_reasoning_scope(stable_session_id, prior_messages, cache_namespace);
    let prefer_portable = stable_session_id.is_some_and(|s| !s.trim().is_empty());
    let lookup_keys = reasoning_lookup_keys(
        &Value::Object(msg.clone()),
        &lookup_scope,
        cache_namespace,
        prior_messages,
        prefer_portable,
    );
    for lookup_key in &lookup_keys {
        if let Some(key_str) = lookup_key.get("key").and_then(|k| k.as_str())
            && let Some(restored) = store.get(key_str)
        {
            return Some(restored);
        }
    }
    None
}

/// Patch missing `reasoning_content` in place without dropping tool/assistant history.
fn patch_missing_reasoning_inplace(
    messages: &mut [Value],
    missing_indexes: &[usize],
    store: Option<&ReasoningBackend>,
    cache_namespace: &str,
    stable_session_id: Option<&str>,
) -> usize {
    let mut patched = 0;
    for &idx in missing_indexes {
        let prior: Vec<Value> = messages.get(..idx).unwrap_or(&[]).to_vec();
        if let Some(store) = store
            && let Some(msg_obj) = messages.get(idx).and_then(|m| m.as_object())
            && let Some(restored) = try_restore_reasoning_from_store(
                msg_obj,
                store,
                &prior,
                stable_session_id,
                cache_namespace,
            )
        {
            if let Some(obj) = messages.get_mut(idx).and_then(|m| m.as_object_mut()) {
                obj.insert("reasoning_content".into(), Value::String(restored));
                patched += 1;
            }
            continue;
        }
        let Some(obj) = messages.get_mut(idx).and_then(|m| m.as_object_mut()) else {
            continue;
        };
        let has_tool_calls = obj
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .is_some_and(|a| !a.is_empty());
        let content = obj.get("content").and_then(|c| c.as_str()).unwrap_or("");
        // Tool-call assistants: minimal placeholder (DeepSeek requires the field).
        // Text-only assistants without Store: use content so upstream sees progress.
        let placeholder = if has_tool_calls || content.is_empty() {
            REASONING_PLACEHOLDER.to_string()
        } else {
            content.to_string()
        };
        obj.insert("reasoning_content".into(), Value::String(placeholder));
        patched += 1;
    }
    patched
}

struct NormalizeResult {
    message: Value,
    patched: bool,
    missing: bool,
}

fn normalize_message(
    message: &Value,
    store: Option<&ReasoningBackend>,
    prior_messages: &[Value],
    cache_namespace: &str,
    stable_session_id: Option<&str>,
    repair_reasoning: bool,
    keep_reasoning: bool,
) -> NormalizeResult {
    let mut msg = message.as_object().cloned().unwrap_or_default();
    msg.retain(|k, _| MESSAGE_FIELDS.contains(&k.as_str()));

    let role = msg
        .get("role")
        .and_then(|r| r.as_str())
        .unwrap_or("user")
        .to_string();
    msg.insert("role".into(), Value::String(role.clone()));

    let role_str = match role.as_str() {
        "function" => "tool",
        "developer" => "system",
        other => other,
    };
    if role_str != role.as_str() {
        msg.insert("role".into(), Value::String(role_str.into()));
    }

    if msg.contains_key("content") {
        let content = msg.get("content").cloned().unwrap_or(Value::Null);
        msg.insert(
            "content".into(),
            extract_text_content(&content)
                .map(Value::String)
                .unwrap_or(Value::String(String::new())),
        );
    } else if ["assistant", "tool", "system", "user"].contains(&role_str) {
        msg.insert("content".into(), Value::String(String::new()));
    }

    if role_str == "assistant"
        && let Some(content) = msg.get("content").and_then(|c| c.as_str())
    {
        let stripped = strip_cursor_thinking_blocks(content);
        msg.insert("content".into(), Value::String(stripped));
    }

    if let Some(tool_calls) = msg.get("tool_calls").and_then(|tc| tc.as_array()).cloned() {
        msg.insert(
            "tool_calls".into(),
            Value::Array(tool_calls.iter().map(normalize_tool_call).collect()),
        );
    }

    let mut patched = false;
    let mut missing = false;

    if role_str == "assistant" {
        if !keep_reasoning {
            msg.remove("reasoning_content");
        } else if repair_reasoning {
            let has_reasoning = msg
                .get("reasoning_content")
                .and_then(|r| r.as_str())
                .is_some();
            if !has_reasoning {
                msg.remove("reasoning_content");
                let needs_reasoning = assistant_needs_reasoning_for_tool_context(
                    &Value::Object(msg.clone()),
                    prior_messages,
                );
                if needs_reasoning {
                    let lookup_scope =
                        resolve_reasoning_scope(stable_session_id, prior_messages, cache_namespace);
                    let prefer_portable = stable_session_id.is_some_and(|s| !s.trim().is_empty());
                    let lookup_keys = reasoning_lookup_keys(
                        &Value::Object(msg.clone()),
                        &lookup_scope,
                        cache_namespace,
                        prior_messages,
                        prefer_portable,
                    );
                    if let Some(store) = store {
                        for lookup_key in &lookup_keys {
                            if let Some(key_str) = lookup_key.get("key").and_then(|k| k.as_str())
                                && let Some(restored) = store.get(key_str)
                            {
                                msg.insert("reasoning_content".into(), Value::String(restored));
                                patched = true;
                                break;
                            }
                        }
                    }
                    if !patched {
                        missing = true;
                    }
                }
            }
        }
    }

    let allowed = get_role_fields(role_str);
    msg.retain(|k, _| allowed.contains(&k.as_str()));

    NormalizeResult {
        message: Value::Object(msg),
        patched,
        missing,
    }
}

pub struct NormalizeMessagesResult {
    pub messages: Vec<Value>,
    pub patched_count: usize,
    pub missing_indexes: Vec<usize>,
}

pub fn normalize_messages(
    messages: &[Value],
    store: Option<&ReasoningBackend>,
    cache_namespace: &str,
    stable_session_id: Option<&str>,
    repair_reasoning: bool,
    keep_reasoning: bool,
) -> NormalizeMessagesResult {
    let mut normalized = Vec::with_capacity(messages.len());
    let mut patched_count = 0;
    let mut missing_indexes = Vec::new();

    for message in messages {
        let result = normalize_message(
            message,
            store,
            &normalized,
            cache_namespace,
            stable_session_id,
            repair_reasoning,
            keep_reasoning,
        );
        if result.patched {
            patched_count += 1;
        }
        if result.missing {
            missing_indexes.push(normalized.len());
        }
        normalized.push(result.message);
    }

    NormalizeMessagesResult {
        messages: normalized,
        patched_count,
        missing_indexes,
    }
}

fn has_recovery_notice(message: &Value) -> bool {
    message.get("role").and_then(|r| r.as_str()) == Some("assistant")
        && message
            .get("content")
            .and_then(|c| c.as_str())
            .map(content_starts_with_recovery_notice)
            .unwrap_or(false)
}

fn history_has_recovery_notice(messages: &[Value]) -> bool {
    messages.iter().any(has_recovery_notice)
}

/// Only the first recover in a thread should surface the user-visible notice (Cursor sub-agents
/// retry the same body without x-conversation-id and would otherwise stack duplicate notices).
fn should_attach_recovery_notice(messages: &[Value]) -> bool {
    !history_has_recovery_notice(messages)
}

fn strip_recovery_notice_for_upstream(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| {
            if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
                return msg.clone();
            }
            let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let Some(prefix_len) = recovery_notice_strip_prefix_len(content) else {
                return msg.clone();
            };
            let mut cleaned = msg.clone();
            if let Some(obj) = cleaned.as_object_mut() {
                let remaining = content[prefix_len..].trim_start_matches(['\r', '\n']);
                obj.insert("content".into(), Value::String(remaining.to_string()));
            }
            cleaned
        })
        .collect()
}

fn leading_system_messages(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .cloned()
        .collect()
}

/// Turn-based prefix retirement: keep system + tools + last `keep_turns` user/assistant pairs.
///
/// Returns `(retired_messages, token_estimate_before, token_estimate_after)` where
/// token estimates are rough character-based proxies (4 chars ≈ 1 token).
pub fn retire_prefix_messages_by_turns(
    messages: &[Value],
    keep_turns: usize,
) -> (Vec<Value>, usize, usize) {
    if messages.is_empty() {
        return (messages.to_vec(), 0, 0);
    }

    let chars_before: usize = messages.iter().map(|m| m.to_string().len()).sum();

    // Collect leading system messages (preserve unconditionally).
    let system_end = messages
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
        .unwrap_or(messages.len());
    let system_msgs: Vec<Value> = messages[..system_end].to_vec();

    // Find the trailing `keep_turns` user messages (each marks a turn boundary).
    let non_system = &messages[system_end..];
    let user_positions: Vec<usize> = non_system
        .iter()
        .enumerate()
        .filter(|(_, m)| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .map(|(i, _)| i)
        .collect();

    // If fewer user messages than keep_turns, no retirement needed.
    if user_positions.len() <= keep_turns {
        return (messages.to_vec(), 0, 0);
    }

    // The cutoff: keep everything from the (N - keep_turns)-th user message onward.
    let cutoff_in_non_system = user_positions[user_positions.len() - keep_turns];
    let cutoff_in_full = system_end + cutoff_in_non_system;

    let mut result = system_msgs;
    // Insert a system notice about the retired messages.
    let retired_count = cutoff_in_full - system_end;
    if retired_count > 0 {
        result.push(serde_json::json!({
            "role": "system",
            "content": format!(
                "[crabcache] {} older messages retired for prefix cache optimization.",
                retired_count
            )
        }));
    }
    result.extend_from_slice(&messages[cutoff_in_full..]);

    let chars_after: usize = result.iter().map(|m| m.to_string().len()).sum();

    (result, chars_before / 4, chars_after / 4)
}

fn active_messages_from_recovery_boundary(
    messages: &[Value],
) -> Option<(Vec<Value>, usize, serde_json::Value)> {
    let recovery_boundary_index = messages.iter().rposition(has_recovery_notice)?;

    let context_user_index = messages[..recovery_boundary_index]
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

    let leading = leading_system_messages(messages);
    let mut recovered_tail = Vec::new();
    if let Some(idx) = context_user_index {
        recovered_tail.push(messages[idx].clone());
    }
    recovered_tail.extend(messages[recovery_boundary_index..].to_vec());

    let mut active = leading.clone();
    active.push(serde_json::json!({"role": "system", "content": RECOVERY_SYSTEM_CONTENT}));
    active.extend(recovered_tail.clone());

    let kept_context = if context_user_index.is_some() { 1 } else { 0 };
    let retired = recovery_boundary_index.saturating_sub(leading.len() + kept_context);

    Some((
        active,
        retired,
        serde_json::json!({
            "strategy": "continued_recovery_boundary",
            "recovery_boundary_index": recovery_boundary_index,
            "context_user_index": context_user_index,
            "retired_prefix_messages": retired,
        }),
    ))
}

fn recover_messages_from_missing_reasoning(
    messages: &[Value],
    missing_indexes: &[usize],
) -> (Vec<Value>, usize, Option<String>, serde_json::Value) {
    let recovery_boundary_index = messages.iter().rposition(|m| {
        has_recovery_notice(m)
            && missing_indexes.iter().any(|&idx| {
                idx < messages.len() && !messages.get(idx).map(has_recovery_notice).unwrap_or(false)
            })
    });

    if let Some(rbi) = recovery_boundary_index {
        let context_user_index = messages[..rbi]
            .iter()
            .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));
        let leading = leading_system_messages(messages);
        let mut recovered_tail = Vec::new();
        if let Some(idx) = context_user_index {
            recovered_tail.push(messages[idx].clone());
        }
        recovered_tail.extend(messages[rbi..].to_vec());

        let mut recovered = leading.clone();
        recovered.push(serde_json::json!({"role": "system", "content": RECOVERY_SYSTEM_CONTENT}));
        recovered.extend(recovered_tail.clone());

        let kept_context = if context_user_index.is_some() { 1 } else { 0 };
        let omitted = rbi.saturating_sub(leading.len() + kept_context);

        return (
            recovered,
            omitted,
            None,
            serde_json::json!({
                "strategy": "recovery_boundary",
                "missing_indexes": missing_indexes,
                "recovery_boundary_index": rbi,
                "dropped_messages": omitted,
            }),
        );
    }

    let last_user_index = messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

    if let Some(lui) = last_user_index {
        let mut recovered = leading_system_messages(messages);
        let omitted = messages.len() - recovered.len() - 1;
        recovered.push(serde_json::json!({"role": "system", "content": RECOVERY_SYSTEM_CONTENT}));
        recovered.push(messages[lui].clone());
        let notice =
            should_attach_recovery_notice(messages).then(|| RECOVERY_NOTICE_CONTENT.to_string());
        return (
            recovered,
            omitted,
            notice,
            serde_json::json!({
                "strategy": "latest_user",
                "missing_indexes": missing_indexes,
                "last_user_index": lui,
                "dropped_messages": omitted,
            }),
        );
    }

    (
        messages.to_vec(),
        0,
        None,
        serde_json::json!({"strategy": "none", "missing_indexes": missing_indexes}),
    )
}

/// Truncate to leading system + last user (drops tool-call assistants that still lack reasoning).
#[allow(dead_code)]
fn force_latest_user_recover(messages: &[Value]) -> Option<(Vec<Value>, usize, Option<String>)> {
    let last_user_index = messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))?;
    let mut recovered = leading_system_messages(messages);
    let omitted = messages.len().saturating_sub(recovered.len() + 1);
    recovered.push(serde_json::json!({"role": "system", "content": RECOVERY_SYSTEM_CONTENT}));
    recovered.push(messages[last_user_index].clone());
    let notice =
        should_attach_recovery_notice(messages).then(|| RECOVERY_NOTICE_CONTENT.to_string());
    Some((recovered, omitted, notice))
}

pub fn reasoning_cache_namespace(
    upstream_base_url: &str,
    upstream_model: &str,
    thinking: &Value,
    reasoning_effort: &str,
    authorization: Option<&str>,
    project_id: Option<&str>,
) -> String {
    use sha2::{Digest, Sha256};
    let auth_hash = authorization
        .map(|a| {
            let mut hasher = Sha256::new();
            hasher.update(a.as_bytes());
            hex::encode(hasher.finalize())
        })
        .unwrap_or_default();

    let payload = serde_json::json!({
        "base_url": upstream_base_url,
        "model": reasoning_model_family(upstream_model),
        "thinking": thinking,
        "reasoning_effort": reasoning_effort,
        "authorization_hash": auth_hash,
        "project_id": project_id.filter(|s| !s.is_empty()),
    });
    let canonical = serde_json::to_string(&payload).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

fn reasoning_model_family(upstream_model: &str) -> &str {
    match upstream_model {
        "deepseek-v4-pro" | "deepseek-v4-flash" => "deepseek-v4",
        other => other,
    }
}

#[derive(Clone)]
struct PrefixSnapshot {
    message_count: usize,
    hash: String,
}

static PREFIX_SNAPSHOTS: LazyLock<Mutex<HashMap<String, PrefixSnapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static IMMUTABLE_PREFIX_BLOCKS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn messages_prefix_hash(messages: &[Value], len: usize) -> String {
    let end = len.min(messages.len());
    let slice = &messages[..end];
    let encoded = serde_json::to_string(slice).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(encoded.as_bytes());
    hex::encode(hasher.finalize())
}

fn validate_prefix_append_only(scope: &str, messages: &[Value]) {
    let Ok(mut guard) = PREFIX_SNAPSHOTS.lock() else {
        return;
    };
    if let Some(snap) = guard.get(scope)
        && messages.len() > snap.message_count
    {
        let current = messages_prefix_hash(messages, snap.message_count);
        if current != snap.hash {
            global_metrics().record_prefix_break();
            tracing::warn!(
                scope = scope,
                expected_len = snap.message_count,
                "Non-append-only message prefix detected"
            );
        }
    }
    if !messages.is_empty() {
        guard.insert(
            scope.to_string(),
            PrefixSnapshot {
                message_count: messages.len(),
                hash: messages_prefix_hash(messages, messages.len()),
            },
        );
    }
}

fn track_immutable_prefix_block(scope: &str, block_hash: &str) {
    let Ok(mut guard) = IMMUTABLE_PREFIX_BLOCKS.lock() else {
        return;
    };
    if let Some(prev) = guard.get(scope)
        && prev != block_hash
    {
        global_metrics().record_prefix_block_drift();
        tracing::warn!(scope = scope, "Immutable system/tools prefix block drifted");
    }
    guard.insert(scope.to_string(), block_hash.to_string());
}

fn maybe_append_context_summary(messages: &mut Vec<Value>, threshold: usize) {
    if threshold == 0 || messages.len() <= threshold {
        return;
    }
    let summary = format!(
        "[CrabCache] Long context ({} messages). Earlier turns are preserved above for upstream prefix cache; continue from this summary if needed.",
        messages.len()
    );
    messages.push(serde_json::json!({
        "role": "user",
        "content": summary,
    }));
    global_metrics().record_context_summary_appended();
}

/// Parsed DeepSeek V4 model suffix (`-max` / `-none`), aligned with new-api `ParseDeepSeekV4ThinkingSuffix`.
#[derive(Debug, Clone, Default)]
pub struct DeepSeekV4SuffixParse {
    pub base_model: String,
    pub thinking_mode_override: Option<String>,
    pub reasoning_effort_override: Option<String>,
    pub matched: bool,
}

/// Strip `-max` or `-none` from `deepseek-v4-*` model names and map to thinking settings.
pub fn parse_deepseek_v4_thinking_suffix(model_name: &str) -> DeepSeekV4SuffixParse {
    for (suffix, thinking, effort) in [
        ("-none", "disabled", None),
        ("-max", "enabled", Some("max")),
    ] {
        if let Some(base) = model_name.strip_suffix(suffix)
            && base.starts_with("deepseek-v4-")
        {
            return DeepSeekV4SuffixParse {
                base_model: base.to_string(),
                thinking_mode_override: Some(thinking.to_string()),
                reasoning_effort_override: effort.map(str::to_string),
                matched: true,
            };
        }
    }
    DeepSeekV4SuffixParse {
        base_model: model_name.to_string(),
        ..Default::default()
    }
}

pub fn upstream_model_for(original_model: &str, fallback_model: &str) -> String {
    if original_model.starts_with("deepseek-") {
        original_model.to_string()
    } else {
        fallback_model.to_string()
    }
}

fn resolve_upstream_model(computed: String, alias_upstream: Option<&str>) -> String {
    alias_upstream.map(|s| s.to_string()).unwrap_or(computed)
}

fn apply_effective_user_id(
    prepared: &mut serde_json::Map<String, Value>,
    effective_user_id: Option<&str>,
) {
    match effective_user_id.filter(|s| !s.is_empty()) {
        Some(id) => {
            prepared.insert("user_id".into(), Value::String(id.to_string()));
        }
        None => {
            prepared.remove("user_id");
        }
    }
}

fn filter_supported_request_fields(payload: &Value) -> serde_json::Map<String, Value> {
    let supported_set: std::collections::HashSet<&str> =
        SUPPORTED_REQUEST_FIELDS.iter().copied().collect();
    let mut prepared: serde_json::Map<String, Value> = payload
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, _)| supported_set.contains(k.as_str()))
        .collect();
    if !prepared.contains_key("max_tokens")
        && let Some(mct) = payload.get("max_completion_tokens")
    {
        prepared.insert("max_tokens".into(), mct.clone());
    }
    // Normalize roles that non-OpenAI backends don't support.
    normalize_message_roles_in_place(&mut prepared);
    prepared
}

/// Map unsupported roles (`developer`, `function`) to their equivalents (`system`, `tool`)
/// directly in the prepared JSON map. Used by pipelines that don't go through `normalize_messages`.
fn normalize_message_roles_in_place(map: &mut serde_json::Map<String, Value>) {
    if let Some(messages) = map.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            if let Some(role) = msg.get("role").and_then(|r| r.as_str()) {
                let mapped = match role {
                    "developer" => Some("system"),
                    "function" => Some("tool"),
                    _ => None,
                };
                if let Some(new_role) = mapped {
                    if let Some(obj) = msg.as_object_mut() {
                        obj.insert("role".into(), Value::String(new_role.into()));
                    }
                }
            }
        }
    }
    if let Some(tools) = map.get("tools").and_then(|v| v.as_array()) {
        map.insert(
            "tools".into(),
            Value::Array(crate::codex_tools::normalize_codex_tools_for_upstream(tools)),
        );
    }
    crate::codex_tools::ensure_codex_file_tools_from_context(map);
}

#[derive(Debug, Clone)]
pub struct LightPreparedRequest {
    pub payload: Value,
    pub original_model: String,
    pub upstream_model: String,
}

/// DeepSeek non-V4: OpenAI field whitelist + message normalization without thinking/reasoning store.
pub fn prepare_light_request(
    payload: &Value,
    fallback_model: &str,
    alias_upstream: Option<&str>,
    effective_user_id: Option<&str>,
) -> LightPreparedRequest {
    let original_model = payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(fallback_model)
        .to_string();
    let computed = if original_model.starts_with("deepseek-") {
        original_model.clone()
    } else {
        fallback_model.to_string()
    };
    let upstream_model = resolve_upstream_model(computed, alias_upstream);

    let mut prepared = filter_supported_request_fields(payload);
    prepared.insert("model".into(), Value::String(upstream_model.clone()));

    if let Some(tools) = prepared.get("tools").and_then(|t| t.as_array()).cloned() {
        prepared.insert(
            "tools".into(),
            Value::Array(tools.iter().map(normalize_tool).collect()),
        );
    } else if let Some(functions) = payload.get("functions").and_then(|f| f.as_array()).cloned() {
        prepared.insert(
            "tools".into(),
            Value::Array(functions.iter().map(legacy_function_to_tool).collect()),
        );
    }

    if let Some(tool_choice) = prepared.get("tool_choice").cloned() {
        if let Some(normalized) = normalize_tool_choice_for_deepseek(&tool_choice) {
            prepared.insert("tool_choice".into(), normalized);
        } else {
            prepared.remove("tool_choice");
        }
    } else if let Some(function_call) = payload.get("function_call")
        && let Some(converted) = convert_function_call(function_call)
    {
        prepared.insert("tool_choice".into(), converted);
    }

    let raw_messages = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let normalized = normalize_messages(raw_messages, None, "", None, false, false);
    prepared.insert("messages".into(), Value::Array(normalized.messages));
    apply_effective_user_id(&mut prepared, effective_user_id);

    LightPreparedRequest {
        payload: Value::Object(prepared),
        original_model,
        upstream_model,
    }
}

#[derive(Debug, Clone)]
pub struct GenericPreparedRequest {
    pub payload: Value,
    pub model: String,
    /// Messages removed by optional prefix retirement (MiMo feature).
    pub retired_prefix_messages: usize,
    /// Assistant tool_calls whose `function.arguments` JSON was repaired for MiMo.
    pub tool_calls_repaired: usize,
    /// When `None`, upstream should send the original request body (no `serde_json::to_vec`).
    pub serialized_body: Option<Vec<u8>>,
}

/// True when MiMo prepare only filtered unsupported top-level fields or normalized `model`.
fn mimo_prepare_changes_wire_body(
    payload: &Value,
    prepared: &serde_json::Map<String, Value>,
) -> bool {
    let supported_set: std::collections::HashSet<&str> =
        SUPPORTED_REQUEST_FIELDS.iter().copied().collect();
    if let Some(obj) = payload.as_object() {
        for key in obj.keys() {
            if !supported_set.contains(key.as_str()) {
                return true;
            }
        }
    } else {
        return true;
    }
    let raw_model = payload.get("model").and_then(|m| m.as_str()).unwrap_or("");
    let normalized = normalize_mimo_model(raw_model);
    if payload.get("model").and_then(|m| m.as_str()) != Some(normalized.as_str()) {
        return true;
    }
    if payload.get("max_completion_tokens").is_some()
        && payload
            .as_object()
            .is_none_or(|o| !o.contains_key("max_tokens"))
    {
        return true;
    }
    if payload.get("messages") != prepared.get("messages") {
        return true;
    }
    if payload.get("tools") != prepared.get("tools") {
        return true;
    }
    false
}

/// Normalize MiMo model id for the OpenAI-compatible API (`xiaomi/mimo-v2.5-pro`).
pub fn normalize_mimo_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_lowercase().replace('_', "-");
    if lower.starts_with("xiaomi/") {
        return lower;
    }
    if lower.starts_with("mimo-") {
        return format!("xiaomi/{lower}");
    }
    lower
}

/// MiMo relay: field filter + OpenAI model id normalization; optional turn-based prefix retirement.
pub fn prepare_mimo_request(
    payload: &Value,
    fallback_model: &str,
    retire_prefix: bool,
    keep_recent_turns: usize,
) -> GenericPreparedRequest {
    let raw = payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(fallback_model);
    let model = {
        let normalized = normalize_mimo_model(raw);
        if normalized.is_empty() {
            normalize_mimo_model(fallback_model)
        } else {
            normalized
        }
    };
    let mut prepared = filter_supported_request_fields(payload);
    prepared.insert("model".into(), Value::String(model.clone()));

    let mut retired_prefix_messages = 0usize;
    if retire_prefix {
        if let Some(messages) = prepared.get("messages").and_then(|m| m.as_array()) {
            // Session store already tail-capped upstream; avoid injecting another [crabcache] notice.
            if messages.len() > 48 {
                let (trimmed, _before, _after) =
                    retire_prefix_messages_by_turns(messages, keep_recent_turns);
                retired_prefix_messages = messages.len().saturating_sub(trimmed.len());
                if retired_prefix_messages > 0 {
                    prepared.insert("messages".into(), Value::Array(trimmed));
                }
            }
        }
    }

    let mut tool_calls_repaired = 0usize;
    if let Some(messages) = prepared.get_mut("messages").and_then(|m| m.as_array_mut()) {
        let taken = std::mem::take(messages);
        let (sanitized, repaired) = sanitize_mimo_tool_calls_in_messages(taken);
        tool_calls_repaired = repaired;
        *messages = sanitized;
    }

    let payload_value = Value::Object(prepared);
    let serialized_body = if retired_prefix_messages > 0
        || tool_calls_repaired > 0
        || mimo_prepare_changes_wire_body(payload, payload_value.as_object().expect("object"))
    {
        Some(serde_json::to_vec(&payload_value).unwrap_or_default())
    } else {
        None
    };

    if tool_calls_repaired > 0 {
        debug!(
            tool_calls_repaired,
            "MiMo prepare: repaired tool call arguments JSON"
        );
    }

    GenericPreparedRequest {
        payload: payload_value,
        model,
        retired_prefix_messages,
        tool_calls_repaired,
        serialized_body,
    }
}

/// Minimal relay: field filter + token alias; messages and model unchanged.
pub fn prepare_generic_request(payload: &Value) -> GenericPreparedRequest {
    let model = payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4")
        .to_string();
    let mut prepared = filter_supported_request_fields(payload);
    if !prepared.contains_key("model") {
        prepared.insert("model".into(), Value::String(model.clone()));
    }
    let payload_value = Value::Object(prepared);
    GenericPreparedRequest {
        serialized_body: Some(serde_json::to_vec(&payload_value).unwrap_or_default()),
        payload: payload_value,
        model,
        retired_prefix_messages: 0,
        tool_calls_repaired: 0,
    }
}

#[derive(Debug, Clone)]
pub struct PreparedRequest {
    pub payload: Value,
    pub original_model: String,
    pub upstream_model: String,
    pub cache_namespace: String,
    pub patched_reasoning_messages: usize,
    pub missing_reasoning_messages: usize,
    pub recovered_reasoning_messages: usize,
    pub recovery_dropped_messages: usize,
    pub retired_prefix_messages: usize,
    pub prefix_tokens_before: usize,
    pub prefix_tokens_after: usize,
    pub recovery_notice: Option<String>,
    pub record_response_scope: String,
    pub record_response_messages: Vec<Value>,
    pub record_response_contexts: Vec<(String, Vec<Value>)>,
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_upstream_request(
    payload: &Value,
    store: Option<&ReasoningBackend>,
    upstream_base_url: &str,
    fallback_model: &str,
    thinking_mode: &str,
    reasoning_effort: &str,
    missing_reasoning_strategy: &str,
    context_summary_message_threshold: usize,
    prefix_validate: bool,
    authorization: Option<&str>,
    stable_session_id: Option<&str>,
    alias_upstream: Option<&str>,
    effective_user_id: Option<&str>,
) -> PreparedRequest {
    let original_model = payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(fallback_model)
        .to_string();
    let v4_suffix = parse_deepseek_v4_thinking_suffix(&original_model);
    let upstream_model = resolve_upstream_model(
        upstream_model_for(&v4_suffix.base_model, fallback_model),
        alias_upstream,
    );
    let thinking_mode = v4_suffix
        .thinking_mode_override
        .as_deref()
        .unwrap_or(thinking_mode);
    let reasoning_effort = v4_suffix
        .reasoning_effort_override
        .as_deref()
        .unwrap_or(reasoning_effort);

    let supported_set: std::collections::HashSet<&str> =
        SUPPORTED_REQUEST_FIELDS.iter().copied().collect();
    let mut prepared: serde_json::Map<String, Value> = payload
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, _)| supported_set.contains(k.as_str()))
        .collect();

    if !prepared.contains_key("max_tokens")
        && let Some(mct) = payload.get("max_completion_tokens")
    {
        prepared.insert("max_tokens".into(), mct.clone());
    }

    prepared.insert("model".into(), Value::String(upstream_model.clone()));

    if prepared
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
    {
        let stream_options = prepared
            .get("stream_options")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));
        let mut so = stream_options.as_object().cloned().unwrap_or_default();
        so.insert("include_usage".into(), Value::Bool(true));
        prepared.insert("stream_options".into(), Value::Object(so));
    }

    if let Some(tools) = prepared.get("tools").and_then(|t| t.as_array()).cloned() {
        prepared.insert(
            "tools".into(),
            Value::Array(tools.iter().map(normalize_tool).collect()),
        );
    } else if let Some(functions) = payload.get("functions").and_then(|f| f.as_array()).cloned() {
        prepared.insert(
            "tools".into(),
            Value::Array(functions.iter().map(legacy_function_to_tool).collect()),
        );
    }

    if let Some(tool_choice) = prepared.get("tool_choice").cloned() {
        if let Some(normalized) = normalize_tool_choice_for_deepseek(&tool_choice) {
            prepared.insert("tool_choice".into(), normalized);
        } else {
            prepared.remove("tool_choice");
        }
    } else if let Some(function_call) = payload.get("function_call").cloned()
        && let Some(converted) = convert_function_call(&function_call)
    {
        prepared.insert("tool_choice".into(), converted);
    }

    let thinking_enabled = thinking_mode == "enabled";
    let thinking_disabled = thinking_mode == "disabled";
    let mut thinking_obj = serde_json::Map::new();
    thinking_obj.insert("type".into(), Value::String(thinking_mode.to_string()));
    prepared.insert("thinking".into(), Value::Object(thinking_obj));

    if thinking_enabled {
        let effort = payload
            .get("reasoning_effort")
            .and_then(|e| e.as_str())
            .unwrap_or(reasoning_effort);
        prepared.insert(
            "reasoning_effort".into(),
            Value::String(normalize_reasoning_effort(effort)),
        );
    }

    let cache_namespace = reasoning_cache_namespace(
        upstream_base_url,
        &upstream_model,
        prepared.get("thinking").unwrap_or(&Value::Null),
        prepared
            .get("reasoning_effort")
            .and_then(|e| e.as_str())
            .unwrap_or(reasoning_effort),
        authorization,
        effective_user_id,
    );

    let raw_inbound_messages = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let inbound_had_recovery_notice = history_has_recovery_notice(raw_inbound_messages);
    // Cursor echoes prior recovery notices into assistant content; strip before repair so
    // active_messages_from_recovery_boundary does not freeze history to a tiny tail.
    let inbound_messages: Vec<Value> = strip_recovery_notice_for_upstream(raw_inbound_messages);

    let pre_repair = normalize_messages(
        &inbound_messages,
        None,
        &cache_namespace,
        stable_session_id,
        false,
        !thinking_disabled,
    );
    let mut record_response_messages = pre_repair.messages.clone();
    let record_response_scope = resolve_reasoning_scope(
        stable_session_id,
        &record_response_messages,
        &cache_namespace,
    );

    let mut messages_for_repair = pre_repair.messages.clone();
    let mut retired_prefix_messages = 0;
    let mut recovered_count = 0;
    let mut recovery_dropped_messages = 0;
    let mut recovery_notice = None;
    let mut prefix_tokens_before: usize = 0;
    let mut prefix_tokens_after: usize = 0;

    let stable_scope = stable_session_id.filter(|s| !s.trim().is_empty());

    // deepseek-cursor-proxy: boundary only on `recover` without stable session (see transform.py).
    // Stable session (client_key / conversation): skip boundary — preserves tool history (H-G fix).
    if thinking_enabled
        && missing_reasoning_strategy == "recover"
        && stable_scope.is_none()
        && let Some((active, retired, _step)) =
            active_messages_from_recovery_boundary(&pre_repair.messages)
    {
        messages_for_repair = active;
        retired_prefix_messages = retired;
    }

    // ── Turn-based prefix retirement (secondary optimization) ─────────
    // Retire older turns to improve prefix cache hit rate, preserving system + recent N turns.
    // Only applies when messages are long enough to benefit (>= 20 messages).
    const MIN_MESSAGES_FOR_TURN_RETIREMENT: usize = 20;
    const KEEP_RECENT_TURNS: usize = 5;
    if retired_prefix_messages == 0 && messages_for_repair.len() >= MIN_MESSAGES_FOR_TURN_RETIREMENT
    {
        let (trimmed, tokens_before_est, tokens_after_est) =
            retire_prefix_messages_by_turns(&messages_for_repair, KEEP_RECENT_TURNS);
        if trimmed.len() < messages_for_repair.len() {
            let retired_now = messages_for_repair.len() - trimmed.len();
            retired_prefix_messages += retired_now;
            messages_for_repair = trimmed;
            prefix_tokens_before = tokens_before_est;
            prefix_tokens_after = tokens_after_est;
            debug!(
                tokens_before = tokens_before_est,
                tokens_after = tokens_after_est,
                retired_now,
                "turn-based prefix retirement applied"
            );
        }
    }

    let tools_for_block = prepared.get("tools").cloned();
    let block_hash = crab_composition::immutable_prefix_block_hash(
        &pre_repair.messages,
        tools_for_block.as_ref(),
    );
    track_immutable_prefix_block(&record_response_scope, &block_hash);

    if prefix_validate {
        validate_prefix_append_only(&record_response_scope, &pre_repair.messages);
    }

    let mut result = normalize_messages(
        &messages_for_repair,
        store,
        &cache_namespace,
        stable_session_id,
        thinking_enabled,
        !thinking_disabled,
    );

    let mut missing_indexes = result.missing_indexes;

    // Stable session: never truncate to latest_user; patch reasoning in place so tool history grows.
    if stable_scope.is_some() && thinking_enabled && !missing_indexes.is_empty() {
        let inline_patched = patch_missing_reasoning_inplace(
            &mut result.messages,
            &missing_indexes,
            store,
            &cache_namespace,
            stable_scope,
        );
        let repaired_messages = result.messages.clone();
        result = normalize_messages(
            &repaired_messages,
            store,
            &cache_namespace,
            stable_session_id,
            thinking_enabled,
            !thinking_disabled,
        );
        result.patched_count += inline_patched;
        missing_indexes = result.missing_indexes;
        // Update record_response_messages to reflect patched reasoning so
        // ReasoningStore recording uses the same context as the upstream request.
        if inline_patched > 0 {
            record_response_messages = result.messages.clone();
        }
    }

    while !missing_indexes.is_empty()
        && missing_reasoning_strategy == "recover"
        && stable_scope.is_none()
    {
        let (recovered, dropped, notice, _step) =
            recover_messages_from_missing_reasoning(&result.messages, &missing_indexes);
        if dropped == 0 {
            break;
        }
        recovered_count += missing_indexes.len();
        recovery_dropped_messages += dropped;
        if notice.is_some() {
            recovery_notice = notice;
        }
        result = normalize_messages(
            &recovered,
            store,
            &cache_namespace,
            stable_session_id,
            thinking_enabled,
            !thinking_disabled,
        );
        missing_indexes = result.missing_indexes;
    }

    let mut final_messages = result.messages.clone();
    maybe_append_context_summary(&mut final_messages, context_summary_message_threshold);

    let active_scope =
        resolve_reasoning_scope(stable_session_id, &final_messages, &cache_namespace);
    let mut record_response_contexts = Vec::new();
    record_response_contexts.push((
        record_response_scope.clone(),
        record_response_messages.clone(),
    ));
    if active_scope != record_response_scope {
        record_response_contexts.push((active_scope, final_messages.clone()));
    }

    if recovery_notice.is_some() && inbound_had_recovery_notice {
        recovery_notice = None;
    }

    let upstream_messages = strip_recovery_notice_for_upstream(&final_messages);
    prepared.insert("messages".into(), Value::Array(upstream_messages));
    apply_effective_user_id(&mut prepared, effective_user_id);

    PreparedRequest {
        payload: Value::Object(prepared),
        original_model,
        upstream_model,
        cache_namespace,
        patched_reasoning_messages: result.patched_count,
        missing_reasoning_messages: missing_indexes.len(),
        recovered_reasoning_messages: recovered_count,
        recovery_dropped_messages,
        retired_prefix_messages,
        prefix_tokens_before,
        prefix_tokens_after,
        recovery_notice,
        record_response_scope,
        record_response_messages,
        record_response_contexts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReasoningBackend;

    #[test]
    fn test_normalize_reasoning_effort() {
        assert_eq!(normalize_reasoning_effort("low"), "high");
        assert_eq!(normalize_reasoning_effort("max"), "max");
        assert_eq!(normalize_reasoning_effort("xhigh"), "max");
        assert_eq!(normalize_reasoning_effort("medium"), "high");
    }

    #[test]
    fn test_extract_text_content() {
        assert_eq!(
            extract_text_content(&Value::String("hello".into())),
            Some("hello".to_string())
        );
        assert_eq!(extract_text_content(&Value::Null), None);
    }

    #[test]
    fn test_strip_cursor_thinking_blocks() {
        let input = "<think>some reasoning</think>\nactual content";
        let result = strip_cursor_thinking_blocks(input);
        assert!(!result.contains("think"));
        assert!(result.contains("actual content"));
    }

    #[test]
    fn reasoning_cache_namespace_differs_by_authorization() {
        let thinking = serde_json::json!({"type": "enabled"});
        let a = reasoning_cache_namespace(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &thinking,
            "max",
            Some("Bearer sk-cc-aaa"),
            None,
        );
        let b = reasoning_cache_namespace(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &thinking,
            "max",
            Some("Bearer sk-cc-bbb"),
            None,
        );
        assert_ne!(a, b);
        let none = reasoning_cache_namespace(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &thinking,
            "max",
            None,
            None,
        );
        assert_ne!(a, none);
    }

    #[test]
    fn legacy_recovery_notice_still_detected() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": "[deepseek-cursor-proxy] Refreshed reasoning_content history.\n\nrest"
        });
        let stripped = strip_recovery_notice_for_upstream(&[msg]);
        assert_eq!(
            stripped[0].get("content").and_then(|c| c.as_str()),
            Some("rest")
        );
    }

    #[test]
    fn test_parse_deepseek_v4_thinking_suffix() {
        let max = parse_deepseek_v4_thinking_suffix("deepseek-v4-flash-max");
        assert!(max.matched);
        assert_eq!(max.base_model, "deepseek-v4-flash");
        assert_eq!(max.thinking_mode_override.as_deref(), Some("enabled"));
        assert_eq!(max.reasoning_effort_override.as_deref(), Some("max"));

        let none = parse_deepseek_v4_thinking_suffix("deepseek-v4-pro-none");
        assert!(none.matched);
        assert_eq!(none.base_model, "deepseek-v4-pro");
        assert_eq!(none.thinking_mode_override.as_deref(), Some("disabled"));
        assert!(none.reasoning_effort_override.is_none());

        let plain = parse_deepseek_v4_thinking_suffix("deepseek-v4-flash");
        assert!(!plain.matched);
        assert_eq!(plain.base_model, "deepseek-v4-flash");
    }

    #[test]
    fn test_upstream_model_for() {
        assert_eq!(
            upstream_model_for("deepseek-v4-pro", "fallback"),
            "deepseek-v4-pro"
        );
        assert_eq!(
            upstream_model_for("gpt-4", "deepseek-v4-pro"),
            "deepseek-v4-pro"
        );
    }

    #[test]
    fn prepare_light_request_does_not_inject_thinking() {
        let payload = serde_json::json!({
            "model": "deepseek-chat",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "ok", "reasoning_content": "secret"}
            ]
        });
        let result = prepare_light_request(&payload, "deepseek-v4-pro", None, None);
        assert!(!result.payload.to_string().contains("thinking"));
        assert!(!result.payload.to_string().contains("reasoning_content"));
        assert_eq!(result.upstream_model, "deepseek-chat");
    }

    #[test]
    fn prepare_generic_request_keeps_model() {
        let payload = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let result = prepare_generic_request(&payload);
        assert_eq!(result.model, "gpt-4");
        assert!(!result.payload.to_string().contains("thinking"));
    }

    #[test]
    fn normalize_mimo_model_adds_vendor_prefix() {
        assert_eq!(
            normalize_mimo_model("mimo-v2.5-pro"),
            "xiaomi/mimo-v2.5-pro"
        );
        assert_eq!(
            normalize_mimo_model("xiaomi/mimo-v2-flash"),
            "xiaomi/mimo-v2-flash"
        );
        assert_eq!(
            normalize_mimo_model("MiMo-V2.5-Pro"),
            "xiaomi/mimo-v2.5-pro"
        );
    }

    #[test]
    fn prepare_mimo_request_serializes_when_tools_are_normalized() {
        let payload = serde_json::json!({
            "model": "xiaomi/mimo-v2.5-pro",
            "stream": true,
            "messages": [
                {"role": "system", "content": "Use apply_patch to edit files."},
                {"role": "user", "content": "hi"},
            ],
            "tools": [
                {"type": "function", "name": "exec_command", "parameters": {"type": "object"}},
                {"type": "namespace", "name": "multi_agent_v1"},
            ],
        });
        let result = prepare_mimo_request(&payload, "xiaomi/mimo-v2.5-pro", false, 6);
        assert!(
            result.serialized_body.is_some(),
            "tool normalization/injection must rewrite upstream body"
        );
        let outbound: serde_json::Value =
            serde_json::from_slice(result.serialized_body.as_ref().unwrap()).unwrap();
        let names: Vec<_> = outbound["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"apply_patch"));
        assert!(names.contains(&"exec_command"));
        assert!(!names.iter().any(|n| *n == "multi_agent_v1"));
    }

    #[test]
    fn prepare_mimo_request_preserves_custom_apply_patch() {
        let payload = serde_json::json!({
            "model": "xiaomi/mimo-v2.5-pro",
            "stream": true,
            "messages": [
                {"role": "system", "content": "Use apply_patch to edit files."},
                {"role": "user", "content": "hi"},
            ],
            "tools": [
                {"type": "function", "name": "exec_command", "parameters": {"type": "object"}},
                {"type": "custom", "name": "apply_patch", "description": "patch"},
            ],
        });
        let result = prepare_mimo_request(&payload, "xiaomi/mimo-v2.5-pro", false, 6);
        let names: Vec<_> = result.payload["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"exec_command"));
        assert!(names.contains(&"apply_patch"));
    }

    #[test]
    fn prepare_mimo_request_normalizes_model() {
        let payload = serde_json::json!({
            "model": "mimo-v2.5-pro",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let result = prepare_mimo_request(&payload, "xiaomi/mimo-v2.5-pro", false, 6);
        assert_eq!(result.model, "xiaomi/mimo-v2.5-pro");
        assert!(!result.payload.to_string().contains("thinking"));
        assert_eq!(result.retired_prefix_messages, 0);
        assert!(
            result.serialized_body.is_some(),
            "model normalization requires serialized upstream body"
        );
    }

    #[test]
    fn prepare_mimo_request_short_circuits_unchanged_body() {
        let payload = serde_json::json!({
            "model": "xiaomi/mimo-v2.5-pro",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true
        });
        let result = prepare_mimo_request(&payload, "xiaomi/mimo-v2.5-pro", false, 6);
        assert_eq!(result.retired_prefix_messages, 0);
        assert!(
            result.serialized_body.is_none(),
            "expected O(1) upstream body when payload already MiMo-clean"
        );
    }

    #[test]
    fn repair_tool_arguments_strips_trailing_json() {
        let raw = r#"{"cmd":"ls"}{"ignored":true}"#;
        let fixed = repair_tool_arguments_json(raw);
        assert_eq!(fixed, r#"{"cmd":"ls"}"#);
    }

    #[test]
    fn prepare_mimo_request_repairs_tool_call_arguments() {
        let payload = serde_json::json!({
            "model": "mimo-v2.5-pro",
            "messages": [{
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "exec_command",
                        "arguments": r#"{"cmd":"echo hi"}{"trailing":"bad"}"#
                    }
                }]
            }],
        });
        let result = prepare_mimo_request(&payload, "mimo-v2.5-pro", false, 6);
        assert!(result.tool_calls_repaired >= 1);
        let args = result.payload["messages"][0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(args, r#"{"cmd":"echo hi"}"#);
    }

    #[test]
    fn prepare_mimo_request_retires_old_turns() {
        let mut messages = Vec::new();
        for i in 0..24 {
            messages.push(serde_json::json!({
                "role": if i % 2 == 0 { "user" } else { "assistant" },
                "content": format!("turn-{i} with padding {}", "x".repeat(400)),
            }));
        }
        let payload = serde_json::json!({
            "model": "mimo-v2.5-pro",
            "messages": messages,
        });
        let before = payload.to_string().len();
        let result = prepare_mimo_request(&payload, "xiaomi/mimo-v2.5-pro", true, 6);
        let after = result.payload.to_string().len();
        assert!(result.retired_prefix_messages > 0, "expected retire");
        assert!(after < before, "upstream body should shrink");
        let out_msgs = result
            .payload
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(out_msgs < 24);
    }

    #[test]
    fn test_prepare_upstream_request_alias_upstream() {
        let payload = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hello"}],
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            Some("deepseek-v4-pro"),
            None,
        );
        assert_eq!(result.original_model, "gpt-4o");
        assert_eq!(result.upstream_model, "deepseek-v4-pro");
    }

    #[test]
    fn test_prepare_upstream_request_injects_user_id() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "user_id": "wrong-tenant",
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            None,
            Some("proj-a"),
        );
        assert_eq!(
            result.payload.get("user_id").and_then(|v| v.as_str()),
            Some("proj-a")
        );
    }

    #[test]
    fn reasoning_cache_namespace_differs_by_project() {
        let thinking = serde_json::json!({"type": "enabled"});
        let a = reasoning_cache_namespace(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &thinking,
            "max",
            None,
            Some("proj-a"),
        );
        let b = reasoning_cache_namespace(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &thinking,
            "max",
            None,
            Some("proj-b"),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn test_prepare_upstream_request_basic() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": true,
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        assert_eq!(result.original_model, "deepseek-v4-pro");
        assert_eq!(result.upstream_model, "deepseek-v4-pro");
        assert_eq!(result.missing_reasoning_messages, 0);
    }

    #[test]
    fn test_prepare_upstream_reject_does_not_recover_missing() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "plan"},
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": "{}"}
                    }]
                }
            ],
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "reject",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        assert!(result.missing_reasoning_messages > 0);
        assert_eq!(result.recovered_reasoning_messages, 0);
    }

    #[test]
    fn test_deepseek_tool_choice_function_downgraded_to_auto() {
        let tc = serde_json::json!({
            "type": "function",
            "function": { "name": "list_dir" }
        });
        let out = normalize_tool_choice_for_deepseek(&tc).expect("normalized");
        assert_eq!(out, serde_json::json!("auto"));
    }

    #[test]
    fn test_prepare_converts_legacy_functions() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "functions": [{"name": "test", "description": "A test", "parameters": {}}],
            "function_call": "auto",
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        let tools = result
            .payload
            .get("tools")
            .and_then(|t| t.as_array())
            .unwrap();
        assert_eq!(tools.len(), 1);
        assert!(result.payload.get("tool_choice").is_some());
    }

    #[test]
    fn test_recover_when_store_miss_under_thinking() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "system", "content": "fixed agent prefix"},
                {"role": "user", "content": "plan"},
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": "{}"}
                    }]
                },
                {"role": "user", "content": "continue"}
            ],
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        assert!(result.recovered_reasoning_messages > 0);
        assert!(result.recovery_dropped_messages > 0);
        assert_eq!(result.missing_reasoning_messages, 0);
        let msgs = result
            .payload
            .get("messages")
            .and_then(|m| m.as_array())
            .unwrap();
        assert!(msgs.iter().any(|m| {
            m.get("role").and_then(|r| r.as_str()) == Some("system")
                && m.get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.contains("CrabCache recovered"))
                    .unwrap_or(false)
        }));
    }

    #[test]
    fn test_thinking_disabled_does_not_require_reasoning() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "plan"},
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": "{}"}
                    }]
                }
            ],
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "disabled",
            "medium",
            "recover",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        assert_eq!(result.missing_reasoning_messages, 0);
    }

    #[test]
    fn test_stable_session_preserves_growing_tool_history() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "explore the repo"},
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": "{}"}
                    }]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "ok"},
                {"role": "assistant", "content": "partial progress"},
                {"role": "user", "content": "explore the repo"}
            ],
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            Some("client:stable-1"),
            None,
            None,
        );
        let msgs = result
            .payload
            .get("messages")
            .and_then(|m| m.as_array())
            .expect("messages");
        assert!(
            msgs.len() >= 5,
            "stable session must not collapse to latest_user tail, got {}",
            msgs.len()
        );
        assert_eq!(result.recovered_reasoning_messages, 0);
        assert_eq!(result.recovery_dropped_messages, 0);
        assert_eq!(result.missing_reasoning_messages, 0);
        assert!(
            result.patched_reasoning_messages >= 1,
            "inline patch should satisfy missing reasoning"
        );
    }

    #[test]
    fn test_stable_session_id_fills_from_store_without_recover() {
        let store = ReasoningBackend::open_sqlite(":memory:", Some(3600), Some(1000))
            .expect("memory store");
        let assistant = serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "list_dir", "arguments": "{}"}
            }]
        });
        let prior = vec![serde_json::json!({"role": "user", "content": "plan"})];
        let thinking = serde_json::json!({"type": "enabled"});
        let namespace = reasoning_cache_namespace(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &thinking,
            "max",
            None,
            None,
        );
        let scope = resolve_reasoning_scope(Some("cursor-thread-1"), &prior, &namespace);
        let mut assistant_with_reasoning = assistant.clone();
        assistant_with_reasoning
            .as_object_mut()
            .expect("assistant object")
            .insert(
                "reasoning_content".into(),
                serde_json::Value::String("stored chain of thought".into()),
            );
        assert!(
            store.store_assistant_message(&assistant_with_reasoning, &scope, &namespace, &prior)
                > 0
        );

        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "plan"},
                assistant,
                {"role": "user", "content": "continue with more context"}
            ],
        });
        let result = prepare_upstream_request(
            &payload,
            Some(&store),
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            Some("cursor-thread-1"),
            None,
            None,
        );
        assert_eq!(result.patched_reasoning_messages, 1);
        assert_eq!(result.recovered_reasoning_messages, 0);
        assert_eq!(result.recovery_dropped_messages, 0);
        assert!(result.recovery_notice.is_none());
        assert_eq!(result.missing_reasoning_messages, 0);
    }

    #[test]
    fn test_recover_does_not_repeat_recovery_notice_when_notice_stripped_from_inbound() {
        let payload = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "explore the repo"},
                {
                    "role": "assistant",
                    "content": RECOVERY_NOTICE_CONTENT.to_string() + "partial answer"
                },
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": "{}"}
                    }]
                },
                {"role": "user", "content": "explore the repo"}
            ],
        });
        let result = prepare_upstream_request(
            &payload,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            Some("cursor-subagent-1"),
            None,
            None,
        );
        assert!(
            result.recovery_notice.is_none(),
            "inbound notice stripped; must not inject again"
        );
    }

    #[test]
    fn test_latest_user_recover_notice_only_once_per_thread() {
        let first = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "task"},
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "list_dir", "arguments": "{}"}
                    }]
                }
            ],
        });
        let first_result = prepare_upstream_request(
            &first,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        assert!(first_result.recovery_notice.is_some());

        let mut second_messages = first
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap();
        second_messages.push(serde_json::json!({
            "role": "assistant",
            "content": RECOVERY_NOTICE_CONTENT.to_string() + "done"
        }));
        second_messages.push(serde_json::json!({"role": "user", "content": "task"}));
        let second = serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": second_messages,
        });
        let second_result = prepare_upstream_request(
            &second,
            None,
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            "enabled",
            "max",
            "recover",
            0,
            false,
            None,
            None,
            None,
            None,
        );
        assert!(second_result.recovery_notice.is_none());
    }
}
