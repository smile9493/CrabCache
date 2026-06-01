use crate::infra::types::ContainerRawSample;
use crate::metrics_history::{GatewayMetricsCache, MetricsHistory};
use crate::persist::{self, PersistHandle};
use crate::types::UpstreamTestResult;
use crate::types::{
    DomainPolicy, OverviewCore, ReasoningConfig, RetentionPolicy, TraceAnalysis, TraceSummary,
};
use crab_control::{GatewayAdminClient, GatewayStatus, PutUpstreamKeysRequest, UpstreamKeyInput};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

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
    pub account_id: String,
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
    pub limits_config: RwLock<StoredLimitsConfig>,
    pub pricing_config: RwLock<StoredPricingConfig>,
    pub features_config: RwLock<StoredFeaturesConfig>,
    pub trace_logging_config: RwLock<StoredTraceLoggingConfig>,
    pub raw_capture_config: RwLock<StoredRawCaptureConfig>,
    pub reasoning_config: RwLock<ReasoningConfig>,
    pub upstream_config: RwLock<StoredUpstreamConfig>,
    pub models: RwLock<StoredModelList>,
    pub backends: RwLock<Vec<StoredBackend>>,
    pub metrics: RwLock<StoredMetrics>,
    pub metrics_history: RwLock<MetricsHistory>,
    pub trace_entries: RwLock<Vec<StoredTraceEntry>>,
    pub trace_summary_cache: RwLock<Option<(Instant, TraceSummary)>>,
    pub trace_analysis_cache: RwLock<Option<(Instant, TraceAnalysis)>>,
    pub domain_policies: RwLock<Vec<DomainPolicy>>,
    /// Last successful upstream reconcile from gateway Management API.
    pub upstream_reconcile_at: RwLock<Option<Instant>>,
    pub gateway_probe_cache: RwLock<Option<(Instant, GatewayProbe)>>,
    /// Cached `OverviewCore` + ETag for `/api/admin/overview/core` (background refresh).
    pub overview_core_cache: RwLock<Option<(Instant, OverviewCore, String)>>,
    pub overview_core_build_lock: AsyncMutex<()>,
    /// Cached timeseries per window + ETag for `/api/admin/overview/timeseries`.
    pub overview_timeseries_cache:
        RwLock<HashMap<String, (Instant, Vec<crate::types::TimeSeriesPoint>, String)>>,
    pub overview_timeseries_lock: AsyncMutex<()>,
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
    /// SQLite metrics store (always available as fallback for PG).
    pub metrics_store: Option<crate::metrics_store::MetricsStore>,
    /// PG config retained for background retry when initial connection fails.
    pub pg_pending_config: parking_lot::RwLock<Option<(String, usize, bool)>>, // (url, max_pool_size, migrate_from_json)
    /// Serializes concurrent PG dual-write tasks to prevent DELETE-then-INSERT races.
    pub pg_write_lock: Arc<AsyncMutex<()>>,
    /// Cached PG health probe (TTL-based, like GatewayProbe).
    pub pg_health_cache: RwLock<Option<(Instant, crate::types::PgHealth)>>,
    /// SSE broadcast channel for real-time metric push to dashboard.
    pub sse_broadcast: tokio::sync::broadcast::Sender<crate::sse::SseEvent>,
    /// Short-lived SSE tokens (token -> expiration Instant).
    pub sse_tokens: DashMap<String, Instant>,
    /// Auth credential directory for Codex OAuth (CRABCACHE_AUTH_DIR or ~/.crabcache/auths).
    pub auth_dir: std::path::PathBuf,
    /// Active Codex device-code OAuth sessions (session_id → session state).
    pub codex_device_sessions: DashMap<Uuid, crate::oauth_codex::CodexDeviceSession>,
    /// Active Codex PKCE OAuth sessions (session_id → session state).
    pub codex_pkce_sessions: DashMap<Uuid, crate::oauth_codex::CodexPkceSession>,
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
    /// `"ok"` or `"unavailable"` (from `/v1/ready`).
    pub redis_status: String,
    /// `"ok"`, `"disabled"`, or `"unavailable"` (from `/v1/ready`).
    pub l2_status: String,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredCacheConfig {
    pub l0_max_capacity: u64,
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
    pub model_overrides: Vec<(String, u64)>,
    pub consumer_overrides: Vec<(String, u64)>,
    /// Combined overrides keyed by `"consumer:model"`.
    pub consumer_model_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredSemanticConfig {
    pub enabled: bool,
    pub similarity_threshold: f32,
    pub ttl_secs: u64,
    pub collection_size: u64,
    pub min_query_chars: usize,
    pub max_query_chars: usize,
    pub max_concurrent_embeds: usize,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredConnectionConfig {
    pub tcp_keepalive_idle_secs: u64,
    pub tcp_keepalive_interval_secs: u64,
    pub tcp_keepalive_count: usize,
    pub idle_timeout_secs: u64,
    pub h2_ping_interval_secs: u64,
    #[serde(default)]
    pub upstream_force_http1: bool,
    #[serde(default)]
    pub upstream_disable_keepalive: bool,
    #[serde(default)]
    pub upstream_request_timeout_secs: u64,
    #[serde(default)]
    pub upstream_write_timeout_secs: u64,
    #[serde(default)]
    pub upstream_connection_timeout_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredLimitsConfig {
    pub max_request_body_bytes: usize,
    pub max_concurrent_requests: usize,
    pub legacy_api_key_as_client_auth: bool,
    pub cors_enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredPricingConfig {
    pub default_input_price_per_million: f64,
    pub default_output_price_per_million: f64,
    #[serde(default)]
    pub model_overrides: std::collections::HashMap<String, StoredModelPricing>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredModelPricing {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredFeaturesConfig {
    pub prefix_aware_cache: bool,
    pub streaming_body_forward: bool,
    pub connection_prewarm: bool,
    pub affinity_prompt_cache_feedback: bool,
    pub delta_cache: bool,
    pub io_uring_backend: bool,
    pub wasm_filters: bool,
    pub mimo_context_compression: bool,
    pub mimo_compression_threshold: usize,
    pub upstream_request_gzip: bool,
    pub upstream_request_gzip_min_bytes: usize,
    pub mimo_retire_prefix_messages: bool,
    pub mimo_keep_recent_turns: usize,
    pub mimo_session_store: bool,
    pub mimo_session_store_ttl_secs: u64,
    pub mimo_session_store_max_messages: usize,
    pub passthrough_prefix_bytes: usize,
    // P1-1: Multi-factor routing
    pub backend_route_strategy: String,
    pub backend_load_aware_routing_enabled: bool,
    pub backend_max_concurrent_requests: usize,
    pub backend_health_weight: f64,
    pub backend_latency_weight: f64,
    pub backend_load_weight: f64,
    pub backend_affinity_weight: f64,
    pub backend_rate_429_weight: f64,
    // P1-2: Quota preflight
    pub preflight_enabled: bool,
    pub preflight_check_health: bool,
    pub preflight_check_429_cooldown: bool,
}

impl Default for StoredFeaturesConfig {
    fn default() -> Self {
        Self {
            prefix_aware_cache: false,
            streaming_body_forward: false,
            connection_prewarm: false,
            affinity_prompt_cache_feedback: false,
            delta_cache: false,
            io_uring_backend: false,
            wasm_filters: false,
            mimo_context_compression: false,
            mimo_compression_threshold: 6,
            upstream_request_gzip: false,
            upstream_request_gzip_min_bytes: 4096,
            mimo_retire_prefix_messages: false,
            mimo_keep_recent_turns: 6,
            mimo_session_store: false,
            mimo_session_store_ttl_secs: 86400,
            mimo_session_store_max_messages: 200,
            passthrough_prefix_bytes: 1024,
            backend_route_strategy: "round_robin".to_string(),
            backend_load_aware_routing_enabled: false,
            backend_max_concurrent_requests: 0,
            backend_health_weight: 0.2,
            backend_latency_weight: 0.2,
            backend_load_weight: 0.2,
            backend_affinity_weight: 0.2,
            backend_rate_429_weight: 0.2,
            preflight_enabled: false,
            preflight_check_health: true,
            preflight_check_429_cooldown: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredTraceLoggingConfig {
    pub max_lines: u64,
    pub max_files: u64,
    pub max_payload_bytes: usize,
    pub max_response_preview_bytes: usize,
}

impl Default for StoredTraceLoggingConfig {
    fn default() -> Self {
        Self {
            max_lines: 100_000,
            max_files: 5,
            max_payload_bytes: 4096,
            max_response_preview_bytes: 512,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredRawCaptureConfig {
    pub enabled: bool,
    pub sample_rate: f64,
    pub mask_api_keys: bool,
    pub sample_always_on_error: bool,
}

impl Default for StoredRawCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sample_rate: 0.01,
            mask_api_keys: false,
            sample_always_on_error: true,
        }
    }
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
    /// OAuth account IDs (or key ids) that can serve this model upstream.
    pub account_ids: Vec<String>,
    pub key_ids: Vec<String>,
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

        let key = std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| {
            if cfg!(debug_assertions) {
                "admin".to_string()
            } else {
                eprintln!("FATAL: CRABCACHE_ADMIN_KEY is not set. Refusing to start with default 'admin' key.");
                std::process::exit(1);
            }
        });
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
                    account_id: String::new(),
                });
            }
        } else if !upstream_api_key.is_empty() {
            pool_secrets.push(UpstreamPoolSecret {
                id: "key-1".to_string(),
                secret: upstream_api_key.clone(),
                enabled: true,
                account_id: String::new(),
            });
        }

        let persist = Arc::new(PersistHandle::new());
        let loaded = persist.load();
        // If no env-driven keys, load persisted upstream pool secrets (v4+).
        if pool_secrets.is_empty() && !loaded.upstream_pool_secrets.is_empty() {
            pool_secrets = loaded
                .upstream_pool_secrets
                .iter()
                .map(|s| UpstreamPoolSecret {
                    id: s.id.clone(),
                    secret: s.secret.clone(),
                    enabled: s.enabled,
                    account_id: s.account_id.clone(),
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
                consumer_model_overrides: vec![],
            }),
            semantic_config: RwLock::new(StoredSemanticConfig {
                enabled: true,
                similarity_threshold: 0.95,
                ttl_secs: 86400,
                collection_size: 0,
                min_query_chars: 32,
                max_query_chars: 8192,
                max_concurrent_embeds: 4,
            }),
            connection_config: RwLock::new(StoredConnectionConfig {
                tcp_keepalive_idle_secs: 60,
                tcp_keepalive_interval_secs: 10,
                tcp_keepalive_count: 3,
                idle_timeout_secs: 90,
                h2_ping_interval_secs: 30,
                upstream_force_http1: false,
                upstream_disable_keepalive: false,
                upstream_request_timeout_secs: 300,
                upstream_write_timeout_secs: 300,
                upstream_connection_timeout_secs: 60,
            }),
            limits_config: RwLock::new(StoredLimitsConfig {
                max_request_body_bytes: 1024 * 1024,
                max_concurrent_requests: 512,
                legacy_api_key_as_client_auth: false,
                cors_enabled: false,
            }),
            pricing_config: RwLock::new(StoredPricingConfig {
                default_input_price_per_million: 0.55,
                default_output_price_per_million: 2.19,
                model_overrides: std::collections::HashMap::new(),
            }),
            features_config: RwLock::new(StoredFeaturesConfig::default()),
            trace_logging_config: RwLock::new(StoredTraceLoggingConfig::default()),
            raw_capture_config: RwLock::new(StoredRawCaptureConfig::default()),
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
            trace_analysis_cache: RwLock::new(None),
            upstream_reconcile_at: RwLock::new(None),
            gateway_probe_cache: RwLock::new(None),
            overview_core_cache: RwLock::new(None),
            overview_core_build_lock: AsyncMutex::new(()),
            overview_timeseries_cache: RwLock::new(HashMap::new()),
            overview_timeseries_lock: AsyncMutex::new(()),
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
            metrics_store: crate::metrics_store::MetricsStore::open().ok(),
            pg_pending_config: parking_lot::RwLock::new(None),
            pg_write_lock: Arc::new(AsyncMutex::new(())),
            pg_health_cache: RwLock::new(None),
            sse_broadcast: crate::sse::create_broadcast(),
            sse_tokens: DashMap::new(),
            auth_dir: crate::oauth_codex::resolve_auth_dir(),
            codex_device_sessions: DashMap::new(),
            codex_pkce_sessions: DashMap::new(),
        };

        // Defer PostgreSQL init to async startup (AppState::new runs inside #[tokio::main]).
        let pg_cfg = crate::pg::PgConfig::from_env();
        if pg_cfg.enabled() {
            if let Some(url) = &pg_cfg.url {
                *state.pg_pending_config.write() =
                    Some((url.clone(), pg_cfg.max_pool_size, pg_cfg.migrate_from_json));
            }
        }

        state
    }

    /// Connect PostgreSQL, optionally migrate JSON state, hydrate metrics history.
    pub async fn try_connect_pg(
        state: &Arc<Self>,
        url: &str,
        pool_size: usize,
        migrate_from_json: bool,
    ) -> bool {
        match crate::pg::PgStore::new(url, pool_size).await {
            Ok(pg) => {
                tracing::info!("PostgreSQL store initialized");
                if migrate_from_json {
                    if let Ok(true) = pg.maybe_import_from_json(&state.persist.load()).await {
                        tracing::info!("JSON state imported into PostgreSQL");
                    }
                }
                tracing::debug!("Assigning PostgreSQL store handle");
                *state.pg_store.write() = Some(pg);
                *state.pg_pending_config.write() = None;
                tracing::debug!("PostgreSQL store handle assigned");
                true
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to initialize PostgreSQL");
                false
            }
        }
    }

    /// Restore profile key pools from PostgreSQL into memory (PG is authoritative when enabled).
    pub async fn hydrate_profile_secrets_from_pg(&self) -> bool {
        let pg = self.pg_store.read().clone();
        let Some(pg) = pg else {
            return false;
        };
        match pg.load_profile_secrets().await {
            Ok(map) if !map.is_empty() => {
                let profile_count = map.len();
                let mut secrets_map = std::collections::HashMap::new();
                for (profile_id, secrets) in map {
                    secrets_map.insert(
                        profile_id,
                        secrets
                            .into_iter()
                            .map(|s| UpstreamPoolSecret {
                                id: s.id,
                                secret: s.secret,
                                enabled: s.enabled,
                                account_id: s.account_id,
                            })
                            .collect(),
                    );
                }
                *self.upstream_profile_secrets.write() = secrets_map;
                self.flush_persist();
                tracing::info!(
                    profiles = profile_count,
                    "Profile key pools restored from PostgreSQL"
                );
                true
            }
            Ok(_) => false,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load profile secrets from PostgreSQL");
                false
            }
        }
    }

    /// Restore system configs from PostgreSQL into memory (PG is authoritative when enabled).
    /// Returns the number of configs restored.
    pub async fn hydrate_system_configs_from_pg(&self) -> usize {
        let pg = self.pg_store.read().clone();
        let Some(pg) = pg else {
            return 0;
        };
        match pg.load_all_system_configs().await {
            Ok(map) if !map.is_empty() => {
                let count = map.len();
                if let Some(v) = map.get("cache_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredCacheConfig>(v.clone()) {
                        *self.cache_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("semantic_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredSemanticConfig>(v.clone()) {
                        *self.semantic_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("connection_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredConnectionConfig>(v.clone()) {
                        *self.connection_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("limits_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredLimitsConfig>(v.clone()) {
                        *self.limits_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("pricing_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredPricingConfig>(v.clone()) {
                        *self.pricing_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("features_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredFeaturesConfig>(v.clone()) {
                        *self.features_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("trace_logging_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredTraceLoggingConfig>(v.clone()) {
                        *self.trace_logging_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("raw_capture_config") {
                    if let Ok(cfg) = serde_json::from_value::<StoredRawCaptureConfig>(v.clone()) {
                        *self.raw_capture_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("reasoning_config") {
                    if let Ok(cfg) = serde_json::from_value::<ReasoningConfig>(v.clone()) {
                        *self.reasoning_config.write() = cfg;
                    }
                }
                if let Some(v) = map.get("log_retention") {
                    if let Ok(cfg) = serde_json::from_value::<RetentionPolicy>(v.clone()) {
                        *self.log_retention.write() = cfg;
                    }
                }
                if let Some(v) = map.get("admin_key") {
                    if let Some(key) = v.as_str() {
                        *self.admin_key.write() = key.to_string();
                    }
                }
                tracing::info!(
                    configs = count,
                    "System configs restored from PostgreSQL"
                );
                count
            }
            Ok(_) => 0,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load system configs from PostgreSQL");
                0
            }
        }
    }

    /// Push all cached profile key pools to Gateway (after PG hydration or recovery).
    pub async fn push_all_profile_pools_to_gateway(&self) {
        let profile_ids: Vec<String> = self
            .upstream_profile_secrets
            .read()
            .keys()
            .cloned()
            .collect();
        for profile_id in profile_ids {
            if let Err(e) = crate::upstream_profiles::sync_profile_pool_to_gateway(
                self,
                &profile_id,
                crate::types::UpstreamKeysPutMode::Replace,
            )
            .await
            {
                tracing::warn!(
                    profile_id = %profile_id,
                    error = %e,
                    "Failed to push profile pool to gateway after PG hydrate"
                );
            }
        }
    }

    /// Persist one profile's key pool to PostgreSQL (best-effort).
    pub async fn persist_profile_secrets_to_pg(&self, profile_id: &str) {
        let pg = self.pg_store.read().clone();
        let Some(pg) = pg else {
            return;
        };
        let persisted: Vec<persist::PersistedUpstreamPoolSecret> = self
            .upstream_profile_secrets
            .read()
            .get(profile_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|s| persist::PersistedUpstreamPoolSecret {
                id: s.id,
                secret: s.secret,
                enabled: s.enabled,
                account_id: s.account_id,
            })
            .collect();
        let _guard = self.pg_write_lock.lock().await;
        if let Err(e) = pg.replace_profile_secrets(profile_id, &persisted).await {
            tracing::warn!(
                error = %e,
                profile_id = %profile_id,
                "PG persist upstream profile keys failed (non-fatal)"
            );
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
                account_id: k.account_id.clone(),
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
                account_id: s.account_id.clone(),
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
        let pg_request_logs: Vec<StoredRequestLog> = self.request_logs.read().clone();

        // Snapshot system configs for PG dual-write.
        let pg_system_configs: HashMap<String, serde_json::Value> = {
            let mut m = HashMap::new();
            m.insert("cache_config".to_string(), serde_json::to_value(&*self.cache_config.read()).unwrap_or_default());
            m.insert("semantic_config".to_string(), serde_json::to_value(&*self.semantic_config.read()).unwrap_or_default());
            m.insert("connection_config".to_string(), serde_json::to_value(&*self.connection_config.read()).unwrap_or_default());
            m.insert("limits_config".to_string(), serde_json::to_value(&*self.limits_config.read()).unwrap_or_default());
            m.insert("pricing_config".to_string(), serde_json::to_value(&*self.pricing_config.read()).unwrap_or_default());
            m.insert("features_config".to_string(), serde_json::to_value(&*self.features_config.read()).unwrap_or_default());
            m.insert("trace_logging_config".to_string(), serde_json::to_value(&*self.trace_logging_config.read()).unwrap_or_default());
            m.insert("raw_capture_config".to_string(), serde_json::to_value(&*self.raw_capture_config.read()).unwrap_or_default());
            m.insert("reasoning_config".to_string(), serde_json::to_value(&*self.reasoning_config.read()).unwrap_or_default());
            m.insert("log_retention".to_string(), serde_json::to_value(&*self.log_retention.read()).unwrap_or_default());
            m.insert("admin_key".to_string(), serde_json::Value::String(self.admin_key.read().clone()));
            m
        };

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
                pg_system_configs,
            ))
        } else {
            None
        };

        self.persist.save_now(&file);

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
            system_configs,
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
                // Dual-write system configs (cache, semantic, connection, limits, pricing, features, etc.).
                if !system_configs.is_empty() {
                    if let Err(e) = pg.upsert_system_configs(&system_configs).await {
                        tracing::warn!(error = %e, "PG dual-write: upsert_system_configs failed");
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

    /// Pull full key secrets from Gateway into Admin cache when local copies are missing or incomplete.
    pub async fn sync_profile_secrets_from_gateway(&self) {
        let Ok(resp) = self.gateway.list_upstream_profiles().await else {
            return;
        };
        let pg_authoritative = self.pg_store.read().is_some();
        let mut any_updated = false;
        for profile in resp.profiles {
            let profile_id = profile.id;
            let Ok(export) = self.gateway.export_upstream_profile_keys(&profile_id).await else {
                continue;
            };
            if export.keys.is_empty() {
                continue;
            }
            let current = self
                .upstream_profile_secrets
                .read()
                .get(&profile_id)
                .cloned()
                .unwrap_or_default();
            if pg_authoritative && !current.is_empty() {
                let incomplete = current.iter().any(|s| s.secret.is_empty());
                if !incomplete {
                    continue;
                }
            }
            if crate::oauth_codex::profile_is_codex_like(self, &profile_id)
                && crate::oauth_codex::codex_oauth_secret_count(&current) > 0
            {
                continue;
            }
            let needs_sync = export.keys.len() != current.len()
                || current.iter().any(|s| s.secret.is_empty())
                || export.keys.iter().any(|ek| {
                    current
                        .iter()
                        .find(|c| c.id == ek.id)
                        .is_none_or(|c| c.secret.is_empty())
                });
            if !needs_sync {
                continue;
            }
            let secrets: Vec<UpstreamPoolSecret> = export
                .keys
                .into_iter()
                .filter(|k| !k.secret.is_empty())
                .map(|k| UpstreamPoolSecret {
                    id: if k.id.is_empty() {
                        uuid::Uuid::new_v4().to_string()
                    } else {
                        k.id
                    },
                    secret: k.secret,
                    enabled: k.enabled,
                    account_id: k.account_id,
                })
                .collect();
            if secrets.is_empty() {
                continue;
            }
            let count = secrets.len();
            self.upstream_profile_secrets
                .write()
                .insert(profile_id.clone(), secrets);
            any_updated = true;
            tracing::info!(
                profile_id = %profile_id,
                count,
                "Synced upstream profile secrets from Gateway"
            );
        }
        if any_updated {
            self.flush_persist();
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
        pg_retention_days: 7,
        compress_before_delete: false,
        compressed_retention_days: 30,
    }
}
