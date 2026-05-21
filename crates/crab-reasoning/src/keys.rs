use serde_json::Value;
use sha2::{Digest, Sha256};

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

fn sha256_json(value: &Value) -> String {
    let canonical = serde_json::to_string(value).unwrap_or_default();
    sha256_hex(&canonical)
}

pub fn normalize_tool_call_for_signature(tool_call: &Value) -> Value {
    let mut normalized = serde_json::Map::new();
    if let Some(tc) = tool_call.as_object() {
        normalized.insert(
            "type".into(),
            tc.get("type")
                .cloned()
                .unwrap_or(Value::String("function".into())),
        );
        let mut func = serde_json::Map::new();
        if let Some(function) = tc.get("function").and_then(|f| f.as_object()) {
            func.insert(
                "name".into(),
                function
                    .get("name")
                    .cloned()
                    .unwrap_or(Value::String(String::new())),
            );
            let arguments = function
                .get("arguments")
                .map(|a| {
                    if a.is_string() {
                        a.as_str().unwrap_or("").to_string()
                    } else {
                        serde_json::to_string(a).unwrap_or_default()
                    }
                })
                .unwrap_or_default();
            func.insert("arguments".into(), Value::String(arguments));
        } else {
            func.insert("name".into(), Value::String(String::new()));
            func.insert("arguments".into(), Value::String(String::new()));
        }
        normalized.insert("function".into(), Value::Object(func));
    }
    Value::Object(normalized)
}

pub fn tool_call_signature(tool_call: &Value) -> String {
    let normalized = normalize_tool_call_for_signature(tool_call);
    let mut without_id = normalized.clone();
    if let Some(obj) = without_id.as_object_mut() {
        obj.remove("id");
    }
    sha256_json(&without_id)
}

pub fn tool_call_ids(message: &Value) -> Vec<String> {
    message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    if tc.is_object() {
                        tc.get("id")
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn tool_call_names(message: &Value) -> Vec<String> {
    message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    tc.get("function")
                        .and_then(|f| f.as_object())
                        .and_then(|obj| obj.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn message_signature(message: &Value) -> String {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "content".into(),
        message
            .get("content")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    let tool_calls: Vec<Value> = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|tc| tc.is_object())
                .map(normalize_tool_call_for_signature)
                .collect()
        })
        .unwrap_or_default();
    payload.insert("tool_calls".into(), Value::Array(tool_calls));
    sha256_json(&Value::Object(payload))
}

fn canonical_scope_message(message: &Value) -> Value {
    let mut canonical = serde_json::Map::new();
    if let Some(role) = message.get("role") {
        canonical.insert("role".into(), role.clone());
    }
    for key in &["content", "name", "tool_call_id", "prefix"] {
        if let Some(val) = message.get(key) {
            canonical.insert((*key).into(), val.clone());
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        let normalized: Vec<Value> = tool_calls
            .iter()
            .filter(|tc| tc.is_object())
            .map(normalize_tool_call_for_signature)
            .collect();
        if !normalized.is_empty() {
            canonical.insert("tool_calls".into(), Value::Array(normalized));
        }
    }
    Value::Object(canonical)
}

pub fn conversation_scope(messages: &[Value], namespace: &str) -> String {
    let scope_messages: Vec<Value> = messages.iter().map(canonical_scope_message).collect();
    let payload = if namespace.is_empty() {
        Value::Array(scope_messages)
    } else {
        let mut obj = serde_json::Map::new();
        obj.insert("namespace".into(), Value::String(namespace.to_string()));
        obj.insert("messages".into(), Value::Array(scope_messages));
        Value::Object(obj)
    };
    sha256_json(&payload)
}

/// ReasoningStore scope: prefer stable client session id (`x-conversation-id` / `prompt_cache_key`)
/// so keys survive growing message history; fall back to hashing messages when absent.
pub fn resolve_reasoning_scope(
    stable_session_id: Option<&str>,
    messages_for_scope: &[Value],
    cache_namespace: &str,
) -> String {
    if let Some(id) = stable_session_id.map(str::trim).filter(|s| !s.is_empty()) {
        if cache_namespace.is_empty() {
            format!("session:{id}")
        } else {
            let ns_tag = &sha256_hex(cache_namespace)[..16];
            format!("session:{id}:ns:{ns_tag}")
        }
    } else {
        conversation_scope(messages_for_scope, cache_namespace)
    }
}

pub fn turn_context_signature(prior_messages: &[Value]) -> String {
    let last_user_index = prior_messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));

    let start_index = if let Some(idx) = last_user_index {
        let mut si = idx;
        while si > 0 {
            if prior_messages[si - 1].get("role").and_then(|r| r.as_str()) == Some("user") {
                si -= 1;
            } else {
                break;
            }
        }
        si
    } else {
        0
    };

    let context_messages: Vec<Value> = prior_messages[start_index..]
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
        .map(canonical_scope_message)
        .collect();

    sha256_json(&Value::Array(context_messages))
}

