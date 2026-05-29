use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// Fields that are stripped from the request body before generating the cache key.
///
/// Sampling/inference parameters stripped from the cache key (does not include `stream`:
/// stream vs non-stream must not share L0/L1 entries for Cursor SSE compatibility).
const STRIPPED_FIELDS: &[&str] = &[
    "temperature",
    "top_p",
    "frequency_penalty",
    "presence_penalty",
];

/// Fields that ARE part of the fingerprint and MUST affect the cache key.
///
/// This list is maintained for documentation and testing. If any of these
/// fields are missing from the request, they do not contribute (the key is
/// computed from whatever is present after stripping STRIPPED_FIELDS).
#[allow(dead_code)]
const KEY_FIELDS: &[&str] = &[
    "model",
    "messages",
    "stream",
    "max_tokens",
    "stop",
    "response_format",
    "tools",
    "tool_choice",
    "user",
];

/// Configuration for cache key fingerprint normalization.
///
/// Controls how request bodies are normalized before hashing to produce
/// cache keys that are tolerant to semantically-irrelevant differences
/// (whitespace, line endings, Unicode normalization forms).
#[derive(Debug, Clone)]
pub struct FingerprintConfig {
    /// Version of the fingerprint normalization rules.
    /// Bump this when the normalization logic changes to invalidate old cache entries.
    pub version: u32,
    /// When true, message content strings are normalized (whitespace, line endings, NFC).
    /// Set to false to disable normalization for emergency rollback.
    pub normalize_content: bool,
}

impl FingerprintConfig {
    /// Legacy configuration: no normalization, version 0.
    /// Used by the old `generate_cache_key` / `generate_namespaced_cache_key` functions.
    pub fn legacy() -> Self {
        Self {
            version: 0,
            normalize_content: false,
        }
    }

    /// Default v1 configuration: content normalization enabled, version 1.
    pub fn default_v1() -> Self {
        Self {
            version: 1,
            normalize_content: true,
        }
    }
}

impl Default for FingerprintConfig {
    fn default() -> Self {
        Self::default_v1()
    }
}

/// Generate a cache key from a request body (legacy, delegates to fingerprint API).
///
/// Uses `FingerprintConfig::legacy()` (version=0, no normalization) for backward compatibility.
pub fn generate_cache_key(request_body: &[u8]) -> Result<String> {
    generate_cache_key_with_fingerprint(request_body, &FingerprintConfig::legacy())
}

/// Generate a cache key with an optional consumer namespace (legacy, delegates to fingerprint API).
///
/// Uses `FingerprintConfig::legacy()` (version=0, no normalization) for backward compatibility.
pub fn generate_namespaced_cache_key(
    request_body: &[u8],
    namespace: Option<&str>,
) -> Result<String> {
    generate_namespaced_cache_key_with_fingerprint(
        request_body,
        namespace,
        &FingerprintConfig::legacy(),
    )
}

/// Generate a cache key from a request body with fingerprint normalization.
///
/// 1. Parses the JSON request body
/// 2. Removes non-key fields (stream, temperature, top_p, etc.)
/// 3. Applies fingerprint normalization to message content (if enabled)
/// 4. Injects fingerprint version for safe rule upgrades
/// 5. Sorts remaining object keys for canonical serialization
/// 6. SHA-256 hashes the normalized JSON
fn fingerprint_hash_normalized_value(value: &Value) -> String {
    let mut canonical = String::new();
    canonical_write(value, &mut canonical);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

fn prepare_value_for_fingerprint(mut value: Value, config: &FingerprintConfig) -> Value {
    if let Some(obj) = value.as_object_mut() {
        for field in STRIPPED_FIELDS {
            obj.remove(*field);
        }
    }

    normalize_for_fingerprint(&mut value, config);

    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "__crab_fp_version".to_string(),
            Value::Number(serde_json::Number::from(config.version)),
        );
    }
    value
}

