//! Shared test helpers for gateway integration tests.

use crab_cache::{FingerprintConfig, TtlConfig};
use crab_pipeline::{PipelineGlobals, UpstreamProvider};
use crab_proxy::{ConnectionConfig, RuntimeConfig, UpstreamKeyPool, UpstreamProfileRuntime};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Create a minimal `RuntimeConfig` for integration tests.
pub fn test_runtime() -> Arc<RuntimeConfig> {
    let backends = crab_control::parse_backend_endpoints(
        &["127.0.0.1:443".to_string()],
        1,
        "api.deepseek.com",
    )
    .unwrap();
    let router = crab_route::LbRouter::new(&backends).unwrap();
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let upstream_pool =
        UpstreamKeyPool::from_secrets(vec!["sk-upstream-test-key-12345678".into()], 60, 1);
    let pool_handle = Arc::new(RwLock::new(upstream_pool));
    let mut profiles = IndexMap::new();
    profiles.insert(
        "deepseek".to_string(),
        Arc::new(UpstreamProfileRuntime {
            id: "deepseek".to_string(),
            provider: UpstreamProvider::Deepseek,
            base_url: "https://api.deepseek.com".to_string(),
            fallback_model: "deepseek-v4-pro".to_string(),
            tls_sni: "api.deepseek.com".to_string(),
            router: crab_route::LbRouter::new(&backends).unwrap(),
            upstream_pool: pool_handle.clone(),
            proxy_url: None,
            fallback_profile_id: None,
            fallback_max_retries: 2,
        }),
    );
    RuntimeConfig::new(
        router,
        ttl,
        ConnectionConfig::default(),
        true,
        FingerprintConfig::default(),
        "https://api.deepseek.com".to_string(),
        "deepseek-v4-pro".to_string(),
        pool_handle,
        profiles,
        "deepseek".to_string(),
        PipelineGlobals::default(),
        false,
        std::collections::HashSet::new(),
        false,
    )
}
