use crate::keys::{conversation_scope, message_signature, tool_call_ids, tool_call_names, tool_call_signature};
use crate::store::ReasoningStore;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static CURSOR_THINKING_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:<(?:think|thinking)\b[^>]*>[\s\S]*?(?:</(?:think|thinking)>|$)|<details\b[^>]*>\s*<summary\b[^>]*>\s*Thinking\s*</summary>[\s\S]*?(?:</details>|$))\s*").unwrap()
});

const SUPPORTED_REQUEST_FIELDS: &[&str] = &[
    "model", "messages", "stream", "stream_options", "max_tokens",
    "response_format", "stop", "tools", "tool_choice", "thinking",
    "reasoning_effort", "temperature", "top_p", "presence_penalty",
    "frequency_penalty", "logprobs", "top_logprobs", "user", "seed", "n",
    "logit_bias",
];

const MESSAGE_FIELDS: &[&str] = &[
    "role", "content", "name", "tool_call_id", "tool_calls",
    "reasoning_content", "prefix",
];

const ROLE_MESSAGE_FIELDS: &[(&str, &[&str])] = &[
    ("system", &["role", "content", "name"]),
    ("user", &["role", "content", "name"]),
    ("assistant", &["role", "content", "name", "tool_calls", "reasoning_content", "prefix"]),
    ("tool", &["role", "content", "tool_call_id"]),
];

const EFFORT_ALIASES: &[(&str, &str)] = &[
    ("low", "high"),
    ("medium", "high"),
    ("high", "high"),
    ("max", "max"),
    ("xhigh", "max"),
];

pub const RECOVERY_NOTICE_TEXT: &str = "[deepseek-cursor-proxy] Refreshed reasoning_content history.";
pub const RECOVERY_NOTICE_CONTENT: &str = "[deepseek-cursor-proxy] Refreshed reasoning_content history.\n\n";
pub const RECOVERY_SYSTEM_CONTENT: &str = "deepseek-cursor-proxy recovered this request because older DeepSeek thinking-mode tool-call reasoning_content was unavailable. Older unrecoverable tool-call history was omitted; continue using only the remaining recovered context.";

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
                        if item_type == "text" || item_type == "input_text" {
                            parts.push(text.to_string());
                        } else if !text.is_empty() {
                            parts.push(text.to_string());
                        }
                    }
                    other => parts.push(other.to_string()),
                }
            }
            let joined: String = parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n");
            if joined.is_empty() { None } else { Some(joined) }
        }
        Value::Object(_) | Value::Number(_) | Value::Bool(_) => {
            Some(serde_json::to_string(content).unwrap_or_default())
        }
    }
}

pub fn strip_cursor_thinking_blocks(content: &str) -> String {
    let result = CURSOR_THINKING_BLOCK_RE.replace_all(content, "").to_string();
    result.trim_start_matches(['\r', '\n']).to_string()
}

fn normalize_tool_call(tool_call: &Value) -> Value {
    let tc = tool_call.as_object().cloned().unwrap_or_default();
    let function = tc.get("function").and_then(|f| f.as_object()).cloned();
    let func_obj = if let Some(func) = function {
        let arguments = func.get("arguments").map(|a| {
            if a.is_string() { a.as_str().unwrap_or("").to_string() }
            else { serde_json::to_string(a).unwrap_or_default() }
        }).unwrap_or_default();
        let mut m = serde_json::Map::new();
        m.insert("name".into(), Value::String(func.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string()));
        m.insert("arguments".into(), Value::String(arguments));
        m
    } else {
        let mut m = serde_json::Map::new();
        m.insert("name".into(), Value::String(String::new()));
        m.insert("arguments".into(), Value::String(String::new()));
        m
    };

    let mut normalized = serde_json::Map::new();
    let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !id.is_empty() {
        normalized.insert("id".into(), Value::String(id));
    }
    normalized.insert("type".into(), tc.get("type").cloned().unwrap_or(Value::String("function".into())));
    normalized.insert("function".into(), Value::Object(func_obj));
    Value::Object(normalized)
}

