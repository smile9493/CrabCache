use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn generate_cache_key(request_body: &[u8]) -> Result<String> {
    let mut value: Value = serde_json::from_slice(request_body)?;

    if let Some(obj) = value.as_object_mut() {
        obj.remove("stream");
        obj.remove("temperature");
        obj.remove("top_p");
        obj.remove("frequency_penalty");
        obj.remove("presence_penalty");
    }

    let normalized = serde_json::to_string(&value)?;
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let hash = hasher.finalize();
    let key = hex::encode(hash);

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn test_generate_cache_key_normalization() {
        let body1 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true,
            "temperature": 0.7
        });

        let body2 = json!({
            "model": "v4-pro",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": false,
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

        assert_ne!(key1, key2, "Different message orders should produce different keys");
    }
}
