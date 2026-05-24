use crate::infra::types::ContainerRawSample;
use crate::metrics_history::{GatewayMetricsCache, MetricsHistory};
use crate::persist::{self, PersistHandle};
use crate::types::{DomainPolicy, ReasoningConfig, TraceSummary};
use std::collections::HashMap;
use std::time::Instant;
use crab_control::{GatewayAdminClient, GatewayStatus, UpstreamTestResult};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Extended metadata for an API key (quota/UI fields not stored on the gateway).
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    pub id: String,
    pub name: String,
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
    pub max_concurrent: u32,
    /// Month key (YYYY-MM) for the accumulated tokens_this_month/input_tokens/output_tokens.
    /// When the current month differs from this value on load, counters are reset.
    pub usage_month: String,
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
    pub current_version: String,
    pub admin_key: Arc<RwLock<String>>,
    pub upstream_api_key: String,
    /// Secrets last pushed to the gateway key pool (default profile legacy).
    pub upstream_pool_secrets: RwLock<Vec<UpstreamPoolSecret>>,
    /// Per-profile upstream API keys for model sync (admin-side cache).
    pub upstream_profile_secrets: RwLock<HashMap<String, Vec<UpstreamPoolSecret>>>,
    /// profile_id → provider string (from gateway; used for model metadata).
    pub upstream_profile_providers: RwLock<HashMap<String, String>>,
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
    pub reasoning_config: RwLock<ReasoningConfig>,
    pub upstream_config: RwLock<StoredUpstreamConfig>,
    pub models: RwLock<StoredModelList>,
    pub backends: RwLock<Vec<StoredBackend>>,
    pub metrics: RwLock<StoredMetrics>,
    pub metrics_history: RwLock<MetricsHistory>,
    /// SQLite-backed cold storage for Prometheus counter snapshots.
    pub metrics_store: Option<crate::metrics_store::MetricsStore>,
    pub trace_entries: RwLock<Vec<StoredTraceEntry>>,
    pub trace_summary_cache: RwLock<Option<(Instant, TraceSummary)>>,
    pub domain_policies: RwLock<Vec<DomainPolicy>>,
    /// Last successful upstream reconcile from gateway Management API.
    pub upstream_reconcile_at: RwLock<Option<Instant>>,
    pub gateway_probe_cache: RwLock<Option<(Instant, GatewayProbe)>>,
    pub gateway_metrics_cache: GatewayMetricsCache,
    /// Shared parsed trace tail for live-metrics (incremental tail, configurable TTL via CRABCACHE_LIVE_TRACE_CACHE_TTL_SECS).
    pub live_trace_cache: RwLock<crate::trace_log::LiveTraceCache>,
    /// Timestamp (ms) of the last trace entry synced for key usage accumulation.
    pub key_usage_last_synced: parking_lot::Mutex<u64>,
    /// Docker client for container monitoring (None if socket unavailable).
    pub infra_docker: Option<bollard::Docker>,
    /// Previous raw counters for infra rate calculation.
    pub infra_prev: RwLock<HashMap<String, ContainerRawSample>>,
    /// Cached infra snapshot for TTL-based dedup.
    pub infra_cache: crate::infra::InfraCache,
    /// In-memory ring of infra metric samples for charts.
    pub infra_history: RwLock<crate::infra::history::InfraHistoryRing>,
    /// Active bandwidth test jobs.
    pub infra_speed_jobs: Arc<crate::infra::speed_test::SpeedTestJobs>,
}

/// Cached result of gateway `/v1/ready` + `/v1/status` for overview and health endpoints.
#[derive(Debug, Clone)]
pub struct GatewayProbe {
    pub ready_ok: bool,
    pub ready_error: Option<String>,
    pub status: Option<GatewayStatus>,
    pub status_error: Option<String>,
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
    pub profile_id: String,
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StoredModelList {
    pub models: Vec<StoredModel>,
    /// Per-profile last sync timestamp (UTC string).
    pub synced_at_by_profile: HashMap<String, String>,
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
    /// Load admin key from file, falling back to env var.
    fn load_or_init_admin_key() -> String {
        let state_dir = std::env::var("CRABCACHE_ADMIN_STATE_PATH")
            .ok()
            .and_then(|p| std::path::Path::new(&p).parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("data"));
        let key_path = state_dir.join("admin-key.txt");

        if let Ok(content) = std::fs::read_to_string(&key_path) {
            let key = content.trim().to_string();
            if !key.is_empty() {
                return key;
            }
        }

        let key = std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| "admin".to_string());
        let _ = std::fs::create_dir_all(&state_dir);
        let _ = std::fs::write(&key_path, &key);
        key
    }

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

