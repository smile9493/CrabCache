//! Session and affinity helpers for raw capture (no raw message content in fingerprints).

use serde_json::Value;
use sha2::{Digest, Sha256};

/// SHA-256 hex prefix (16 chars) of `bytes`.
pub fn fingerprint_bytes(bytes: &[u8]) -> String {
    let hash = hex::encode(Sha256::digest(bytes));
    hash[..hash.len().min(16)].to_string()
}

/// Stable thread id from the first `user` message in an OpenAI-style payload.
pub fn session_fingerprint_from_payload(payload: &Value) -> Option<String> {
    let messages = payload.get("messages")?.as_array()?;
    let first_user = messages
        .iter()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))?;
    let content = first_user.get("content")?;
    match content {
        Value::String(s) => Some(fingerprint_bytes(s.as_bytes())),
        other => {
            let bytes = serde_json::to_vec(other).ok()?;
            Some(fingerprint_bytes(&bytes))
        }
    }
}

/// Ketama affinity key prefix: `conv` | `pck` | `user` | `ip` | `unknown`.
pub fn affinity_kind_from_key(key: &str) -> &str {
    key.split_once(':')
        .map(|(k, _)| k)
        .unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_fp_stable_for_same_first_user() {
        let a = json!({"messages":[{"role":"user","content":"hello"}]});
        let b = json!({"messages":[{"role":"user","content":"hello"},{"role":"assistant","content":"hi"}]});
        assert_eq!(
            session_fingerprint_from_payload(&a),
            session_fingerprint_from_payload(&b)
        );
    }

    #[test]
    fn session_fp_differs_for_different_first_user() {
        let a = json!({"messages":[{"role":"user","content":"thread-a"}]});
        let b = json!({"messages":[{"role":"user","content":"thread-b"}]});
        assert_ne!(
            session_fingerprint_from_payload(&a),
            session_fingerprint_from_payload(&b)
        );
    }

    #[test]
    fn affinity_kind_parses_prefix() {
        assert_eq!(affinity_kind_from_key("conv:abc"), "conv");
        assert_eq!(affinity_kind_from_key("pck:x"), "pck");
        assert_eq!(affinity_kind_from_key("ip:deadbeef"), "ip");
    }
}
