use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub qps: f64,
    pub tps: f64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub latency_l0_ms: f64,
    pub latency_l1_ms: f64,
    pub latency_l2_ms: f64,
    pub latency_upstream_ms: f64,
    pub active_keys: u64,
    pub uptime_hours: u64,
    /// Gateway process uptime (preferred over admin container uptime for rates).
    #[serde(default)]
    pub uptime_secs: u64,
    pub hourly_stats: Vec<TimeSeriesPoint>,
    pub daily_stats: Vec<TimeSeriesPoint>,
    pub weekly_stats: Vec<TimeSeriesPoint>,
    pub monthly_stats: Vec<TimeSeriesPoint>,
    /// Sum of gateway_semantic_cache_requests_total with hit_* statuses (not the same as l2_hits tier).
    pub semantic_hits: u64,
    pub semantic_rejected: u64,
    pub semantic_skipped: u64,
    /// Upstream DeepSeek prefix cache (L3): prompt_cache_hit_tokens total.
    #[serde(default)]
    pub prefix_cache_hit_tokens: u64,
    #[serde(default)]
    pub prefix_cache_miss_tokens: u64,
    #[serde(default)]
    pub prefix_cache_hit_ratio: f64,
    /// Process-lifetime request hit rate (L0+L1+L2) / total.
    #[serde(default)]
    pub hit_rate_cumulative: f64,
    /// Rolling window request hit rate (default 5 minutes).
    #[serde(default)]
    pub hit_rate_5m: f64,
    /// Rolling window token-weighted input hit rate.
    #[serde(default)]
    pub token_hit_rate_5m: f64,
    #[serde(default)]
    pub qps_5m: f64,
    #[serde(default)]
    pub coalesced_total: u64,
    #[serde(default)]
    pub consumer_buckets: Vec<ConsumerMetricsBucket>,
    #[serde(default)]
    pub domain_buckets: Vec<DomainMetricsBucket>,
    /// True when the 5m window has fewer than 5 request deltas (low traffic).
    #[serde(default)]
    pub metrics_sample_insufficient: bool,
    #[serde(default)]
    pub history_meta: MetricsHistoryMeta,
    #[serde(default)]
    pub tier_deltas_5m: TierDeltas5m,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsHistoryMeta {
    pub sample_count: usize,
    pub oldest_sample_at_secs: u64,
    pub sampling_interval_secs: u64,
    /// Set when the gateway's uptime is shorter than the oldest snapshot
    /// in the history ring, indicating the gateway was restarted since.
    #[serde(default)]
    pub gateway_counter_reset: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TierDeltas5m {
    pub l0: u64,
    pub l1: u64,
    pub l2: u64,
    pub miss: u64,
    pub coalesced: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverviewOpsMetrics {
    pub cost_saved_usd_total: f64,
    pub cost_saved_usd_5m: f64,
    pub coalesced_total: u64,
    pub coalesced_5m: f64,
    pub rejected_total: u64,
    pub rejected_5m: u64,
    pub ttft_ms: f64,
    pub prefix_break_total: u64,
    pub reasoning_store_hits: u64,
    pub reasoning_store_misses: u64,
    pub stream_cache_sse_omitted: u64,
    pub upstream_key_count: u32,
    pub upstream_keys_available: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub hours: u32,
    pub total_requests: usize,
    pub cache_hit_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewSuggestion {
    pub severity: String,
    pub target: String,
    pub message: String,
}

/// Metrics without time-series arrays (lighter overview poll payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshotCore {
    pub qps: f64,
    pub tps: f64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub latency_l0_ms: f64,
    pub latency_l1_ms: f64,
    pub latency_l2_ms: f64,
    pub latency_upstream_ms: f64,
    pub active_keys: u64,
    pub uptime_hours: u64,
    #[serde(default)]
    pub uptime_secs: u64,
    pub semantic_hits: u64,
    pub semantic_rejected: u64,
    pub semantic_skipped: u64,
    #[serde(default)]
    pub prefix_cache_hit_tokens: u64,
    #[serde(default)]
    pub prefix_cache_miss_tokens: u64,
    #[serde(default)]
    pub prefix_cache_hit_ratio: f64,
    #[serde(default)]
    pub hit_rate_cumulative: f64,
    #[serde(default)]
    pub hit_rate_5m: f64,
    #[serde(default)]
    pub token_hit_rate_5m: f64,
    #[serde(default)]
    pub qps_5m: f64,
    #[serde(default)]
    pub coalesced_total: u64,
    #[serde(default)]
    pub consumer_buckets: Vec<ConsumerMetricsBucket>,
    #[serde(default)]
    pub domain_buckets: Vec<DomainMetricsBucket>,
    #[serde(default)]
    pub metrics_sample_insufficient: bool,
    #[serde(default)]
    pub history_meta: MetricsHistoryMeta,
    #[serde(default)]
    pub tier_deltas_5m: TierDeltas5m,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewCore {
    pub metrics: MetricsSnapshotCore,
    pub health: GatewayHealthView,
    pub prefix_cache: PrefixCacheMetricsSnapshot,
    pub semantic: SemanticConfig,
    pub ops: OverviewOpsMetrics,
    #[serde(default)]
    pub suggestions: Vec<OverviewSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewTimeseriesResponse {
    pub window: String,
    pub points: Vec<TimeSeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewBundle {
    pub metrics: MetricsSnapshot,
    pub health: GatewayHealthView,
    pub prefix_cache: PrefixCacheMetricsSnapshot,
    pub semantic: SemanticConfig,
    pub trace_summary: TraceSummary,
    pub ops: OverviewOpsMetrics,
    #[serde(default)]
    pub suggestions: Vec<OverviewSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerMetricsBucket {
    pub consumer: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainMetricsBucket {
    pub domain: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
    pub cost_saved_usd: f64,
    pub qps_5m: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainPolicy {
    pub domain: String,
    pub monthly_token_budget: u64,
    pub monthly_cost_budget_usd: f64,
    pub min_hit_rate: f64,
    pub enabled: bool,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainDetailBundle {
    pub domain: String,
    pub bucket: DomainMetricsBucket,
    pub consumer_buckets: Vec<ConsumerMetricsBucket>,
    pub history_7d: Vec<TimeSeriesPoint>,
    pub history_30d: Vec<TimeSeriesPoint>,
    pub tier_deltas_5m: TierDeltas5m,
    pub policy: Option<DomainPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefixCacheModelBucket {
    pub model: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefixCacheMetricsSnapshot {
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
    pub by_model: Vec<PrefixCacheModelBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastInvalidateView {
    pub scope: String,
    pub status: String,
    pub at_secs: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateJobView {
    pub scope: String,
    pub phase: String,
    pub error: Option<String>,
    pub started_at_secs: u64,
    pub completed_at_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheOpsView {
    pub fingerprint_version: u32,
    pub fingerprint_normalize: bool,
    pub stream_cache_enabled: bool,
    pub last_invalidate: Option<LastInvalidateView>,
    pub invalidate_all_in_progress: bool,
    pub invalidate_job: Option<InvalidateJobView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheBody {
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheResult {
    pub scope: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfigBody {
    pub version: u32,
    pub normalize_content: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: String,
    pub requests: u64,
    pub tokens: u64,
    pub cache_hits: u64,
    pub avg_latency_ms: f64,
    #[serde(default)]
    pub hit_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_preview: String,
    pub key_full: Option<String>,
    pub active: bool,
    pub rpm_limit: u32,
    pub monthly_token_budget: u64,
    pub tokens_used_this_month: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Vec<String>,
    pub remain_quota: i64,
    pub unlimited_quota: bool,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub inflight: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub rpm_limit: u32,
    pub monthly_token_budget: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Option<Vec<String>>,
    pub remain_quota: Option<i64>,
    pub unlimited_quota: Option<bool>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
    #[serde(default)]
    pub max_concurrent: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineProfileView {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub fallback_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileAdminView {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    pub endpoints: Vec<String>,
    pub tls_sni: String,
    pub key_pool_count: usize,
    pub keys_available: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfilesAdminResponse {
    pub profiles: Vec<UpstreamProfileAdminView>,
    pub default_profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutUpstreamProfileAdminRequest {
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    #[serde(default)]
    pub endpoints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_sni: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeyPoolEntry {
    pub id: String,
    pub preview: String,
    #[serde(default)]
    pub account_id: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileKeysAdminView {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyPoolEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ModelsQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModelSyncQuery {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRuntimeConfig {
    pub pipeline_mode: String,
    pub default_upstream_profile: String,
    pub profiles: Vec<PipelineProfileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: Vec<(String, u64)>,
    #[serde(default)]
    pub consumer_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCacheConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
    #[serde(default)]
    pub enabled: bool,
    pub similarity_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub tcp_keepalive_idle_secs: u64,
    pub tcp_keepalive_interval_secs: u64,
    pub tcp_keepalive_count: usize,
    pub idle_timeout_secs: u64,
    pub h2_ping_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    #[serde(default)]
    pub thinking_mode: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default)]
    pub reasoning_recovery: bool,
    #[serde(default)]
    pub sqlite_cache_enabled: bool,
    #[serde(default)]
    pub sqlite_cache_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingStatus {
    pub backends: Vec<BackendStatus>,
    pub total_backends: usize,
    pub active_backends: usize,
    pub total_requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendStatus {
    pub name: String,
    pub request_count: u64,
    pub healthy: bool,
    #[serde(default)]
    pub addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetricsQuery {
    pub consumer: String,
    #[serde(default = "default_live_window_secs")]
    pub window_secs: u32,
    #[serde(default = "default_live_bucket_secs")]
    pub bucket_secs: u32,
}

pub fn default_live_window_secs() -> u32 {
    300
}

fn default_live_bucket_secs() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetricsResponse {
    pub consumer: String,
    pub window_secs: u32,
    pub bucket_secs: u32,
    pub trace_available: bool,
    pub buckets: Vec<LiveMetricsBucket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_consumers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<LiveRequestPoint>,
    pub summary: LiveMetricsSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetricsBucket {
    pub timestamp_ms: u64,
    pub request_count: u32,
    pub e2e_latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    /// Number of requests in this bucket that had upstream_latency_ms recorded.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub upstream_sample_count: u32,
    /// Number of requests in this bucket that had ttft_ms recorded.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub ttft_sample_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveRequestPoint {
    pub timestamp_ms: u64,
    pub model: String,
    pub e2e_latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetricsSummary {
    pub request_count: u32,
    pub avg_e2e_latency_ms: f64,
    pub avg_upstream_latency_ms: f64,
    pub avg_ttft_ms: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLog {
    pub id: String,
    pub timestamp: String,
    pub model: String,
    pub consumer: String,
    pub latency_ms: u64,
    pub total_tokens: u64,
    pub cache_status: String,
    pub request_payload: String,
    pub response_preview: String,
    /// Input token count (from trace entry detail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output token count (from trace entry detail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Time to first token in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    /// Content length in bytes of the original request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_length: Option<usize>,
    /// Request fingerprint hash for dedup / similarity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id_audit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayHealthView {
    pub healthy: bool,
    pub uptime_secs: u64,
    pub active_keys: u64,
    pub backend_count: usize,
    pub stream_cache_enabled: bool,
    #[serde(default)]
    pub upstream_key_count: u32,
    #[serde(default)]
    pub upstream_keys_available: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDetail {
    pub cache_path: String,
    pub request_payload: String,
    pub response_body: String,
    pub route_backend: String,
    /// Upstream latency in ms (only for trace-path entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    /// Time to first token in ms (only for streaming trace entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    /// Input tokens (only for trace entries with token breakdown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Output tokens (only for trace entries with token breakdown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Request fingerprint hash for dedup / similarity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    /// Semantic cluster id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_cluster: Option<u32>,
}

/// Paginated logs response returned by `GET /api/admin/logs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsPageResponse {
    pub items: Vec<RequestLog>,
    /// Opaque cursor for the next page (`timestamp_ms:request_hash`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// Approximate count of entries within the filter window (may be capped).
    #[serde(default)]
    pub total_in_window: u64,
}

/// Query parameters for `GET /api/admin/logs`.
#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub consumer: Option<String>,
    pub model: Option<String>,
    pub cache_tier: Option<String>,
    pub request_hash: Option<String>,
    pub latency_min: Option<f64>,
    pub latency_max: Option<f64>,
    pub token_min: Option<u64>,
    pub token_max: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCacheConfigRequest {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: Vec<(String, u64)>,
    #[serde(default)]
    pub consumer_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSemanticConfigRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    pub similarity_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConnectionConfigRequest {
    pub tcp_keepalive_idle_secs: u64,
    pub tcp_keepalive_interval_secs: u64,
    pub tcp_keepalive_count: usize,
    pub idle_timeout_secs: u64,
    pub h2_ping_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEndpoint {
    pub name: String,
    pub addr: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutBackendsRequest {
    pub backends: Vec<BackendEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorModelAlias {
    pub model: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorModelsConfig {
    pub aliases: Vec<CursorModelAlias>,
}

pub use crab_control::{
    PatchUpstreamKeyRequest, UpstreamKeyView, UpstreamKeysView,
};

use crab_composition::CompositionSummary;

// ── Composition Debug API types ──────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompositionDebugResponse {
    pub entries: Vec<crab_composition::CompositionDebugEntry>,
    pub total: usize,
}

// ── Composition API types ────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompositionSummaryResponse {
    pub total_entries_in_window: usize,
    pub summary: CompositionSummary,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompositionTrendsResponse {
    pub hours: u32,
    pub points: Vec<HourlyPoint>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HourlyPoint {
    pub timestamp_ms: u64,
    pub request_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    pub base_url: String,
    pub model: String,
    pub endpoints: Vec<String>,
    pub key_pool_count: usize,
    pub gateway_reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_test: Option<crab_control::UpstreamTestResult>,
    /// Deprecated: use key pool. Kept empty for backward compatibility.
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_key_masked: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUpstreamConfigRequest {
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    pub endpoints: Vec<String>,
    /// When set, append these keys to the pool after relay save.
    #[serde(default)]
    pub keys_to_append: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUpstreamConfigResponse {
    pub config: UpstreamConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncResult>,
}

#[derive(Debug, Deserialize)]
pub struct UpstreamTestBody {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDetectResponse {
    pub to_add: Vec<String>,
    pub to_remove: Vec<String>,
    pub unchanged: usize,
    pub upstream_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub profile_id: String,
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub models: Vec<ModelInfo>,
    pub total: usize,
    pub profile_id: Option<String>,
    pub synced_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModelApplyBody {
    pub profile_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamModel {
    pub id: String,
    pub owned_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamModelsResponse {
    pub data: Vec<UpstreamModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayInfo {
    pub base_url: String,
    pub listen_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserIdAuditBreakdown {
    pub injected: usize,
    pub absent: usize,
    pub stripped_client: usize,
    pub mismatch: usize,
    pub not_applicable: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIdModelCount {
    pub model: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserIdProjectCount {
    pub project_id: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekUserIdAudit {
    pub deepseek_requests: usize,
    pub with_upstream_user_id: usize,
    pub without_upstream_user_id: usize,
    pub upstream_user_id_ratio: f64,
    pub missing_project_id: usize,
    pub client_user_id_leaks: usize,
    pub audit_breakdown: UserIdAuditBreakdown,
    pub by_upstream_model: Vec<UserIdModelCount>,
    pub top_project_ids: Vec<UserIdProjectCount>,
    pub isolation_ok: bool,
    pub conclusion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceAnalysis {
    pub total_requests: usize,
    pub unique_requests: usize,
    pub repeat_ratio: f64,
    pub semantic_cluster_ratio: f64,
    pub estimated_zipf_alpha: f64,
    pub estimated_hit_rate: f64,
    pub avg_latency_ms: f64,
    pub avg_prompt_tokens: f64,
    pub cache_hit_ratio: f64,
    pub top_models: Vec<ModelUsage>,
    pub cluster_distribution: Vec<ClusterInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deepseek_user_id: Option<DeepSeekUserIdAudit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInfo {
    pub cluster_id: usize,
    pub count: usize,
    pub percentage: f64,
}
