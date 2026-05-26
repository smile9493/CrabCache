use crate::infra::types::ContainerRawSample;
use crate::metrics_history::{GatewayMetricsCache, MetricsHistory};
use crate::persist::{self, PersistHandle};
use crate::types::UpstreamTestResult;
use crate::types::{DomainPolicy, OverviewCore, ReasoningConfig, RetentionPolicy, TraceSummary};
use crab_control::{GatewayAdminClient, GatewayStatus, PutUpstreamKeysRequest, UpstreamKeyInput};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as AsyncMutex;

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
    pub trace_entries: RwLock<Vec<StoredTraceEntry>>,
    pub trace_summary_cache: RwLock<Option<(Instant, TraceSummary)>>,
    pub domain_policies: RwLock<Vec<DomainPolicy>>,
    /// Last successful upstream reconcile from gateway Management API.
    pub upstream_reconcile_at: RwLock<Option<Instant>>,
    pub gateway_probe_cache: RwLock<Option<(Instant, GatewayProbe)>>,
    /// Cached `OverviewCore` + ETag for `/api/admin/overview/core` (background refresh).
    pub overview_core_cache: RwLock<Option<(Instant, OverviewCore, String)>>,
    pub overview_core_build_lock: AsyncMutex<()>,
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
    /// Log retention policy for automatic cleanup.
    pub log_retention: RwLock<RetentionPolicy>,
    /// PostgreSQL store (None when CRADMIN_PG_URL is not set).
    pub pg_store: parking_lot::RwLock<Option<crate::pg::PgStore>>,
    /// PG config retained for background retry when initial connection fails.
    pub pg_pending_config: parking_lot::RwLock<Option<(String, usize, bool)>>, // (url, max_pool_size, migrate_from_json)
    /// Serializes concurrent PG dual-write tasks to prevent DELETE-then-INSERT races.
    pub pg_write_lock: Arc<AsyncMutex<()>>,
    /// Cached PG health probe (TTL-based, like GatewayProbe).
    pub pg_health_cache: RwLock<Option<(Instant, crate::types::PgHealth)>>,
    /// SSE broadcast channel for real-time metric push to dashboard.
    pub sse_broadcast: tokio::sync::broadcast::Sender<crate::sse::SseEvent>,
}

impl AppState {
    /// Returns `true` when a PG store is configured and connected.
    pub fn has_pg(&self) -> bool {
        self.pg_store.read().is_some()
    }

    /// Load trace entries from PG if available, otherwise from JSONL.
    pub async fn load_trace_entries(
        &self,
        trace_path: &str,
        hours: u32,
    ) -> Vec<crate::trace_log::TraceLogEntry> {
        let pg = self.pg_store.read().clone();
        crate::trace_log::load_trace_entries_auto(pg, trace_path, hours).await
    }

    /// Load trace entries with opts from PG if available, otherwise from JSONL.
    pub async fn load_trace_with_opts(
        &self,
        trace_path: &str,
        opts: &crate::trace_log::TraceLoadOpts,
    ) -> Vec<crate::trace_log::TraceLogEntry> {
        let pg = self.pg_store.read().clone();
        crate::trace_log::load_trace_with_opts_auto(pg, trace_path, opts).await
    }

    /// Find a single trace entry by id, trying PG first then JSONL.
    pub async fn find_trace_entry(
        &self,
        id: &str,
        trace_path: &str,
    ) -> Option<crate::trace_log::TraceLogEntry> {
        let pg = self.pg_store.read().clone();
        crate::trace_log::find_trace_entry_auto(pg, id, trace_path).await
    }
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
        let loaded_for_pg = loaded.clone(); // clone before partial moves for PG migration

        // If no env-driven keys, load persisted upstream pool secrets (v4+).
        if pool_secrets.is_empty() && !loaded.upstream_pool_secrets.is_empty() {
            pool_secrets = loaded
                .upstream_pool_secrets
                .iter()
                .map(|s| UpstreamPoolSecret {
                    id: s.id.clone(),
                    secret: s.secret.clone(),
                    enabled: s.enabled,
                })
                .collect();
        }
        let models = StoredModelList::from(loaded.models);
        let mut upstream_cfg = StoredUpstreamConfig::default();
        if let Some(snap) = loaded.upstream_snapshot {
            upstream_cfg.base_url = snap.base_url;
            upstream_cfg.model = snap.model;
            upstream_cfg.endpoints = snap.endpoints;
        }

        let history = MetricsHistory::new();

