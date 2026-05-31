use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Custom serde module for encoding `Vec<u8>` as base64 strings instead of JSON arrays.
///
/// This reduces Redis storage size by ~2.2x for binary data (JSON arrays like `[72,101,108,...]`
/// become compact base64 strings).
mod serde_base64 {
    use base64::{Engine, engine::general_purpose};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CacheEntry {
    #[serde(with = "serde_base64")]
    pub response_body: Vec<u8>,
    pub model: String,
    pub usage: UsageInfo,
    pub created_at: u64,
    pub ttl_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "serde_base64_opt")]
    pub sse_body: Option<Vec<u8>>,
    /// Whether this entry was stored from a streaming (`stream: true`) request.
    #[serde(default)]
    pub is_stream: bool,
    /// `display_reasoning` at write time; mismatch on hit forces JSON→SSE regen.
    #[serde(default = "default_client_display_reasoning")]
    pub client_display_reasoning: bool,
}

impl CacheEntry {
    /// Returns `true` if the entry has exceeded its TTL (is stale).
    pub fn is_stale(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.created_at + self.ttl_secs
    }

    /// Seconds elapsed since the entry became stale. Returns `0` if still fresh.
    pub fn stale_age_secs(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expiry = self.created_at + self.ttl_secs;
        now.saturating_sub(expiry)
    }
    /// Approximate byte size of this cache entry (key overhead + body + SSE body + model name).
    /// Used by Moka weigher for byte-level capacity control.
    pub fn estimated_bytes(&self) -> u32 {
        let body_bytes = self.response_body.len() as u64;
        let sse_bytes = self
            .sse_body
            .as_ref()
            .map_or(0u64, |b| b.len() as u64);
        let model_bytes = self.model.len() as u64;
        let total = body_bytes + sse_bytes + model_bytes + 128; // 128B struct overhead estimate
        total.min(u32::MAX as u64) as u32
    }
}

