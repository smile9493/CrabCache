use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GatewayHealth {
    pub healthy: bool,
    #[serde(default)]
    pub uptime_secs: u64,
    #[serde(default)]
    pub active_keys: u64,
    #[serde(default)]
    pub backend_count: usize,
    #[serde(default)]
    pub stream_cache_enabled: bool,
    #[serde(default)]
    pub upstream_key_count: u32,
    #[serde(default)]
    pub upstream_keys_available: u32,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub primary_ip: Option<String>,
    pub all_ips: Vec<NetworkInterface>,
    pub gateway_url: String,
    pub gateway_url_lan: Option<String>,
    #[serde(default)]
    pub gateway_url_openresty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub ip: String,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub uptime_secs: u64,
    pub hourly_stats: Vec<TimeSeriesPoint>,
    pub daily_stats: Vec<TimeSeriesPoint>,
    pub weekly_stats: Vec<TimeSeriesPoint>,
    pub monthly_stats: Vec<TimeSeriesPoint>,
    #[serde(default)]
    pub semantic_hits: u64,
    #[serde(default)]
    pub semantic_rejected: u64,
    #[serde(default)]
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetricsHistoryMeta {
    pub sample_count: usize,
    pub oldest_sample_at_secs: u64,
    pub sampling_interval_secs: u64,
    #[serde(default)]
    pub gateway_counter_reset: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TierDeltas5m {
    pub l0: u64,
    pub l1: u64,
    pub l2: u64,
    pub miss: u64,
    pub coalesced: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceSummary {
    pub hours: u32,
    pub total_requests: usize,
    pub cache_hit_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewSuggestion {
    pub severity: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub semantic_hits: u64,
    #[serde(default)]
    pub semantic_rejected: u64,
    #[serde(default)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverviewCore {
    pub metrics: MetricsSnapshotCore,
    pub health: GatewayHealth,
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
    pub health: GatewayHealth,
    pub prefix_cache: PrefixCacheMetricsSnapshot,
    pub semantic: SemanticConfig,
    pub trace_summary: TraceSummary,
    pub ops: OverviewOpsMetrics,
    #[serde(default)]
    pub suggestions: Vec<OverviewSuggestion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsumerMetricsBucket {
    pub consumer: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrefixCacheMetricsSnapshot {
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
    pub by_model: Vec<PrefixCacheModelBucket>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrefixCacheModelBucket {
    pub model: String,
    pub hit_tokens: u64,
    pub miss_tokens: u64,
    pub hit_ratio: f64,
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
    #[serde(default)]
    pub invalidate_all_in_progress: bool,
    #[serde(default)]
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
pub struct StreamCacheToggle {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
pub struct UpstreamProfileKeysAdminView {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRuntimeConfig {
    pub pipeline_mode: String,
    pub default_upstream_profile: String,
    pub profiles: Vec<PipelineProfileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct CacheConfig {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: Vec<(String, u64)>,
    #[serde(default)]
    pub consumer_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDetail {
    pub cache_path: String,
    pub request_payload: String,
    pub response_body: String,
    pub route_backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_cluster: Option<u32>,
}

/// Query parameters for `GET /api/admin/logs` (dashboard → admin).
#[derive(Debug, Clone, Default)]
pub struct LogsFilterQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub model: Option<String>,
    pub consumer: Option<String>,
    pub cache_tier: Option<String>,
    pub request_hash: Option<String>,
    pub latency_min: Option<f64>,
    pub latency_max: Option<f64>,
    pub token_min: Option<u64>,
    pub token_max: Option<u64>,
}

/// Paginated logs response from `GET /api/admin/logs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsPageResponse {
    pub items: Vec<RequestLog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    #[serde(default)]
    pub total_in_window: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCacheConfigRequest {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: Vec<(String, u64)>,
    #[serde(default)]
    pub consumer_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct UpstreamKeyView {
    pub id: String,
    pub preview: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeysView {
    pub keys: Vec<UpstreamKeyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeyInput {
    #[serde(default)]
    pub id: String,
    pub secret: String,
    #[serde(default = "default_key_enabled")]
    pub enabled: bool,
}

fn default_key_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchUpstreamKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamTestResult {
    pub ok: bool,
    pub status_code: u16,
    pub latency_ms: u64,
    pub model_count: Option<usize>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamTestBody {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    pub base_url: String,
    pub model: String,
    pub endpoints: Vec<String>,
    pub key_pool_count: usize,
    pub gateway_reachable: bool,
    pub last_test: Option<UpstreamTestResult>,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_key_masked: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUpstreamConfigRequest {
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    pub endpoints: Vec<String>,
    #[serde(default)]
    pub keys_to_append: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUpstreamConfigResponse {
    pub config: UpstreamConfig,
    pub sync: Option<SyncResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamKeysPutMode {
    #[default]
    Replace,
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutUpstreamKeysRequest {
    pub keys: Vec<UpstreamKeyInput>,
    #[serde(default)]
    pub mode: UpstreamKeysPutMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDetectResponse {
    pub to_add: Vec<String>,
    pub to_remove: Vec<String>,
    pub unchanged: usize,
    pub upstream_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelApplyBody {
    pub profile_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    #[serde(default)]
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
    #[serde(default)]
    pub profile_id: Option<String>,
    pub synced_at: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetricsResponse {
    pub consumer: String,
    pub window_secs: u32,
    pub bucket_secs: u32,
    pub trace_available: bool,
    pub buckets: Vec<LiveMetricsBucket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_consumers: Vec<String>,
    #[serde(default)]
    pub latest: Option<LiveRequestPoint>,
    pub summary: LiveMetricsSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetricsBucket {
    pub timestamp_ms: u64,
    pub request_count: u32,
    pub e2e_latency_ms: f64,
    #[serde(default)]
    pub upstream_latency_ms: Option<f64>,
    #[serde(default)]
    pub ttft_ms: Option<f64>,
    #[serde(default)]
    pub upstream_sample_count: u32,
    #[serde(default)]
    pub ttft_sample_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveRequestPoint {
    pub timestamp_ms: u64,
    pub model: String,
    pub e2e_latency_ms: f64,
    pub upstream_latency_ms: Option<f64>,
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
pub struct CursorModelAlias {
    pub model: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorModelsConfig {
    pub aliases: Vec<CursorModelAlias>,
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

// ── System / Update ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub published_at: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemVersion {
    pub current_version: String,
    pub latest: Option<ReleaseInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release: Option<ReleaseInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemUpdateResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
}

// ── Composition API types ────────────────────────────────────────

/// Summary of aggregated composition data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionSummaryResponse {
    pub total_entries_in_window: usize,
    pub summary: CompositionSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompositionSummary {
    pub total_entries: usize,
    pub tenant_count: usize,
    pub consumer_count: usize,
    pub model_distribution: Vec<NamedCount>,
    pub project_distribution: Vec<NamedCount>,
    pub consumer_distribution: Vec<NamedCount>,
    pub tool_count_histogram: Vec<BucketCount>,
    pub message_count_histogram: Vec<BucketCount>,
    pub component_rates: Vec<ComponentRate>,
    pub avg_latency_ms: f64,
    pub avg_total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketCount {
    pub bucket_label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRate {
    pub component: String,
    pub present_count: usize,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionTrendsResponse {
    pub hours: u32,
    pub points: Vec<HourlyPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyPoint {
    pub timestamp_ms: u64,
    pub request_count: u32,
}

// ── Chart types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarItem {
    pub label: String,
    pub value: f64,
}

// ── Composition Debug types ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionDebugEntry {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub consumer: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionDebugResponse {
    pub entries: Vec<CompositionDebugEntry>,
    pub total: usize,
}

// ── Infra (container/host monitoring) ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraSnapshot {
    pub containers: Vec<ContainerStats>,
    pub host_disks: Vec<HostDisk>,
    #[serde(default)]
    pub volumes: Vec<VolumeDisk>,
    pub collected_at: u64,
    pub compose_project: String,
    #[serde(default)]
    pub docker_connected: bool,
    #[serde(default)]
    pub collection_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub name: String,
    pub container_id: String,
    pub cpu_percent: Option<f64>,
    pub mem_usage_bytes: u64,
    pub mem_limit_bytes: u64,
    pub mem_percent: f64,
    pub net_rx_bps: Option<f64>,
    pub net_tx_bps: Option<f64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDisk {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeDisk {
    pub volume_name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraStatus {
    pub docker_connected: bool,
    pub compose_project: String,
    #[serde(default)]
    pub poll_hint_secs: u64,
    #[serde(default)]
    pub history_sample_count: usize,
    pub last_collected_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraChartPoint {
    pub timestamp: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraTimeseriesResponse {
    pub window: String,
    pub container_id: String,
    pub cpu: Vec<InfraChartPoint>,
    pub memory: Vec<InfraChartPoint>,
    pub net_rx: Vec<InfraChartPoint>,
    pub net_tx: Vec<InfraChartPoint>,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestAccepted {
    pub job_id: String,
    pub upload_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestJobView {
    pub job_id: String,
    pub status: String,
    pub direction: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub error: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub upload_token: Option<String>,
    #[serde(default)]
    pub upload_bytes: Option<u64>,
}
