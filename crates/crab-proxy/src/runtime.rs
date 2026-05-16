use crate::context::{ConnectionConfig, StoredKey};
use crab_cache::{FingerprintConfig, TtlConfig};
use crab_route::AffinityRouter;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

pub struct RuntimeConfig {
    pub keys: DashMap<String, StoredKey>,
    pub ttl: Arc<RwLock<TtlConfig>>,
    pub router: RwLock<AffinityRouter>,
    pub conn_config: RwLock<ConnectionConfig>,
    pub stream_cache_enabled: AtomicBool,
    pub fingerprint: RwLock<FingerprintConfig>,
    pub upstream_base_url: RwLock<String>,
    pub fallback_model: RwLock<String>,
    pub bootstrap_api_key: String,
    pub started_at: Instant,
}

impl RuntimeConfig {
    pub fn new(
        router: AffinityRouter,
        ttl: Arc<RwLock<TtlConfig>>,
        conn_config: ConnectionConfig,
        stream_cache_enabled: bool,
        fingerprint: FingerprintConfig,
        upstream_base_url: String,
        fallback_model: String,
        bootstrap_api_key: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            keys: DashMap::new(),
            ttl,
            router: RwLock::new(router),
            conn_config: RwLock::new(conn_config),
            stream_cache_enabled: AtomicBool::new(stream_cache_enabled),
            fingerprint: RwLock::new(fingerprint),
            upstream_base_url: RwLock::new(upstream_base_url),
            fallback_model: RwLock::new(fallback_model),
            bootstrap_api_key,
            started_at: Instant::now(),
        })
    }

    pub fn stream_cache_enabled(&self) -> bool {
        self.stream_cache_enabled.load(Ordering::Relaxed)
    }

    pub fn set_stream_cache_enabled(&self, enabled: bool) {
        self.stream_cache_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn insert_bootstrap_key(&self, token: &str, name: &str) {
        self.keys.insert(
            token.to_string(),
            StoredKey {
                id: "default".to_string(),
                name: name.to_string(),
                key_hash: token.to_string(),
                enabled: true,
            },
        );
    }
}
