use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;

pub use crab_proxy::ConnectionConfig;

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    pub listen_addr: String,
    pub metrics_addr: String,
    pub api_key: String,
    pub upstream: UpstreamConfig,
    pub cache: CacheConfig,
    pub semantic: SemanticConfig,
    pub connection: Option<ConnectionConfig>,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamConfig {
    pub deepseek_endpoints: Vec<String>,
    pub default_weight: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CacheConfig {
    pub l0_max_capacity: Option<u64>,
    pub l0_ttl_secs: Option<u64>,
    pub l1_redis_url: String,
    pub l1_pool_size: Option<u32>,
    pub default_ttl_secs: Option<u64>,
    pub model_ttl_overrides: Option<HashMap<String, u64>>,
    pub consumer_ttl_overrides: Option<HashMap<String, u64>>,
}

#[derive(Debug, Deserialize)]
pub struct SemanticConfig {
    pub enabled: bool,
    pub model_path: String,
    pub tokenizer_path: String,
    pub qdrant_url: String,
    pub collection_name: String,
    pub vector_size: Option<u64>,
    pub similarity_threshold: Option<f32>,
    pub ttl_secs: Option<u64>,
}

impl GatewayConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn parse_endpoints(&self) -> Vec<crab_route::Backend> {
        self.upstream
            .deepseek_endpoints
            .iter()
            .enumerate()
            .map(|(i, endpoint)| {
                let addr: SocketAddr = endpoint
                    .parse()
                    .unwrap_or_else(|_| {
                        panic!("Invalid endpoint address: {}", endpoint)
                    });
                crab_route::Backend::new(
                    format!("backend-{}", i + 1),
                    addr,
                    self.upstream.default_weight.unwrap_or(1),
                    "api.deepseek.com".to_string(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_load_missing_file() {
        let result = GatewayConfig::load("/nonexistent/config.toml");
        assert!(result.is_err());
    }
}