/// Generate a cache key from a parsed request body (avoids redundant `from_slice` in hot path).
pub fn generate_cache_key_with_fingerprint_from_value(
    value: &Value,
    config: &FingerprintConfig,
) -> Result<String> {
    Ok(fingerprint_hash_normalized_value(
        &prepare_value_for_fingerprint(value.clone(), config),
    ))
}

pub fn generate_cache_key_with_fingerprint(
    request_body: &[u8],
    config: &FingerprintConfig,
) -> Result<String> {
    let value: Value = serde_json::from_slice(request_body)?;
    generate_cache_key_with_fingerprint_from_value(&value, config)
}

/// Generate a fingerprint-normalized cache key with an optional consumer namespace.
///
/// When `namespace` is Some, it is prefixed to the hash to allow tenant isolation.
/// The resulting key is `"{namespace}:{hash}"`.
pub fn generate_namespaced_cache_key_with_fingerprint(
    request_body: &[u8],
    namespace: Option<&str>,
    config: &FingerprintConfig,
) -> Result<String> {
    let base = generate_cache_key_with_fingerprint(request_body, config)?;
    namespaced_fingerprint_key(base, namespace)
}

/// Namespaced cache key from an already-parsed client payload.
pub fn generate_namespaced_cache_key_with_fingerprint_from_value(
    value: &Value,
    namespace: Option<&str>,
    config: &FingerprintConfig,
) -> Result<String> {
    let base = generate_cache_key_with_fingerprint_from_value(value, config)?;
    namespaced_fingerprint_key(base, namespace)
}

fn namespaced_fingerprint_key(base: String, namespace: Option<&str>) -> Result<String> {
    match namespace {
        Some(ns) if !ns.is_empty() => Ok(format!("{ns}:{base}")),
        _ => Ok(base),
    }
}

/// Composite cache key with separate hashes for system prompt and tools.
///
/// This structure allows requests with the same system prompt and tools
/// but different conversations to share cache entries more effectively.
///
/// Key format: `{namespace}:{fp_version}:{system_hash}:{tools_hash}:{messages_hash}`
///
/// The system hash and tools hash are stable across requests with the same
/// configuration, improving prefix cache hit rates on upstream providers.
pub fn generate_composite_cache_key(
    request_body: &[u8],
    namespace: Option<&str>,
    config: &FingerprintConfig,
) -> Result<String> {
    let mut value: Value = serde_json::from_slice(request_body)?;

    if let Some(obj) = value.as_object_mut() {
        for field in STRIPPED_FIELDS {
            obj.remove(*field);
        }
    }

    normalize_for_fingerprint(&mut value, config);

    let system_hash = hash_system_messages(&value, config);
    let tools_hash = hash_tools(&value);
    let messages_hash = hash_conversation_messages(&value, config);

    let key = format!(
        "{}:{}:{}:{}",
        config.version, system_hash, tools_hash, messages_hash
    );

    match namespace {
        Some(ns) if !ns.is_empty() => Ok(format!("{ns}:{key}")),
        _ => Ok(key),
    }
}

/// Composite cache key from an already-parsed JSON value (avoids redundant parse).
///
/// The value is deep-cloned and mutated internally (field stripping + normalization).
/// Returns the composite key string.
///
/// **Cost note**: The deep clone is O(n) on the JSON tree size. For large payloads
/// (2–4 MiB) this adds ~1–3 ms. Only call when `prefix_aware_cache` feature is enabled.
pub fn generate_composite_cache_key_from_value(
    value: &Value,
    namespace: Option<&str>,
    config: &FingerprintConfig,
) -> String {
    let mut value = value.clone();

    if let Some(obj) = value.as_object_mut() {
        for field in STRIPPED_FIELDS {
            obj.remove(*field);
        }
    }

    normalize_for_fingerprint(&mut value, config);

    let system_hash = hash_system_messages(&value, config);
    let tools_hash = hash_tools(&value);
    let messages_hash = hash_conversation_messages(&value, config);

    let key = format!(
        "{}:{}:{}:{}",
        config.version, system_hash, tools_hash, messages_hash
    );

    match namespace {
        Some(ns) if !ns.is_empty() => format!("{ns}:{key}"),
        _ => key,
    }
}