pub fn scoped_reasoning_keys(message: &Value, scope: &str) -> Vec<String> {
    let mut keys = Vec::new();
    keys.push(format!(
        "scope:{}:signature:{}",
        scope,
        message_signature(message)
    ));
    for tc_id in tool_call_ids(message) {
        keys.push(format!("scope:{scope}:tool_call:{tc_id}"));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        for tc in tool_calls {
            if tc.is_object() {
                keys.push(format!(
                    "scope:{}:tool_call_signature:{}",
                    scope,
                    tool_call_signature(tc)
                ));
            }
        }
    }
    for name in tool_call_names(message) {
        keys.push(format!("scope:{scope}:tool_name:{name}"));
    }
    keys
}

pub fn portable_reasoning_keys(
    message: &Value,
    cache_namespace: &str,
    prior_messages: &[Value],
) -> Vec<String> {
    if cache_namespace.is_empty() {
        return Vec::new();
    }

    let turn_sig = turn_context_signature(prior_messages);
    let mut keys = Vec::new();

    keys.push(format!(
        "namespace:{}:turn:{}:signature:{}",
        cache_namespace,
        turn_sig,
        message_signature(message)
    ));

    for tc_id in tool_call_ids(message) {
        keys.push(format!(
            "namespace:{cache_namespace}:turn:{turn_sig}:tool_call:{tc_id}"
        ));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        for tc in tool_calls {
            if tc.is_object() {
                keys.push(format!(
                    "namespace:{}:turn:{}:tool_call_signature:{}",
                    cache_namespace,
                    turn_sig,
                    tool_call_signature(tc)
                ));
            }
        }
    }

    for name in tool_call_names(message) {
        keys.push(format!(
            "namespace:{cache_namespace}:turn:{turn_sig}:tool_name:{name}"
        ));
    }

    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_signature_deterministic() {
        let msg = json!({"content": "hello", "tool_calls": []});
        let sig1 = message_signature(&msg);
        let sig2 = message_signature(&msg);
        assert_eq!(sig1, sig2);
        assert_eq!(sig1.len(), 64);
    }

    #[test]
    fn test_conversation_scope_deterministic() {
        let msgs = vec![json!({"role": "user", "content": "hi"})];
        let scope1 = conversation_scope(&msgs, "ns");
        let scope2 = conversation_scope(&msgs, "ns");
        assert_eq!(scope1, scope2);
    }

    #[test]
    fn test_resolve_reasoning_scope_stable_across_growing_history() {
        let short = vec![json!({"role": "user", "content": "hi"})];
        let long = vec![
            json!({"role": "user", "content": "hi"}),
            json!({"role": "assistant", "content": "ok"}),
            json!({"role": "user", "content": "more"}),
        ];
        let legacy_short = conversation_scope(&short, "ns");
        let legacy_long = conversation_scope(&long, "ns");
        assert_ne!(legacy_short, legacy_long);

        let stable_short = resolve_reasoning_scope(Some("conv-abc"), &short, "ns");
        let stable_long = resolve_reasoning_scope(Some("conv-abc"), &long, "ns");
        assert_eq!(stable_short, stable_long);
        assert!(stable_short.starts_with("session:conv-abc:"));
    }

    #[test]
    fn test_tool_call_ids_extraction() {
        let msg = json!({
            "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "foo", "arguments": "{}"}},
                {"id": "call_2", "type": "function", "function": {"name": "bar", "arguments": "{}"}}
            ]
        });
        let ids = tool_call_ids(&msg);
        assert_eq!(ids, vec!["call_1", "call_2"]);
    }

    #[test]
    fn test_portable_keys_include_tool_call_id() {
        let prior = vec![json!({"role": "user", "content": "explore repo"})];
        let assistant = json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "list_dir", "arguments": "{}"}
            }]
        });
        let keys = portable_reasoning_keys(&assistant, "cursor-ns", &prior);
        assert!(
            keys.iter().any(|k| k.contains("tool_call:call_1")),
            "portable keys must include tool_call id for Store round-trip"
        );
    }

    #[test]
    fn test_scoped_reasoning_keys() {
        let msg = json!({
            "content": "",
            "tool_calls": [
                {"id": "tc1", "type": "function", "function": {"name": "test", "arguments": "{}"}}
            ]
        });
        let keys = scoped_reasoning_keys(&msg, "scope123");
        assert!(!keys.is_empty());
        assert!(keys[0].starts_with("scope:scope123:signature:"));
        assert!(keys.iter().any(|k| k.contains("tool_call:tc1")));
    }
}
