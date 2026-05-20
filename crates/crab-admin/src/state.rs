use crate::metrics_history::MetricsHistory;
use crate::persist::{self, PersistHandle};
use crate::types::TraceSummary;
use std::time::Instant;
use crab_control::GatewayAdminClient;
use crab_control::UpstreamTestResult;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Extended metadata for an API key (quota/UI fields not stored on the gateway).
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    pub id: String,
    pub token: String,
    pub rpm_limit: u64,
    pub monthly_token_limit: u64,
    pub current_rpm: u64,
    pub tokens_this_month: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Vec<String>,
    pub remain_quota: i64,
    pub unlimited_quota: bool,
}

#[derive(Debug, Clone)]
pub struct LastInvalidate {
    pub scope: String,
    pub status: String,
    pub at_secs: u64,
    pub error: Option<String>,
}

/// Cached DeepSeek secrets for model sync (populated via Dashboard PUT upstream keys).
#[derive(Debug, Clone)]
pub struct UpstreamPoolSecret {
    pub id: String,
    pub secret: String,
    pub enabled: bool,
}

pub struct AppState {
    pub start_time: u64,
    pub upstream_api_key: String,
    /// Secrets last pushed to the gateway key pool (used by sync_models).
    pub upstream_pool_secrets: RwLock<Vec<UpstreamPoolSecret>>,
    pub upstream_notes: RwLock<Option<String>>,
    pub last_upstream_test: RwLock<Option<UpstreamTestResult>>,
    pub gateway_reachable: RwLock<bool>,
    pub persist: Arc<PersistHandle>,
    pub gateway: GatewayAdminClient,
    pub last_invalidate: RwLock<Option<LastInvalidate>>,
    /// API key metadata indexed by key id (gateway-assigned).
    pub keys_meta: DashMap<String, KeyMetadata>,
    pub request_logs: RwLock<Vec<StoredRequestLog>>,
    pub cache_config: RwLock<StoredCacheConfig>,
    pub semantic_config: RwLock<StoredSemanticConfig>,
    pub connection_config: RwLock<StoredConnectionConfig>,
    pub upstream_config: RwLock<StoredUpstreamConfig>,
    pub models: RwLock<StoredModelList>,
    pub backends: RwLock<Vec<StoredBackend>>,
    pub metrics: RwLock<StoredMetrics>,
    pub metrics_history: RwLock<MetricsHistory>,
    pub trace_entries: RwLock<Vec<StoredTraceEntry>>,
    pub trace_summary_cache: RwLock<Option<(Instant, TraceSummary)>>,
}

#[derive(Debug, Clone)]
pub struct StoredRequestLog {
    pub id: String,
    pub timestamp: u64,
    pub model: String,
    pub consumer: String,
    pub duration_ms: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_status: String,
    pub cache_tier: String,
    pub status_code: u16,
    pub conversation_id: String,
    pub request_payload: serde_json::Value,
    pub response_body: String,
    pub cache_path: Vec<String>,
    pub route_backend: String,
}

#[derive(Debug, Clone)]
pub struct StoredCacheConfig {
    pub l0_max_capacity: u64,
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
    pub model_overrides: Vec<(String, u64)>,
    pub consumer_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone)]
pub struct StoredSemanticConfig {
    pub enabled: bool,
    pub similarity_threshold: f32,
    pub ttl_secs: u64,
    pub collection_size: u64,
}

#[derive(Debug, Clone)]
pub struct StoredBackend {
    pub name: String,
    pub addr: String,
    pub weight: u32,
    pub healthy: bool,
    pub request_count: u64,
}

#[derive(Debug, Clone)]
pub struct StoredUpstreamConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub endpoints: Vec<String>,
}