/// Serde module for `Option<Vec<u8>>` with base64 encoding.
mod serde_base64_opt {
    use base64::{Engine, engine::general_purpose};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match bytes {
            Some(b) => s.serialize_str(&general_purpose::STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(d)?;
        match opt {
            Some(s) => {
                let bytes = general_purpose::STANDARD
                    .decode(&s)
                    .map_err(serde::de::Error::custom)?;
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }
}

fn default_client_display_reasoning() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub prompt_cache_hit_tokens: u64,
    pub prompt_cache_miss_tokens: u64,
}

#[derive(Clone, Debug)]
pub struct L0Config {
    pub max_capacity: u64,
    pub ttl_secs: u64,
    /// Per-entry byte cap for Moka weigher. When > 0, enables byte-level eviction
    /// and interprets `max_capacity` as total byte budget (not entry count).
    pub max_entry_bytes: u32,
}

impl Default for L0Config {
    fn default() -> Self {
        Self {
            max_capacity: 10_000,
            ttl_secs: 3600,
            max_entry_bytes: 0, // disabled by default (entry-count mode)
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TtlConfig {
    pub default_ttl_secs: u64,
    pub model_overrides: HashMap<String, u64>,
    pub consumer_overrides: HashMap<String, u64>,
    /// Combined overrides keyed by `"consumer:model"` for highest-precision TTL control.
    #[serde(default)]
    pub consumer_model_overrides: HashMap<String, u64>,
}

impl TtlConfig {
    pub fn new(default_ttl_secs: u64) -> Self {
        Self {
            default_ttl_secs,
            model_overrides: HashMap::new(),
            consumer_overrides: HashMap::new(),
            consumer_model_overrides: HashMap::new(),
        }
    }

    /// Resolve TTL for a given (model, consumer) pair.
    ///
    /// Priority (highest to lowest):
    /// 1. Exact `consumer:model` combination override
    /// 2. Model-only override
    /// 3. Consumer-only override
    /// 4. Default TTL
    pub fn resolve(&self, model: &str, consumer: Option<&str>) -> u64 {
        if let Some(c) = consumer {
            let combo_key = format!("{c}:{model}");
            if let Some(ttl) = self.consumer_model_overrides.get(&combo_key) {
                return *ttl;
            }
        }

        if let Some(ttl) = self.model_overrides.get(model) {
            return *ttl;
        }

        if let Some(c) = consumer {
            if let Some(ttl) = self.consumer_overrides.get(c) {
                return *ttl;
            }
        }

        self.default_ttl_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_entry_deserialize_without_client_display_reasoning_defaults_true() {
        // response_body as base64-encoded empty string (was previously [])
        let json = r#"{"response_body":"","model":"m","usage":{"prompt_tokens":0,"completion_tokens":0,"prompt_cache_hit_tokens":0,"prompt_cache_miss_tokens":0},"created_at":1,"ttl_secs":60}"#;
        let entry: CacheEntry = serde_json::from_str(json).expect("deserialize");
        assert!(entry.client_display_reasoning);
        assert!(entry.response_body.is_empty());
    }

    #[test]
    fn cache_entry_roundtrip_base64() {
        let entry = CacheEntry {
            response_body: b"Hello, World!".to_vec(),
            model: "v4-pro".to_string(),
            usage: UsageInfo::default(),
            created_at: 1000,
            ttl_secs: 3600,
            sse_body: Some(b"data: chunk\n\n".to_vec()),
            is_stream: true,
            client_display_reasoning: true,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            !json.contains("[72,101,108]"),
            "should not use JSON array for bytes"
        );
        let deserialized: CacheEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.response_body, b"Hello, World!");
        assert_eq!(deserialized.sse_body, Some(b"data: chunk\n\n".to_vec()));
    }

    #[test]
    fn test_ttl_config_resolve_default() {
        let config = TtlConfig::new(3600);
        assert_eq!(config.resolve("v4-pro", None), 3600);
    }

    #[test]
    fn test_ttl_config_resolve_model_override() {
        let mut config = TtlConfig::new(3600);
        config.model_overrides.insert("v4-pro".to_string(), 7200);
        assert_eq!(config.resolve("v4-pro", None), 7200);
        assert_eq!(config.resolve("v3", None), 3600);
    }

    #[test]
    fn test_ttl_config_resolve_consumer_override() {
        let mut config = TtlConfig::new(3600);
        config
            .consumer_overrides
            .insert("premium".to_string(), 10800);
        assert_eq!(config.resolve("v4-pro", Some("premium")), 10800);
        assert_eq!(config.resolve("v4-pro", Some("standard")), 3600);
    }

    #[test]
    fn test_ttl_config_priority() {
        let mut config = TtlConfig::new(3600);
        config.model_overrides.insert("v4-pro".to_string(), 7200);
        config
            .consumer_overrides
            .insert("premium".to_string(), 10800);

        // Without combination override: model wins over consumer
        assert_eq!(config.resolve("v4-pro", Some("premium")), 7200);
        assert_eq!(config.resolve("v4-pro", Some("standard")), 7200);
        assert_eq!(config.resolve("v3", Some("premium")), 10800);
        assert_eq!(config.resolve("v3", Some("standard")), 3600);
    }

    #[test]
    fn test_ttl_config_consumer_model_combination() {
        let mut config = TtlConfig::new(3600);
        config.model_overrides.insert("v4-pro".to_string(), 7200);
        config
            .consumer_overrides
            .insert("premium".to_string(), 10800);
        config
            .consumer_model_overrides
            .insert("premium:v4-pro".to_string(), 14400);

        // Combination override has highest priority
        assert_eq!(config.resolve("v4-pro", Some("premium")), 14400);
        // Other consumers still get model override
        assert_eq!(config.resolve("v4-pro", Some("standard")), 7200);
        // Other models still get consumer override
        assert_eq!(config.resolve("v3", Some("premium")), 10800);
        // Default fallback
        assert_eq!(config.resolve("v3", Some("standard")), 3600);
    }
}