fn normalize_tool(tool: &Value) -> Value {
    let mut normalized = tool.as_object().cloned().unwrap_or_default();
    normalized.insert("type".into(), normalized.get("type").cloned().unwrap_or(Value::String("function".into())));
    Value::Object(normalized)
}

fn legacy_function_to_tool(function: &Value) -> Value {
    let func = function.as_object().cloned().unwrap_or_default();
    let mut m = serde_json::Map::new();
    m.insert("type".into(), Value::String("function".into()));
    m.insert("function".into(), Value::Object(func));
    Value::Object(m)
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
            if obj.get("type").and_then(|t| t.as_str()) == Some("function") {
                if let Some(func) = obj.get("function").and_then(|f| f.as_object()) {
                    if func.get("name").is_some() {
                        let mut m = serde_json::Map::new();
                        m.insert("type".into(), Value::String("function".into()));
                        let mut fm = serde_json::Map::new();
                        fm.insert("name".into(), func.get("name").cloned().unwrap_or(Value::Null));
                        m.insert("function".into(), Value::Object(fm));
                        return Some(Value::Object(m));
                    }
                }
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
                fm.insert("name".into(), obj.get("name").cloned().unwrap_or(Value::Null));
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
    if message.get("tool_calls").and_then(|tc| tc.as_array()).map(|a| !a.is_empty()).unwrap_or(false) {
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

fn reasoning_lookup_keys(
    message: &Value,
    scope: &str,
    cache_namespace: &str,
    prior_messages: &[Value],
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

    for tc in message.get("tool_calls").and_then(|tcs| tcs.as_array()).unwrap_or(&Vec::new()) {
        if tc.is_object() {
            let func_name = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("");
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
        for tc in message.get("tool_calls").and_then(|tcs| tcs.as_array()).unwrap_or(&Vec::new()) {
            if tc.is_object() {
                let func_name = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("");
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

    keys
}

struct NormalizeResult {
    message: Value,
    patched: bool,
    missing: bool,
}

fn normalize_message(
    message: &Value,
    store: Option<&ReasoningStore>,
    prior_messages: &[Value],
    cache_namespace: &str,
    repair_reasoning: bool,
    keep_reasoning: bool,
) -> NormalizeResult {
    let mut msg = message.as_object().cloned().unwrap_or_default();
    msg.retain(|k, _| MESSAGE_FIELDS.contains(&k.as_str()));

    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string();
    msg.insert("role".into(), Value::String(role.clone()));

    let role_str = if role == "function" { "tool" } else { &role };
    if role == "function" {
        msg.insert("role".into(), Value::String("tool".into()));
    }

    if msg.contains_key("content") {
        let content = msg.get("content").cloned().unwrap_or(Value::Null);
        msg.insert("content".into(), extract_text_content(&content).map(Value::String).unwrap_or(Value::String(String::new())));
    } else if ["assistant", "tool", "system", "user"].contains(&role_str) {
        msg.insert("content".into(), Value::String(String::new()));
    }

    if role_str == "assistant" {
        if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
            let stripped = strip_cursor_thinking_blocks(content);
            msg.insert("content".into(), Value::String(stripped));
        }
    }

    if let Some(tool_calls) = msg.get("tool_calls").and_then(|tc| tc.as_array()).cloned() {
        msg.insert("tool_calls".into(), Value::Array(tool_calls.iter().map(|tc| normalize_tool_call(tc)).collect()));
    }

    let mut patched = false;
    let mut missing = false;

    if role_str == "assistant" {
        if !keep_reasoning {
            msg.remove("reasoning_content");
        } else if repair_reasoning {
            let has_reasoning = msg.get("reasoning_content").and_then(|r| r.as_str()).is_some();
            if !has_reasoning {
                msg.remove("reasoning_content");
                let needs_reasoning = assistant_needs_reasoning_for_tool_context(&Value::Object(msg.clone()), prior_messages);
                if needs_reasoning {
                    let lookup_scope = conversation_scope(prior_messages, cache_namespace);
                    let lookup_keys = reasoning_lookup_keys(&Value::Object(msg.clone()), &lookup_scope, cache_namespace, prior_messages);
                    if let Some(store) = store {
                        for lookup_key in &lookup_keys {
                            if let Some(key_str) = lookup_key.get("key").and_then(|k| k.as_str()) {
                                if let Some(restored) = store.get(key_str) {
                                    msg.insert("reasoning_content".into(), Value::String(restored));
                                    patched = true;
                                    break;
                                }
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
    store: Option<&ReasoningStore>,
    cache_namespace: &str,
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
        && message.get("content").and_then(|c| c.as_str()).map(|s| s.starts_with(RECOVERY_NOTICE_TEXT)).unwrap_or(false)
}

fn strip_recovery_notice_for_upstream(messages: &[Value]) -> Vec<Value> {
    messages.iter().map(|msg| {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            return msg.clone();
        }
        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if !content.starts_with(RECOVERY_NOTICE_TEXT) {
            return msg.clone();
        }
        let mut cleaned = msg.clone();
        if let Some(obj) = cleaned.as_object_mut() {
            let remaining = content[RECOVERY_NOTICE_TEXT.len()..].trim_start_matches(['\r', '\n']);
            obj.insert("content".into(), Value::String(remaining.to_string()));
        }
        cleaned
    }).collect()
}

fn leading_system_messages(messages: &[Value]) -> Vec<Value> {
    messages.iter().take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("system")).cloned().collect()
}

fn active_messages_from_recovery_boundary(messages: &[Value]) -> Option<(Vec<Value>, usize, serde_json::Value)> {
    let recovery_boundary_index = messages.iter().rposition(|m| has_recovery_notice(m))?;

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

    Some((active, retired, serde_json::json!({
        "strategy": "continued_recovery_boundary",
        "recovery_boundary_index": recovery_boundary_index,
        "context_user_index": context_user_index,
        "retired_prefix_messages": retired,
    })))
}

fn recover_messages_from_missing_reasoning(
    messages: &[Value],
    missing_indexes: &[usize],
) -> (Vec<Value>, usize, Option<String>, serde_json::Value) {
    let recovery_boundary_index = messages.iter().rposition(|m| {
        has_recovery_notice(m) && missing_indexes.iter().any(|&idx| idx < messages.len() && messages.get(idx).map(|mi| has_recovery_notice(mi)).unwrap_or(false) == false)
    });

    if let Some(rbi) = recovery_boundary_index {
        let context_user_index = messages[..rbi].iter().rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));
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

        return (recovered, omitted, None, serde_json::json!({
            "strategy": "recovery_boundary",
            "missing_indexes": missing_indexes,
            "recovery_boundary_index": rbi,
            "dropped_messages": omitted,
        }));
    }

    let last_user_index = messages.iter().rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

    if let Some(lui) = last_user_index {
        let mut recovered = leading_system_messages(messages);
        let omitted = messages.len() - recovered.len() - 1;
        recovered.push(serde_json::json!({"role": "system", "content": RECOVERY_SYSTEM_CONTENT}));
        recovered.push(messages[lui].clone());
        return (recovered, omitted, Some(RECOVERY_NOTICE_CONTENT.to_string()), serde_json::json!({
            "strategy": "latest_user",
            "missing_indexes": missing_indexes,
            "last_user_index": lui,
            "dropped_messages": omitted,
        }));
    }

    (messages.to_vec(), 0, None, serde_json::json!({"strategy": "none", "missing_indexes": missing_indexes}))
}

pub fn reasoning_cache_namespace(
    upstream_base_url: &str,
    upstream_model: &str,
    thinking: &Value,
    reasoning_effort: &str,
    authorization: Option<&str>,
) -> String {
    use sha2::{Digest, Sha256};
    let auth_hash = authorization.map(|a| {
        let mut hasher = Sha256::new();
        hasher.update(a.as_bytes());
        hex::encode(hasher.finalize())
    }).unwrap_or_default();

    let payload = serde_json::json!({
        "base_url": upstream_base_url,
        "model": reasoning_model_family(upstream_model),
        "thinking": thinking,
        "reasoning_effort": reasoning_effort,
        "authorization_hash": auth_hash,
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

pub fn upstream_model_for(original_model: &str, fallback_model: &str) -> String {
    if original_model.starts_with("deepseek-") {
        original_model.to_string()
    } else {
        fallback_model.to_string()
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
    pub recovery_notice: Option<String>,
    pub record_response_scope: String,
    pub record_response_messages: Vec<Value>,
    pub record_response_contexts: Vec<(String, Vec<Value>)>,
}

pub fn prepare_upstream_request(
    payload: &Value,
    store: Option<&ReasoningStore>,
    upstream_base_url: &str,
    fallback_model: &str,
    thinking_mode: &str,
    reasoning_effort: &str,
    missing_reasoning_strategy: &str,
    authorization: Option<&str>,
) -> PreparedRequest {
    let original_model = payload.get("model").and_then(|m| m.as_str()).unwrap_or(fallback_model).to_string();
    let upstream_model = upstream_model_for(&original_model, fallback_model);

    let supported_set: std::collections::HashSet<&str> = SUPPORTED_REQUEST_FIELDS.iter().copied().collect();
    let mut prepared: serde_json::Map<String, Value> = payload
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, _)| supported_set.contains(k.as_str()))
        .collect();

    if !prepared.contains_key("max_tokens") {
        if let Some(mct) = payload.get("max_completion_tokens") {
            prepared.insert("max_tokens".into(), mct.clone());
        }
    }

    prepared.insert("model".into(), Value::String(upstream_model.clone()));

    if prepared.get("stream").and_then(|s| s.as_bool()).unwrap_or(false) {
        let stream_options = prepared.get("stream_options").cloned().unwrap_or(Value::Object(serde_json::Map::new()));
        let mut so = stream_options.as_object().cloned().unwrap_or_default();
        so.insert("include_usage".into(), Value::Bool(true));
        prepared.insert("stream_options".into(), Value::Object(so));
    }

    if let Some(tools) = prepared.get("tools").and_then(|t| t.as_array()).cloned() {
        prepared.insert("tools".into(), Value::Array(tools.iter().map(|t| normalize_tool(t)).collect()));
    } else if let Some(functions) = payload.get("functions").and_then(|f| f.as_array()).cloned() {
        prepared.insert("tools".into(), Value::Array(functions.iter().map(|f| legacy_function_to_tool(f)).collect()));
    }

    if let Some(tool_choice) = prepared.get("tool_choice").cloned() {
        if let Some(normalized) = normalize_tool_choice(&tool_choice) {
            prepared.insert("tool_choice".into(), normalized);
        } else {
            prepared.remove("tool_choice");
        }
    } else if let Some(function_call) = payload.get("function_call").cloned() {
        if let Some(converted) = convert_function_call(&function_call) {
            prepared.insert("tool_choice".into(), converted);
        }
    }

    let thinking_enabled = thinking_mode == "enabled";
    let thinking_disabled = thinking_mode == "disabled";
    let mut thinking_obj = serde_json::Map::new();
    thinking_obj.insert("type".into(), Value::String(thinking_mode.to_string()));
    prepared.insert("thinking".into(), Value::Object(thinking_obj));

    if thinking_enabled {
        let effort = payload.get("reasoning_effort").and_then(|e| e.as_str()).unwrap_or(reasoning_effort);
        prepared.insert("reasoning_effort".into(), Value::String(normalize_reasoning_effort(effort)));
    }

    let cache_namespace = reasoning_cache_namespace(
        upstream_base_url,
        &upstream_model,
        prepared.get("thinking").unwrap_or(&Value::Null),
        prepared.get("reasoning_effort").and_then(|e| e.as_str()).unwrap_or(reasoning_effort),
        authorization,
    );

    let pre_repair = normalize_messages(
        payload.get("messages").and_then(|m| m.as_array()).map(|a| a.as_slice()).unwrap_or(&[]),
        None,
        &cache_namespace,
        false,
        !thinking_disabled,
    );
    let record_response_messages = pre_repair.messages.clone();
    let record_response_scope = conversation_scope(&record_response_messages, &cache_namespace);

    let mut messages_for_repair = pre_repair.messages.clone();
    let mut retired_prefix_messages = 0;
    let mut recovered_count = 0;
    let mut recovery_dropped_messages = 0;
    let mut recovery_notice = None;

    if thinking_enabled && missing_reasoning_strategy == "recover" {
        if let Some((active, retired, _step)) = active_messages_from_recovery_boundary(&pre_repair.messages) {
            messages_for_repair = active;
            retired_prefix_messages = retired;
        }
    }

    let mut result = normalize_messages(
        &messages_for_repair,
        store,
        &cache_namespace,
        thinking_enabled,
        !thinking_disabled,
    );

    let mut missing_indexes = result.missing_indexes;
    while !missing_indexes.is_empty() && missing_reasoning_strategy == "recover" {
        let (recovered, dropped, notice, _step) = recover_messages_from_missing_reasoning(&result.messages, &missing_indexes);
        if dropped == 0 {
            break;
        }
        recovered_count += missing_indexes.len();
        recovery_dropped_messages += dropped;
        if notice.is_some() {
            recovery_notice = notice;
        }
        result = normalize_messages(&recovered, store, &cache_namespace, thinking_enabled, !thinking_disabled);
        missing_indexes = result.missing_indexes;
    }

    let active_scope = conversation_scope(&result.messages, &cache_namespace);
    let mut record_response_contexts = Vec::new();
    record_response_contexts.push((record_response_scope.clone(), record_response_messages.clone()));
    if active_scope != record_response_scope {
        record_response_contexts.push((active_scope, result.messages.clone()));
    }

    let final_messages = strip_recovery_notice_for_upstream(&result.messages);
    prepared.insert("messages".into(), Value::Array(final_messages));

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
        recovery_notice,
        record_response_scope,
        record_response_messages,
        record_response_contexts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_reasoning_effort() {
        assert_eq!(normalize_reasoning_effort("low"), "high");
        assert_eq!(normalize_reasoning_effort("max"), "max");
        assert_eq!(normalize_reasoning_effort("xhigh"), "max");
        assert_eq!(normalize_reasoning_effort("medium"), "high");
    }

    #[test]
    fn test_extract_text_content() {
        assert_eq!(extract_text_content(&Value::String("hello".into())), Some("hello".to_string()));
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
    fn test_upstream_model_for() {
        assert_eq!(upstream_model_for("deepseek-v4-pro", "fallback"), "deepseek-v4-pro");
        assert_eq!(upstream_model_for("gpt-4", "deepseek-v4-pro"), "deepseek-v4-pro");
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
            None,
        );
        assert_eq!(result.original_model, "deepseek-v4-pro");
        assert_eq!(result.upstream_model, "deepseek-v4-pro");
        assert_eq!(result.missing_reasoning_messages, 0);
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
            None,
        );
        let tools = result.payload.get("tools").and_then(|t| t.as_array()).unwrap();
        assert_eq!(tools.len(), 1);
        assert!(result.payload.get("tool_choice").is_some());
    }
}
