use hex;
use sha2::{Digest, Sha256};

/// Compute a SHA256-based fingerprint (first 16 hex chars) of arbitrary bytes.
pub fn fingerprint_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let full = hex::encode(hasher.finalize());
    if full.len() >= 16 {
        full[..16].to_string()
    } else {
        full
    }
}

/// Compute a hash fingerprint of the **immutable prefix block**: leading system
/// messages (consecutive `role == "system"` from the start) plus the `tools`
/// JSON payload, if any.
///
/// This is identical to the prior `immutable_prefix_block_hash` in
/// `crab-reasoning::normalize`. It is migrated here so that both the reasoning
/// pipeline and the composition extractor share a single implementation.
pub fn immutable_prefix_block_hash(
    messages: &[serde_json::Value],
    tools: Option<&serde_json::Value>,
) -> String {
    let mut hasher = Sha256::new();
    for msg in messages
        .iter()
        .take_while(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
    {
        if let Ok(bytes) = serde_json::to_vec(msg) {
            hasher.update(&bytes);
        }
    }
    if let Some(tools) = tools {
        if let Ok(bytes) = serde_json::to_vec(tools) {
            hasher.update(&bytes);
        }
    }
    hex::encode(hasher.finalize())
}

/// Compute a hash fingerprint of tool names only (sorted for stability).
pub fn tool_names_hash(tools: &[serde_json::Value]) -> String {
    let names: Vec<String> = tools
        .iter()
        .filter_map(|t| t.get("function"))
        .filter_map(|f| f.get("name"))
        .filter_map(|n| n.as_str())
        .map(|s| s.to_string())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    fingerprint_bytes(sorted.join(",").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_fingerprint_bytes() {
        let fp1 = fingerprint_bytes(b"hello");
        let fp2 = fingerprint_bytes(b"hello");
        let fp3 = fingerprint_bytes(b"world");
        assert_eq!(fp1, fp2);
        assert_ne!(fp1, fp3);
        assert_eq!(fp1.len(), 16);
    }

    #[test]
    fn test_immutable_prefix_block_hash_system_only() {
        let messages = vec![
            json!({"role": "system", "content": "Be helpful."}),
            json!({"role": "user", "content": "Hi"}),
        ];
        let hash = immutable_prefix_block_hash(&messages, None);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_immutable_prefix_block_hash_with_tools() {
        let messages = vec![json!({"role": "system", "content": "You are a bot."})];
        let tools = json!([{"function": {"name": "search"}}]);
        let hash = immutable_prefix_block_hash(&messages, Some(&tools));
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_tool_names_hash_sorted() {
        let tools = json!([
            {"function": {"name": "search"}},
            {"function": {"name": "compute"}},
        ]);
        let tools2 = json!([
            {"function": {"name": "compute"}},
            {"function": {"name": "search"}},
        ]);
        assert_eq!(
            tool_names_hash(tools.as_array().unwrap()),
            tool_names_hash(tools2.as_array().unwrap())
        );
    }

    #[test]
    fn test_tool_names_hash_empty() {
        let result = tool_names_hash(&[]);
        // SHA256 of empty string sorted join should produce a stable fingerprint
        assert_eq!(result.len(), 16);
    }
}
