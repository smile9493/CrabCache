use http::HeaderMap;
use sha2::{Digest, Sha256};
use tracing::trace;

pub fn extract_affinity_key(headers: &HeaderMap, client_ip: &str) -> String {
    if let Some(conv_id) = headers.get("x-conversation-id")
        && let Ok(conv_id_str) = conv_id.to_str()
    {
        let key = format!("conv:{}", conv_id_str);
        trace!(key = %key, "Using conversation ID as affinity key");
        return key;
    }

    if let Some(user_id) = headers.get("x-user-id")
        && let Ok(user_id_str) = user_id.to_str()
    {
        let key = format!("user:{}", user_id_str);
        trace!(key = %key, "Using user ID as affinity key");
        return key;
    }

    let mut hasher = Sha256::new();
    hasher.update(client_ip.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let key = format!("ip:{}", &hash[..16]);

    trace!(key = %key, client_ip = %client_ip, "Using client IP as affinity key");
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    #[test]
    fn test_conversation_id_priority() {
        let mut headers = HeaderMap::new();
        headers.insert("x-conversation-id", HeaderValue::from_static("conv-123"));
        headers.insert("x-user-id", HeaderValue::from_static("user-456"));

        let key = extract_affinity_key(&headers, "192.168.1.1");
        assert!(key.starts_with("conv:"));
        assert!(key.contains("conv-123"));
    }

    #[test]
    fn test_user_id_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", HeaderValue::from_static("user-456"));

        let key = extract_affinity_key(&headers, "192.168.1.1");
        assert!(key.starts_with("user:"));
        assert!(key.contains("user-456"));
    }

    #[test]
    fn test_ip_fallback() {
        let headers = HeaderMap::new();
        let key = extract_affinity_key(&headers, "192.168.1.1");
        assert!(key.starts_with("ip:"));
    }

    #[test]
    fn test_same_ip_same_key() {
        let headers1 = HeaderMap::new();
        let headers2 = HeaderMap::new();

        let key1 = extract_affinity_key(&headers1, "192.168.1.1");
        let key2 = extract_affinity_key(&headers2, "192.168.1.1");

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_different_ip_different_key() {
        let headers1 = HeaderMap::new();
        let headers2 = HeaderMap::new();

        let key1 = extract_affinity_key(&headers1, "192.168.1.1");
        let key2 = extract_affinity_key(&headers2, "192.168.1.2");

        assert_ne!(key1, key2);
    }
}