impl Default for StoredUpstreamConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            api_key: String::new(),
            endpoints: vec!["api.deepseek.com:443".to_string()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredConnectionConfig {
    pub tcp_keepalive_idle_secs: u64,
    pub tcp_keepalive_interval_secs: u64,
    pub tcp_keepalive_count: usize,
    pub idle_timeout_secs: u64,
    pub h2_ping_interval_secs: u64,
}

#[derive(Debug, Clone)]
pub struct StoredModel {
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct StoredModelList {
    pub models: Vec<StoredModel>,
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredMetrics {
    pub total_requests: u64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub l0_latency_sum_ms: f64,
    pub l0_latency_count: u64,
    pub l1_latency_sum_ms: f64,
    pub l1_latency_count: u64,
    pub l2_latency_sum_ms: f64,
    pub l2_latency_count: u64,
    pub upstream_latency_sum_ms: f64,
    pub upstream_latency_count: u64,
    pub ttft_sum_ms: f64,
    pub ttft_count: u64,
}

#[derive(Debug, Clone)]
pub struct StoredTraceEntry {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub content_length: usize,
    pub semantic_cluster: usize,
    pub conversation_id: Option<String>,
    pub model: String,
    pub prompt_tokens: usize,
    pub latency_ms: f64,
    pub cache_hit: bool,
}

impl Default for StoredMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            l0_hits: 0,
            l1_hits: 0,
            l2_hits: 0,
            cache_misses: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            l0_latency_sum_ms: 0.0,
            l0_latency_count: 0,
            l1_latency_sum_ms: 0.0,
            l1_latency_count: 0,
            l2_latency_sum_ms: 0.0,
            l2_latency_count: 0,
            upstream_latency_sum_ms: 0.0,
            upstream_latency_count: 0,
            ttft_sum_ms: 0.0,
            ttft_count: 0,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let upstream_api_key = std::env::var("DEEPSEEK_API_KEY")
            .or_else(|_| std::env::var("CRABCACHE_API_KEY"))
            .unwrap_or_default();
        let mut pool_secrets = Vec::new();
        if let Ok(csv) = std::env::var("CRABCACHE_UPSTREAM_KEYS") {
            for (i, secret) in csv
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .enumerate()
            {
                pool_secrets.push(UpstreamPoolSecret {
                    id: format!("key-{}", i + 1),
                    secret: secret.to_string(),
                    enabled: true,
                });
            }
        } else if !upstream_api_key.is_empty() {
            pool_secrets.push(UpstreamPoolSecret {
                id: "key-1".to_string(),
                secret: upstream_api_key.clone(),
                enabled: true,
            });
        }

        let persist = Arc::new(PersistHandle::new());
        let loaded = persist.load();
        let models = StoredModelList::from(loaded.models);
        let mut upstream_cfg = StoredUpstreamConfig::default();
        if let Some(snap) = loaded.upstream_snapshot {
            upstream_cfg.base_url = snap.base_url;
            upstream_cfg.model = snap.model;
            upstream_cfg.endpoints = snap.endpoints;
        }

        Self {
            start_time: now,
            upstream_api_key,
            upstream_pool_secrets: RwLock::new(pool_secrets),
            upstream_notes: RwLock::new(loaded.upstream_notes),
            last_upstream_test: RwLock::new(loaded.last_upstream_test),
            gateway_reachable: RwLock::new(false),
            persist,
            gateway: GatewayAdminClient::from_env(),
            keys_meta: DashMap::new(),
            request_logs: RwLock::new(Vec::new()),
            cache_config: RwLock::new(StoredCacheConfig {
                l0_max_capacity: 10000,
                l0_ttl_secs: 3600,
                l1_ttl_secs: 3600,
                default_ttl_secs: 3600,
                model_overrides: vec![
                    ("deepseek-chat".into(), 7200),
                    ("deepseek-coder".into(), 300),
                ],
                consumer_overrides: vec![("reporting-job".into(), 1800)],
            }),
            semantic_config: RwLock::new(StoredSemanticConfig {
                enabled: true,
                similarity_threshold: 0.95,
                ttl_secs: 86400,
                collection_size: 0,
            }),
            connection_config: RwLock::new(StoredConnectionConfig {
                tcp_keepalive_idle_secs: 60,
                tcp_keepalive_interval_secs: 10,
                tcp_keepalive_count: 3,
                idle_timeout_secs: 90,
                h2_ping_interval_secs: 30,
            }),
            upstream_config: RwLock::new(upstream_cfg),
            models: RwLock::new(models),
            backends: RwLock::new(Vec::new()),
            metrics: RwLock::new(StoredMetrics::default()),
            metrics_history: RwLock::new(MetricsHistory::new()),
            trace_entries: RwLock::new(Vec::new()),
            trace_summary_cache: RwLock::new(None),
            last_invalidate: RwLock::new(None),
        }
    }

    pub fn replace_upstream_pool_secrets(&self, keys: &[crab_control::UpstreamKeyInput]) {
        let secrets: Vec<UpstreamPoolSecret> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| UpstreamPoolSecret {
                id: if k.id.is_empty() {
                    format!("key-{}", i + 1)
                } else {
                    k.id.clone()
                },
                secret: k.secret.clone(),
                enabled: k.enabled,
            })
            .collect();
        *self.upstream_pool_secrets.write() = secrets;
    }

    pub fn flush_persist(&self) {
        let file = persist::build_state_file(
            &self.models.read(),
            &self.upstream_config.read(),
            self.last_upstream_test.read().clone(),
            self.upstream_notes.read().clone(),
        );
        self.persist.save_debounced(file);
    }

    /// Merge gateway relay + backends into stored upstream config (gateway wins).
    pub async fn reconcile_upstream_from_gateway(&self) {
        match self.gateway.get_upstream_relay().await {
            Ok(relay) => {
                *self.gateway_reachable.write() = true;
                let mut cfg = self.upstream_config.write();
                cfg.base_url = relay.base_url;
                cfg.model = relay.model;
            }
            Err(e) => {
                *self.gateway_reachable.write() = false;
                tracing::warn!(error = %e, "Could not fetch upstream relay from gateway");
            }
        }

        if let Ok(backends) = self.gateway.get_backends().await {
            let endpoints: Vec<String> = backends.backends.iter().map(|b| b.addr.clone()).collect();
            if !endpoints.is_empty() {
                self.upstream_config.write().endpoints = endpoints;
            }
        }

        if let Ok(keys) = self.gateway.get_upstream_keys().await {
            self.replace_upstream_pool_from_views(&keys.keys);
        }
    }

    fn replace_upstream_pool_from_views(&self, views: &[crab_control::UpstreamKeyView]) {
        if views.is_empty() {
            return;
        }
        let secrets: Vec<UpstreamPoolSecret> = self.upstream_pool_secrets.read().clone();
        if secrets.iter().any(|s| !s.secret.is_empty()) {
            return;
        }
        tracing::info!(
            count = views.len(),
            "Gateway key pool has entries but admin has no secrets; enable keys via dashboard replace"
        );
    }

    /// Pick a DeepSeek API key for upstream model list sync.
    pub fn pick_sync_api_key(&self) -> Option<String> {
        let pool = self.upstream_pool_secrets.read();
        if let Some(s) = pool.iter().find(|k| k.enabled && !k.secret.is_empty()) {
            return Some(s.secret.clone());
        }
        drop(pool);
        let cfg = self.upstream_config.read();
        if !cfg.api_key.is_empty() && !cfg.api_key.contains("****") {
            return Some(cfg.api_key.clone());
        }
        if !self.upstream_api_key.is_empty() {
            return Some(self.upstream_api_key.clone());
        }
        None
    }
}