        let state = Self {
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
                for meta in &loaded.keys_meta {
                    let km: KeyMetadata = meta.clone().into();
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
                missing_reasoning_strategy: "recover".to_string(),
                display_reasoning: true,
                collapsible_reasoning: true,
                cache_invalidate_recommended: None,
                storage_backend: String::new(),
                cache_db_path: String::new(),
                redis_url_masked: None,
                sqlite_cache_enabled: true,
                sqlite_cache_path: None,
                reasoning_recovery: Some(true),
            }),
            upstream_config: RwLock::new(upstream_cfg),
            models: RwLock::new(models),
            backends: RwLock::new(Vec::new()),
            metrics: RwLock::new(StoredMetrics::default()),
            metrics_history: RwLock::new(history),
            trace_entries: RwLock::new(Vec::new()),
            trace_summary_cache: RwLock::new(None),
            upstream_reconcile_at: RwLock::new(None),
            gateway_probe_cache: RwLock::new(None),
            overview_core_cache: RwLock::new(None),
            overview_core_build_lock: AsyncMutex::new(()),
            gateway_metrics_cache: GatewayMetricsCache::default(),
            live_trace_cache: RwLock::new(crate::trace_log::LiveTraceCache::default()),
            key_usage_last_synced: parking_lot::Mutex::new(0),
            domain_policies: RwLock::new({
                let policies: Vec<DomainPolicy> = loaded
                    .domain_policies
                    .iter()
                    .map(|p| DomainPolicy {
                        domain: p.domain.clone(),
                        monthly_token_budget: p.monthly_token_budget,
                        monthly_cost_budget_usd: p.monthly_cost_budget_usd,
                        min_hit_rate: p.min_hit_rate,
                        enabled: p.enabled,
                        pipeline: p.pipeline.clone(),
                        upstream_profile: p.upstream_profile.clone(),
                    })
                    .collect();
                policies
            }),
            last_invalidate: RwLock::new(None),
            infra_docker: crate::infra::docker::try_connect(&crate::infra::resolve_docker_host()),
            infra_prev: RwLock::new(HashMap::new()),
            infra_cache: crate::infra::InfraCache::new(),
            infra_history: RwLock::new(crate::infra::history::InfraHistoryRing::new()),
            infra_speed_jobs: Arc::new(crate::infra::speed_test::SpeedTestJobs::new()),
            log_retention: RwLock::new(load_retention_policy()),
            pg_store: parking_lot::RwLock::new(None),
            pg_pending_config: parking_lot::RwLock::new(None),
            pg_write_lock: Arc::new(AsyncMutex::new(())),
            pg_health_cache: RwLock::new(None),
            sse_broadcast: crate::sse::create_broadcast(),
        };

        // Initialize PostgreSQL store (async) if configured.
        let pg_cfg = crate::pg::PgConfig::from_env();
        if pg_cfg.enabled() {
            if let Some(url) = &pg_cfg.url {
                match tokio::runtime::Handle::current()
                    .block_on(crate::pg::PgStore::new(url, pg_cfg.max_pool_size))
                {
                    Ok(pg) => {
                        tracing::info!("PostgreSQL store initialized");
                        // Attempt one-time JSON → PG migration.
                        if pg_cfg.migrate_from_json {
                            if let Ok(migrated) = tokio::runtime::Handle::current()
                                .block_on(pg.maybe_import_from_json(&loaded_for_pg))
                            {
                                if migrated {
                                    tracing::info!("JSON state imported into PostgreSQL");
                                }
                            }
                        }
                        // Hydrate metrics history from PG if it has more data than SQLite.
                        let cutoff = now.saturating_sub(crate::metrics_history::MAX_RETENTION_SECS);
                        if let Ok(pg_snapshots) = tokio::runtime::Handle::current()
                            .block_on(pg.load_metric_snapshots_since(cutoff))
                        {
                            let sqlite_count = state.metrics_history.read().sample_count();
                            if pg_snapshots.len() > sqlite_count {
                                let mut hist = state.metrics_history.write();
                                *hist = MetricsHistory::new();
                                for s in &pg_snapshots {
                                    hist.append(s.clone());
                                }
                                tracing::info!(
                                    hydrated = hist.sample_count(),
                                    "Metrics history restored from PostgreSQL (supersedes SQLite)"
                                );
                            }
                        }
                        *state.pg_store.write() = Some(pg);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to initialize PostgreSQL; will retry in background");
                        *state.pg_pending_config.write() =
                            Some((url.clone(), pg_cfg.max_pool_size, pg_cfg.migrate_from_json));
                    }
                }
            }
        }

        state
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
        let pool_secrets: Vec<persist::PersistedUpstreamPoolSecret> = self
            .upstream_pool_secrets
            .read()
            .iter()
            .map(|s| persist::PersistedUpstreamPoolSecret {
                id: s.id.clone(),
                secret: s.secret.clone(),
                enabled: s.enabled,
            })
            .collect();
        let file = persist::build_state_file(
            &self.models.read(),
            &self.upstream_config.read(),
            self.last_upstream_test.read().clone(),
            self.upstream_notes.read().clone(),
            &keys_meta,
            &domain_policies,
            &profile_secrets,
            &pool_secrets,
        );

        // Snapshot request logs for PG dual-write (take before releasing lock).
        let pg_request_logs: Vec<StoredRequestLog> =
            self.request_logs.read().clone();

        // Dual-write to PostgreSQL if available (extract data before moving file).
        let pg_task = if let Some(ref pg) = *self.pg_store.read() {
            let pg = pg.clone();
            let pg_keys = keys_meta;
            let pg_policies = domain_policies;
            let pg_pool = pool_secrets;
            let pg_profiles = profile_secrets;
            let models_snap = file.models.clone();
            let upstream_snap = file.upstream_snapshot.clone();
            let notes = file.upstream_notes.clone();
            let last_test = file.last_upstream_test.clone();
            Some((
                pg,
                pg_keys,
                pg_policies,
                pg_pool,
                pg_profiles,
                models_snap,
                upstream_snap,
                notes,
                last_test,
            ))
        } else {
            None
        };

        self.persist.save_debounced(file);

        if let Some((
            pg,
            pg_keys,
            pg_policies,
            pg_pool,
            pg_profiles,
            models_snap,
            upstream_snap,
            notes,
            last_test,
        )) = pg_task
        {
            let write_lock = self.pg_write_lock.clone();
            tokio::spawn(async move {
                let _guard = write_lock.lock().await;
                for key in &pg_keys {
                    if let Err(e) = pg.upsert_key(key).await {
                        tracing::warn!(error = %e, key_id = %key.id, "PG dual-write: upsert_key failed");
                    }
                }
                if let Err(e) = pg.replace_policies(&pg_policies).await {
                    tracing::warn!(error = %e, "PG dual-write: replace_policies failed");
                }
                if let Err(e) = pg.replace_pool_secrets(&pg_pool).await {
                    tracing::warn!(error = %e, "PG dual-write: replace_pool_secrets failed");
                }
                for (pid, secrets) in &pg_profiles.by_profile {
                    if let Err(e) = pg.replace_profile_secrets(pid, secrets).await {
                        tracing::warn!(error = %e, profile_id = %pid, "PG dual-write: replace_profile_secrets failed");
                    }
                }
                for (pid, synced_at) in &models_snap.synced_at_by_profile {
                    let profile_models: Vec<_> = models_snap
                        .models
                        .iter()
                        .filter(|m| &m.profile_id == pid)
                        .cloned()
                        .collect();
                    if let Err(e) = pg.replace_models(pid, &profile_models, synced_at).await {
                        tracing::warn!(error = %e, profile_id = %pid, "PG dual-write: replace_models failed");
                    }
                }
                if let Some(ref snap) = upstream_snap {
                    if let Err(e) = pg
                        .save_upstream(
                            &snap.base_url,
                            &snap.model,
                            &snap.endpoints,
                            notes.as_deref(),
                            last_test.as_ref(),
                        )
                        .await
                    {
                        tracing::warn!(error = %e, "PG dual-write: save_upstream failed");
                    }
                }
                // Dual-write request logs.
                if !pg_request_logs.is_empty() {
                    if let Err(e) = pg.insert_request_logs(&pg_request_logs).await {
                        tracing::warn!(error = %e, count = pg_request_logs.len(), "PG dual-write: insert_request_logs failed");
                    }
                }
            });
        }
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
            if let Some(at) = *guard
                && at.elapsed()
                    < std::time::Duration::from_secs(Self::upstream_reconcile_interval_secs())
            {
                return;
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
                if let Some(key) = relay
                    .api_key
                    .filter(|k| !k.is_empty() && !k.contains("****"))
                {
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
            if keys.keys.is_empty() {
                // Gateway pool is empty — push persisted admin secrets if available.
                let secrets = self.upstream_pool_secrets.read().clone();
                if !secrets.is_empty() {
                    let req = PutUpstreamKeysRequest {
                        keys: secrets
                            .iter()
                            .map(|s| UpstreamKeyInput {
                                id: s.id.clone(),
                                secret: s.secret.clone(),
                                enabled: s.enabled,
                                account_id: String::new(),
                            })
                            .collect(),
                        mode: crab_control::UpstreamKeysPutMode::Replace,
                    };
                    match self.gateway.put_upstream_keys(&req).await {
                        Ok(_) => tracing::info!(
                            count = secrets.len(),
                            "Pushed persisted upstream secrets to Gateway (empty pool detected)"
                        ),
                        Err(e) => tracing::warn!(
                            error = %e,
                            "Failed to push persisted upstream secrets to Gateway"
                        ),
                    }
                }
            } else {
                self.replace_upstream_pool_from_views(&keys.keys);
            }
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
        if let Some(pool) = profiles.get(profile_id)
            && let Some(s) = pool.iter().find(|k| k.enabled && !k.secret.is_empty())
        {
            return Some(s.secret.clone());
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

fn load_retention_policy() -> RetentionPolicy {
    let max_age_hours = std::env::var("CRABCACHE_LOG_MAX_AGE_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(168);
    let max_disk_mb = std::env::var("CRABCACHE_LOG_MAX_DISK_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let max_trace_files = std::env::var("CRABCACHE_LOG_MAX_TRACE_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let max_capture_body_files = std::env::var("CRABCACHE_LOG_MAX_CAPTURE_BODY_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    RetentionPolicy {
        max_age_hours,
        max_disk_mb,
        max_trace_files,
        max_capture_body_files,
    }
}