/// Hash only system messages from the messages array.
fn hash_system_messages(value: &Value, _config: &FingerprintConfig) -> String {
    let Some(messages) = value.get("messages").and_then(|m| m.as_array()) else {
        return "none".to_string();
    };

    let system_msgs: Vec<&Value> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .collect();

    if system_msgs.is_empty() {
        return "none".to_string();
    }

    let mut canonical = String::new();
    for msg in &system_msgs {
        canonical_write(msg, &mut canonical);
    }

    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let hash = hex::encode(hasher.finalize());
    hash[..hash.len().min(12)].to_string()
}

/// Hash the tools definition (if present).
fn hash_tools(value: &Value) -> String {
    let Some(tools) = value.get("tools") else {
        return "none".to_string();
    };

    let Value::Array(tools_arr) = tools else {
        return "none".to_string();
    };

    if tools_arr.is_empty() {
        return "none".to_string();
    }

    let mut canonical = String::new();
    canonical_write(tools, &mut canonical);

    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let hash = hex::encode(hasher.finalize());
    hash[..hash.len().min(12)].to_string()
}

/// Hash non-system messages (user + assistant + tool messages).
fn hash_conversation_messages(value: &Value, _config: &FingerprintConfig) -> String {
    let Some(messages) = value.get("messages").and_then(|m| m.as_array()) else {
        return "empty".to_string();
    };

    let non_system: Vec<&Value> = messages
        .iter()
        .filter(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"))
        .collect();

    if non_system.is_empty() {
        return "empty".to_string();
    }

    let mut canonical = String::new();
    for msg in &non_system {
        canonical_write(msg, &mut canonical);
    }

    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

/// Apply fingerprint normalization to a parsed JSON request body.
///
/// Normalizes message content strings to reduce semantically-irrelevant differences:
/// - `\r\n` → `\n` (Windows line endings)
/// - Trim leading/trailing ASCII whitespace
/// - Unicode NFC normalization
///
/// Processes the `messages` array in-place without changing message count or order.
/// Multimodal content arrays (parts) have their `text` / `input_text` fields normalized.
fn normalize_for_fingerprint(value: &mut Value, config: &FingerprintConfig) {
    if !config.normalize_content {
        return;
    }

    let Some(obj) = value.as_object_mut() else {
        return;
    };

    let Some(messages) = obj.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };

    for msg in messages {
        let Some(msg_obj) = msg.as_object_mut() else {
            continue;
        };

        match msg_obj.get_mut("content") {
            Some(Value::String(s)) => {
                *s = normalize_content_string(s);
            }
            Some(Value::Array(parts)) => {
                for part in parts {
                    let Some(part_obj) = part.as_object_mut() else {
                        continue;
                    };
                    for field in &["text", "input_text"] {
                        if let Some(Value::String(s)) = part_obj.get_mut(*field) {
                            *s = normalize_content_string(s);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Normalize a single content string for fingerprinting:
/// - `\r\n` → `\n`
/// - Trim leading/trailing ASCII whitespace
/// - Unicode NFC normalization
fn normalize_content_string(s: &str) -> String {
    let normalized = s.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    trimmed.nfc().collect::<String>()
}

/// Recursively serialize a JSON value with sorted object keys for canonical output.
///
/// Writes directly into the provided `String` buffer to avoid intermediate allocations.
fn canonical_write(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // Key: use serde_json escaping for correctness (handles special chars)
                out.push('"');
                // serde_json keys are already valid JSON strings; write the raw key
                // and let the serializer handle escaping via to_writer if needed.
                // For simplicity, use serde_json::to_string for the key part.
                let key_json =
                    serde_json::to_string(k.as_str()).unwrap_or_else(|_| format!("\"{k}\""));
                out.push_str(&key_json[1..key_json.len() - 1]); // strip outer quotes
                out.push_str("\":");
                canonical_write(&map[*k], out);
            }
            out.push('}');
        }
        Value::Array(arr) => {
            out.push('[');
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical_write(v, out);
            }
            out.push(']');
        }
        Value::String(s) => {
            // Use serde_json for proper JSON string escaping
            let escaped = serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""));
            out.push_str(&escaped);
        }
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Legacy (delegation) tests ──────────────────────────────────────────

    #[test]
    fn test_generate_cache_key_basic() {
        let body = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let key = generate_cache_key(&body_bytes).unwrap();
        assert!(!key.is_empty());
        assert_eq!(key.len(), 64);
    }

    #[test]
    fn test_generate_cache_key_stream_mode_differs() {
        let body_stream = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true,
            "temperature": 0.7
        });

        let body_non_stream = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": false,
            "temperature": 0.9
        });

        let key_stream = generate_cache_key(&serde_json::to_vec(&body_stream).unwrap()).unwrap();
        let key_non_stream =
            generate_cache_key(&serde_json::to_vec(&body_non_stream).unwrap()).unwrap();

        assert_ne!(
            key_stream, key_non_stream,
            "stream true/false must not share the same cache key (Cursor SSE vs JSON)"
        );
    }

    #[test]
    fn test_generate_cache_key_sampling_params_stripped() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true,
            "temperature": 0.7
        });

        let body2 = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true,
            "temperature": 0.9
        });

        let key1 = generate_cache_key(&serde_json::to_vec(&body1).unwrap()).unwrap();
        let key2 = generate_cache_key(&serde_json::to_vec(&body2).unwrap()).unwrap();

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_generate_cache_key_different_content() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "World"}
            ]
        });

        let key1 = generate_cache_key(&serde_json::to_vec(&body1).unwrap()).unwrap();
        let key2 = generate_cache_key(&serde_json::to_vec(&body2).unwrap()).unwrap();

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_generate_cache_key_message_order_matters() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"}
            ]
        });

        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "assistant", "content": "Hi"},
                {"role": "user", "content": "Hello"}
            ]
        });

        let key1 = generate_cache_key(&serde_json::to_vec(&body1).unwrap()).unwrap();
        let key2 = generate_cache_key(&serde_json::to_vec(&body2).unwrap()).unwrap();

        assert_ne!(
            key1, key2,
            "Different message orders should produce different keys"
        );
    }

    #[test]
    fn test_canonical_json_key_order_independent() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let body2 = json!({
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "model": "v4-pro"
        });

        let key1 = generate_cache_key(&serde_json::to_vec(&body1).unwrap()).unwrap();
        let key2 = generate_cache_key(&serde_json::to_vec(&body2).unwrap()).unwrap();

        assert_eq!(
            key1, key2,
            "Different key orders should produce same key with canonical JSON"
        );
    }

    #[test]
    fn test_canonical_json_nested_key_order() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello", "name": "test"}
            ]
        });

        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"name": "test", "content": "Hello", "role": "user"}
            ]
        });

        let key1 = generate_cache_key(&serde_json::to_vec(&body1).unwrap()).unwrap();
        let key2 = generate_cache_key(&serde_json::to_vec(&body2).unwrap()).unwrap();

        assert_eq!(
            key1, key2,
            "Different nested key orders should produce same key"
        );
    }

    #[test]
    fn test_stripped_fields_do_not_affect_key() {
        let test_cases = [
            ("temperature", json!(0.7)),
            ("temperature", json!(1.0)),
            ("top_p", json!(0.9)),
            ("frequency_penalty", json!(0.5)),
            ("presence_penalty", json!(0.5)),
        ];

        let base = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let base_key = generate_cache_key(&serde_json::to_vec(&base).unwrap()).unwrap();

        for (field, value) in &test_cases {
            let mut body = base.clone();
            if let Some(obj) = body.as_object_mut() {
                obj.insert(field.to_string(), value.clone());
            }
            let key = generate_cache_key(&serde_json::to_vec(&body).unwrap()).unwrap();
            assert_eq!(
                key, base_key,
                "Field '{field}' should be stripped and not affect key"
            );
        }
    }

    #[test]
    fn test_key_fields_affect_key() {
        let base = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let base_key = generate_cache_key(&serde_json::to_vec(&base).unwrap()).unwrap();

        let diff_model = json!({
            "model": "v4-flash",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let diff_key = generate_cache_key(&serde_json::to_vec(&diff_model).unwrap()).unwrap();
        assert_ne!(
            diff_key, base_key,
            "Different model should produce different key"
        );
    }

    #[test]
    fn test_generate_namespaced_cache_key() {
        let body = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let key_no_ns = generate_namespaced_cache_key(&body_bytes, None).unwrap();
        let key_with_ns = generate_namespaced_cache_key(&body_bytes, Some("tenant-a")).unwrap();

        assert_eq!(key_no_ns.len(), 64);
        assert!(key_with_ns.starts_with("tenant-a:"));
        assert_eq!(key_with_ns.len(), key_no_ns.len() + 9); // "tenant-a:" = 9 chars
    }

    #[test]
    fn test_canonical_to_string_consistency() {
        let obj1: Value = json!({"b": 2, "a": 1, "c": {"z": 9, "y": 8}});
        let obj2: Value = json!({"c": {"y": 8, "z": 9}, "a": 1, "b": 2});

        let mut s1 = String::new();
        canonical_write(&obj1, &mut s1);
        let mut s2 = String::new();
        canonical_write(&obj2, &mut s2);
        assert_eq!(
            s1, s2,
            "Canonical representation should be key-order independent"
        );
    }

    // ── Fingerprint tests ──────────────────────────────────────────────────

    #[test]
    fn test_fingerprint_whitespace_difference_same_key() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "  Hello world  "}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello world"}
            ]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_eq!(
            key1, key2,
            "Whitespace differences should produce same key with fingerprint normalization"
        );
    }

    #[test]
    fn test_fingerprint_crlf_normalization_same_key() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "line1\r\nline2"}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "line1\nline2"}
            ]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_eq!(key1, key2, "\\r\\n vs \\n should produce same key");
    }

    #[test]
    fn test_fingerprint_different_system_different_key() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "system", "content": "You are dangerous."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_ne!(
            key1, key2,
            "Different system text must produce different keys"
        );
    }

    #[test]
    fn test_fingerprint_version_changes_key() {
        let body = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let v1 = FingerprintConfig {
            version: 1,
            normalize_content: true,
        };
        let v2 = FingerprintConfig {
            version: 2,
            normalize_content: true,
        };

        let key1 = generate_cache_key_with_fingerprint(&body_bytes, &v1).unwrap();
        let key2 = generate_cache_key_with_fingerprint(&body_bytes, &v2).unwrap();

        assert_ne!(
            key1, key2,
            "Different fingerprint versions must produce different keys"
        );
    }

    #[test]
    fn test_fingerprint_normalize_content_false_preserves_whitespace_difference() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "  Hello  "}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let config_no_norm = FingerprintConfig {
            version: 1,
            normalize_content: false,
        };
        let config_norm = FingerprintConfig {
            version: 1,
            normalize_content: true,
        };

        // Without normalization, whitespace should produce different keys
        let key1 = generate_cache_key_with_fingerprint(
            &serde_json::to_vec(&body1).unwrap(),
            &config_no_norm,
        )
        .unwrap();
        let key2 = generate_cache_key_with_fingerprint(
            &serde_json::to_vec(&body2).unwrap(),
            &config_no_norm,
        )
        .unwrap();
        assert_ne!(
            key1, key2,
            "normalize_content=false should preserve whitespace differences"
        );

        // With normalization, same content should produce same key
        let key3 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config_norm)
                .unwrap();
        let key4 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config_norm)
                .unwrap();
        assert_eq!(
            key3, key4,
            "normalize_content=true should collapse whitespace differences"
        );
    }

    #[test]
    fn test_cache_key_from_value_matches_bytes() {
        let body = json!({
            "model": "mimo-v2.5-pro",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": true,
            "temperature": 0.7
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let config = FingerprintConfig::default_v1();
        let from_bytes = generate_cache_key_with_fingerprint(&bytes, &config).unwrap();
        let from_value = generate_cache_key_with_fingerprint_from_value(&body, &config).unwrap();
        assert_eq!(from_bytes, from_value);
    }

    fn test_fingerprint_multimodal_content_normalization() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "  Hello world  "}
                ]}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Hello world"}
                ]}
            ]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_eq!(key1, key2, "Multimodal content text should be normalized");
    }

    #[test]
    fn test_fingerprint_legacy_vs_v1() {
        let body = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "  Hello  "}
            ]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let legacy_key =
            generate_cache_key_with_fingerprint(&body_bytes, &FingerprintConfig::legacy()).unwrap();
        let v1_key =
            generate_cache_key_with_fingerprint(&body_bytes, &FingerprintConfig::default_v1())
                .unwrap();

        // Legacy (no normalize, version 0) vs v1 (normalize, version 1) should differ
        // because: different version numbers AND different content (trimmed vs not)
        assert_ne!(
            legacy_key, v1_key,
            "Legacy and v1 fingerprints should differ"
        );
    }

    #[test]
    fn test_fingerprint_message_order_still_matters() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi"}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "assistant", "content": "Hi"},
                {"role": "user", "content": "Hello"}
            ]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_ne!(
            key1, key2,
            "Message order must still affect key with fingerprint"
        );
    }

    #[test]
    fn test_fingerprint_different_model_different_key() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body2 = json!({
            "model": "v4-flash",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_ne!(key1, key2, "Different models must produce different keys");
    }

    #[test]
    fn test_fingerprint_namespaced() {
        let body = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let config = FingerprintConfig::default_v1();
        let key_no_ns =
            generate_namespaced_cache_key_with_fingerprint(&body_bytes, None, &config).unwrap();
        let key_with_ns =
            generate_namespaced_cache_key_with_fingerprint(&body_bytes, Some("tenant-a"), &config)
                .unwrap();

        assert_eq!(key_no_ns.len(), 64);
        assert!(key_with_ns.starts_with("tenant-a:"));
    }

    #[test]
    fn test_fingerprint_unicode_nfc_normalization() {
        // U+00E9 (é precomposed) vs U+0065 U+0301 (e + combining acute)
        let e_precomposed = "\u{00E9}"; // é as single codepoint (NFC)
        let e_decomposed = "e\u{0301}"; // e + combining acute (NFD)

        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": format!("caf{}", e_precomposed)}
            ]
        });
        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": format!("caf{}", e_decomposed)}
            ]
        });

        let config = FingerprintConfig::default_v1();
        let key1 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body1).unwrap(), &config)
                .unwrap();
        let key2 =
            generate_cache_key_with_fingerprint(&serde_json::to_vec(&body2).unwrap(), &config)
                .unwrap();

        assert_eq!(
            key1, key2,
            "NFC normalization should unify precomposed and decomposed forms"
        );
    }

    #[test]
    fn test_fingerprint_legacy_delegation_equivalent() {
        // Verify that the old API produces the same key as the new API with legacy config
        let body = json!({
            "model": "v4-pro",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let old_key = generate_cache_key(&body_bytes).unwrap();
        let new_key =
            generate_cache_key_with_fingerprint(&body_bytes, &FingerprintConfig::legacy()).unwrap();

        assert_eq!(
            old_key, new_key,
            "Old API must delegate correctly to new API"
        );
    }
}
