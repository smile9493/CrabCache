use crate::context::{ConnectionConfig, StoredKey};
use crate::upstream_pool::UpstreamKeyPool;
use crab_cache::{FingerprintConfig, TtlConfig};
use crab_route::{AffinityRouter, BackendHealth};
use dashmap::DashMap;
use std::collections::HashSet;
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
    /// DeepSeek upstream API key pool (outbound Bearer).
    pub upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
    /// When true, tokens in `legacy_client_tokens` may authenticate as clients.
    pub legacy_api_key_as_client_auth: bool,
    pub legacy_client_tokens: HashSet<String>,
    pub started_at: Instant,
    pub backend_health: Arc<RwLock<std::collections::HashMap<String, BackendHealth>>>,
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
        upstream_pool: Arc<UpstreamKeyPool>,
        legacy_api_key_as_client_auth: bool,
        legacy_client_tokens: HashSet<String>,
    ) -> Arc<Self> {
        let backends = router
            .backends()
            .iter()
            .map(|b| (b.name.clone(), BackendHealth::new_healthy()))
            .collect::<std::collections::HashMap<_, _>>();

        Arc::new(Self {
            keys: DashMap::new(),
            ttl,
            router: RwLock::new(router),
            conn_config: RwLock::new(conn_config),
            stream_cache_enabled: AtomicBool::new(stream_cache_enabled),
            fingerprint: RwLock::new(fingerprint),
            upstream_base_url: RwLock::new(upstream_base_url),
            fallback_model: RwLock::new(fallback_model),
            upstream_pool: Arc::new(RwLock::new(upstream_pool)),
            legacy_api_key_as_client_auth,
            legacy_client_tokens,
            started_at: Instant::now(),
            backend_health: Arc::new(RwLock::new(backends)),
        })
    }

    pub fn upstream_pool(&self) -> Arc<UpstreamKeyPool> {
        self.upstream_pool
            .read()
            .map(|p| Arc::clone(&p))
            .unwrap_or_else(|_| panic!("upstream_pool lock poisoned"))
    }

    pub fn replace_upstream_pool(&self, pool: Arc<UpstreamKeyPool>) {
        if let Ok(mut guard) = self.upstream_pool.write() {
            *guard = pool;
        }
    }

    pub fn is_legacy_client_token(&self, token: &str, full_auth: &str) -> bool {
        if !self.legacy_api_key_as_client_auth {
            return false;
        }
        self.legacy_client_tokens.contains(token)
            || self
                .legacy_client_tokens
                .iter()
                .any(|k| full_auth.ends_with(k))
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
}
