use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CacheEntry {
    pub response_body: Vec<u8>,
    pub model: String,
    pub usage: UsageInfo,
    pub created_at: u64,
    pub ttl_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sse_body: Option<Vec<u8>>,
    /// Whether this entry was stored from a streaming (`stream: true`) request.
    #[serde(default)]
    pub is_stream: bool,
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
}

impl Default for L0Config {
    fn default() -> Self {
        Self {
            max_capacity: 10_000,
            ttl_secs: 3600,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TtlConfig {
    pub default_ttl_secs: u64,
    pub model_overrides: HashMap<String, u64>,
    pub consumer_overrides: HashMap<String, u64>,
}

impl TtlConfig {
    pub fn new(default_ttl_secs: u64) -> Self {
        Self {
            default_ttl_secs,
            model_overrides: HashMap::new(),
            consumer_overrides: HashMap::new(),
        }
    }

    pub fn resolve(&self, model: &str, consumer: Option<&str>) -> u64 {
        if let Some(consumer) = consumer
            && let Some(ttl) = self.consumer_overrides.get(consumer)
        {
            return *ttl;
        }

        if let Some(ttl) = self.model_overrides.get(model) {
            return *ttl;
        }

        self.default_ttl_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(config.resolve("v4-pro", Some("premium")), 10800);
        assert_eq!(config.resolve("v4-pro", Some("standard")), 7200);
        assert_eq!(config.resolve("v3", Some("premium")), 10800);
        assert_eq!(config.resolve("v3", Some("standard")), 3600);
    }
}