        // Open MetricsStore and hydrate memory ring from SQLite.
        let metrics_store = crate::metrics_store::MetricsStore::open().ok();
        let mut history = MetricsHistory::new();
        if let Some(ref store) = metrics_store {
            let cutoff = now.saturating_sub(crate::metrics_history::MAX_RETENTION_SECS);
            let snapshots = store.load_snapshots_since(cutoff);
            for s in snapshots {
                history.append(s);
            }
            tracing::info!(
                hydrated = history.sample_count(),
                "Metrics history restored from SQLite"
            );
        }

        Self {
            start_time: now,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            admin_key: Arc::new(RwLock::new(Self::load_or_init_admin_key())),
            upstream_api_key,
            upstream_pool_secrets: RwLock::new(pool_secrets),
            upstream_profile_secrets: RwLock::new(loaded.upstream_profile_secrets.into()),
            upstream_profile_providers: RwLock::new(HashMap::new()),
            upstream_notes: RwLock::new(loaded.upstream_notes),
            last_upstream_test: RwLock::new(loaded.last_upstream_test),
            gateway_reachable: RwLock::new(false),
            persist,
            gateway: GatewayAdminClient::from_env(),
            keys_meta: {
                let map = DashMap::new();
                for meta in loaded.keys_meta {
                    let km: KeyMetadata = meta.into();
                    map.insert(km.id.clone(), km);
                }
                map
            },
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
            reasoning_config: RwLock::new(ReasoningConfig {
                thinking_mode: "auto".to_string(),
                reasoning_effort: "medium".to_string(),
                reasoning_recovery: true,
                sqlite_cache_enabled: true,
                sqlite_cache_path: None,
            }),
            upstream_config: RwLock::new(upstream_cfg),
            models: RwLock::new(models),
            backends: RwLock::new(Vec::new()),
            metrics: RwLock::new(StoredMetrics::default()),
            metrics_history: RwLock::new(history),
            metrics_store,
            trace_entries: RwLock::new(Vec::new()),
            trace_summary_cache: RwLock::new(None),
            upstream_reconcile_at: RwLock::new(None),
            gateway_probe_cache: RwLock::new(None),
            gateway_metrics_cache: GatewayMetricsCache::default(),
            live_trace_cache: RwLock::new(crate::trace_log::LiveTraceCache::default()),
            key_usage_last_synced: parking_lot::Mutex::new(0),
            domain_policies: RwLock::new(
                loaded
                    .domain_policies
                    .into_iter()
                    .map(|p| DomainPolicy {
                        domain: p.domain,
                        monthly_token_budget: p.monthly_token_budget,
                        monthly_cost_budget_usd: p.monthly_cost_budget_usd,
                        min_hit_rate: p.min_hit_rate,
                        enabled: p.enabled,
                        pipeline: p.pipeline,
                        upstream_profile: p.upstream_profile,
                    })
                    .collect(),
            ),
            last_invalidate: RwLock::new(None),
            infra_docker: crate::infra::docker::try_connect(&crate::infra::resolve_docker_host()),
            infra_prev: RwLock::new(HashMap::new()),
            infra_cache: crate::infra::InfraCache::new(),
            infra_history: RwLock::new(crate::infra::history::InfraHistoryRing::new()),
            infra_speed_jobs: Arc::new(crate::infra::speed_test::SpeedTestJobs::new()),
        }
    }

    pub async fn fetch_gateway_metrics(&self) -> Result<String, String> {
        crate::metrics_history::fetch_gateway_metrics_cached(&self.gateway_metrics_cache).await
    }

    pub async fn sync_domain_policies_to_gateway(&self) {
        let policies: Vec<crab_control::DomainPolicySpec> = self
            .domain_policies
            .read()
            .iter()
            .map(|p| crab_control::DomainPolicySpec {
                domain: p.domain.clone(),
                monthly_token_budget: p.monthly_token_budget,
                monthly_cost_budget_usd: p.monthly_cost_budget_usd,
                min_hit_rate: p.min_hit_rate,
                enabled: p.enabled,
                pipeline: p.pipeline.clone(),
                upstream_profile: p.upstream_profile.clone(),
            })
            .collect();
        if let Err(e) = self
            .gateway
            .put_domain_policies(&crab_control::PutDomainPoliciesRequest { policies })
            .await
        {
            tracing::warn!(error = %e, "Failed to sync domain policies to gateway");
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
        let keys_meta: Vec<persist::PersistedKeyMetadata> = self
            .keys_meta
            .iter()
            .map(|e| persist::PersistedKeyMetadata::from(e.value()))
            .collect();
        let domain_policies: Vec<persist::PersistedDomainPolicy> = self
            .domain_policies
            .read()
            .iter()
            .map(|p| persist::PersistedDomainPolicy {
                domain: p.domain.clone(),
                monthly_token_budget: p.monthly_token_budget,
                monthly_cost_budget_usd: p.monthly_cost_budget_usd,
                min_hit_rate: p.min_hit_rate,
                enabled: p.enabled,
                pipeline: p.pipeline.clone(),
                upstream_profile: p.upstream_profile.clone(),
            })
            .collect();
        let profile_secrets: persist::PersistedProfileSecrets =
            (&*self.upstream_profile_secrets.read()).into();
        let file = persist::build_state_file(
            &self.models.read(),
            &self.upstream_config.read(),
            self.last_upstream_test.read().clone(),
            self.upstream_notes.read().clone(),
            &keys_meta,
            &domain_policies,
            &profile_secrets,
        );
        self.persist.save_debounced(file);
    }

    /// Refresh cached profile_id → provider map from the gateway.
    pub async fn refresh_profile_providers(&self) {
        match self.gateway.list_upstream_profiles().await {
            Ok(resp) => {
                let mut map = HashMap::new();
                for p in resp.profiles {
                    map.insert(p.id, p.provider);
                }
                *self.upstream_profile_providers.write() = map;
            }
            Err(e) => {
                tracing::debug!(error = %e, "Could not refresh upstream profile providers");
            }
        }
    }

    pub fn upstream_reconcile_interval_secs() -> u64 {
        std::env::var("CRABCACHE_UPSTREAM_RECONCILE_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&s| s > 0)
            .unwrap_or(30)
    }

    /// Reconcile upstream config from gateway unless a recent reconcile already ran.
    pub async fn reconcile_upstream_if_stale(&self, force: bool) {
        if !force {
            let guard = self.upstream_reconcile_at.read();
            if let Some(at) = *guard {
                if at.elapsed() < std::time::Duration::from_secs(Self::upstream_reconcile_interval_secs())
                {
                    return;
                }
            }
        }
        self.reconcile_upstream_from_gateway().await;
        *self.upstream_reconcile_at.write() = Some(Instant::now());
    }

    /// Merge gateway relay + backends into stored upstream config (gateway wins).
    pub async fn reconcile_upstream_from_gateway(&self) {
        match self.gateway.get_upstream_relay().await {
            Ok(relay) => {
                *self.gateway_reachable.write() = true;
                let mut cfg = self.upstream_config.write();
                cfg.base_url = relay.base_url;
                cfg.model = relay.model;
                if let Some(key) = relay.api_key.filter(|k| !k.is_empty() && !k.contains("****")) {
                    cfg.api_key = key;
                }
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

        self.refresh_profile_providers().await;
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

    /// Pick an API key for upstream model list sync (profile-specific or default).
    pub fn pick_sync_api_key(&self, profile_id: &str) -> Option<String> {
        let profiles = self.upstream_profile_secrets.read();
        if let Some(pool) = profiles.get(profile_id) {
            if let Some(s) = pool.iter().find(|k| k.enabled && !k.secret.is_empty()) {
                return Some(s.secret.clone());
            }
        }
        drop(profiles);
        if profile_id == "deepseek" || self.default_profile_id() == profile_id {
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
        }
        None
    }

    pub fn default_profile_id(&self) -> String {
        "deepseek".to_string()
    }

    pub fn profile_provider(&self, profile_id: &str) -> String {
        self.upstream_profile_providers
            .read()
            .get(profile_id)
            .cloned()
            .unwrap_or_else(|| profile_id.to_string())
    }
}
