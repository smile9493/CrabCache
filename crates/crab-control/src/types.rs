use crate::validate::KeyQuotaInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const GATEWAY_ADMIN_KEY_HEADER: &str = "x-gateway-admin-key";

/// Required when POST `/v1/cache/invalidate` uses `scope=all`.
pub const CACHE_INVALIDATE_CONFIRM_HEADER: &str = "x-cache-invalidate-confirm";

/// Header value that must accompany `scope=all`.
pub const CACHE_INVALIDATE_CONFIRM_ALL: &str = "all";

/// Client-facing gateway base URL (from Pingora discovery + FRP/OpenResty scan).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientEndpointView {
    pub gateway_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_url_lan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_url_public: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub uptime_secs: u64,
    pub active_keys: u64,
    pub backend_count: usize,
    pub stream_cache_enabled: bool,
    pub upstream_key_count: usize,
    pub upstream_keys_available: usize,
    /// Best-effort global request rate estimate (may be 0 when idle/unknown).
    #[serde(default)]
    pub global_rps_estimate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
}

/// Detailed response from `GET /v1/ready` including subsystem health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayReadyDetail {
    pub ready: bool,
    /// `"ok"` or `"unavailable"`.
    pub redis: String,
    /// `"ok"` (L2 cache loaded), `"disabled"` (semantic not configured), or `"unavailable"`.
    #[serde(default = "default_l2_disabled")]
    pub l2: String,
}

fn default_l2_disabled() -> String {
    "disabled".to_string()
}

/// Runtime upstream relay target (hot-reloadable via Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamRelayConfigView {
    pub base_url: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutUpstreamRelayConfigRequest {
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// When set, replaces Ketama peer list and TLS SNI derived from `base_url`.
    #[serde(default)]
    pub endpoints: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_sni: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeyView {
    pub id: String,
    pub preview: String,
    #[serde(default)]
    pub account_id: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
    /// Key priority: 0 = highest, higher values = lower priority.
    #[serde(default)]
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeysView {
    pub keys: Vec<UpstreamKeyView>,
}

/// Per-key upstream model catalog (Codex OAuth: `GET /backend-api/codex/models`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeyModelsEntry {
    pub key_id: String,
    #[serde(default)]
    pub account_id: String,
    pub enabled: bool,
    pub ok: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<KeyQuotaInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileKeysModelsView {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyModelsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeyInput {
    #[serde(default)]
    pub id: String,
    pub secret: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub account_id: String,
    /// Key priority: 0 = highest (default), higher values = lower priority.
    #[serde(default)]
    pub priority: u32,
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
pub struct PatchUpstreamKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeySpec {
    pub id: String,
    pub name: String,
    pub key_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_full: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub rpm_limit: u32,
    #[serde(default)]
    pub inflight: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGatewayKeyRequest {
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm_limit: Option<u32>,
}

fn default_enabled() -> bool {
    true
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGatewayKeyResponse {
    pub id: String,
    pub name: String,
    pub key_full: String,
    pub key_preview: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub rpm_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchGatewayKeyRequest {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpm_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainPolicySpec {
    pub domain: String,
    pub monthly_token_budget: u64,
    pub monthly_cost_budget_usd: f64,
    pub min_hit_rate: f64,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutDomainPoliciesRequest {
    pub policies: Vec<DomainPolicySpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainUsageEntry {
    pub domain: String,
    pub tokens: u64,
    pub spend_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainUsageResponse {
    pub usage: Vec<DomainUsageEntry>,
    pub month: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutDomainUsageRequest {
    pub usage: Vec<DomainUsageEntry>,
    pub month: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtlConfigView {
    pub default_ttl_secs: u64,
    pub model_overrides: HashMap<String, u64>,
    pub consumer_overrides: HashMap<String, u64>,
    /// Combined overrides keyed by `"consumer:model"`.
    #[serde(default)]
    pub consumer_model_overrides: HashMap<String, u64>,
    /// Grace period (seconds) for stale-while-revalidate.
    #[serde(default)]
    pub stale_while_revalidate_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutTtlConfigRequest {
    pub default_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: HashMap<String, u64>,
    #[serde(default)]
    pub consumer_overrides: HashMap<String, u64>,
    /// Combined overrides keyed by `"consumer:model"`.
    #[serde(default)]
    pub consumer_model_overrides: HashMap<String, u64>,
    /// Grace period (seconds) for stale-while-revalidate.
    #[serde(default)]
    pub stale_while_revalidate_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendSpec {
    pub name: String,
    pub addr: String,
    pub weight: u32,
    pub tls_sni: String,
    #[serde(default = "default_backend_healthy")]
    pub healthy: bool,
    #[serde(default)]
    pub last_check_ms: u64,
    #[serde(default)]
    pub latency_ms: u64,
}

fn default_backend_healthy() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingBackendsView {
    pub backends: Vec<BackendSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutBackendsRequest {
    pub endpoints: Vec<String>,
    #[serde(default = "default_weight")]
    pub default_weight: u32,
    #[serde(default = "default_tls_sni")]
    pub tls_sni: String,
}

fn default_weight() -> u32 {
    1
}

fn default_tls_sni() -> String {
    "api.deepseek.com".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCacheConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineProfileView {
    pub id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fallback_model: String,
}

/// A single pipeline rule view (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRuleView {
    pub name: String,
    pub priority: u32,
    pub pipeline: String,
    #[serde(rename = "match")]
    pub match_conditions: PipelineRuleMatchView,
}

/// Match conditions for a pipeline rule view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRuleMatchView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_pattern: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wire_format: Option<Vec<String>>,
}

/// Pipeline rules configuration (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRulesConfigView {
    pub rules: Vec<PipelineRuleView>,
}

/// Pipeline test request (simulate rule matching).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineTestRequest {
    pub model: String,
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub wire_format: Option<String>,
}

/// Pipeline test response (matched rule result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineTestResponse {
    pub matched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
}

/// Read-only upstream profile summary (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileView {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    #[serde(default)]
    pub endpoints: Vec<String>,
    pub tls_sni: String,
    pub key_pool_count: usize,
    pub keys_available: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    /// Profile ID to try when this profile's upstream fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_profile_id: Option<String>,
    /// Maximum number of fallback attempts per request.
    #[serde(default = "default_fallback_max_retries")]
    pub fallback_max_retries: u32,
}

fn default_fallback_max_retries() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfilesResponse {
    pub profiles: Vec<UpstreamProfileView>,
    pub default_profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutUpstreamProfileRequest {
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    #[serde(default)]
    pub endpoints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_sni: Option<String>,
    #[serde(default = "default_weight")]
    pub default_weight: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_max_retries: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileKeysView {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyView>,
}

/// Admin-only export of profile key pool secrets (Management API reconciliation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileKeysExport {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutUpstreamProfileKeysRequest {
    pub keys: Vec<UpstreamKeyInput>,
    #[serde(default)]
    pub mode: UpstreamKeysPutMode,
}

/// Hot-reloadable global pipeline selection (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRuntimeConfigView {
    pub pipeline_mode: String,
    pub default_upstream_profile: String,
    pub profiles: Vec<PipelineProfileView>,
}

/// Cursor-visible model alias table (hot-reloadable).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CursorModelAliasView {
    pub upstream: String,
    #[serde(default = "default_cursor_alias_pipeline")]
    pub pipeline: String,
}

fn default_cursor_alias_pipeline() -> String {
    "cursor_deepseek_v4".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CursorModelsConfigView {
    #[serde(default)]
    pub force_deepseek_profile_for_aliases: bool,
    #[serde(default)]
    pub synthetic_models_enabled: bool,
    #[serde(default)]
    pub aliases: HashMap<String, CursorModelAliasView>,
}

/// Hot-reloadable upstream connection tuning (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRuntimeView {
    pub tcp_keepalive_idle_secs: u64,
    pub tcp_keepalive_interval_secs: u64,
    pub tcp_keepalive_count: usize,
    pub idle_timeout_secs: u64,
    pub h2_ping_interval_secs: u64,
    /// When true, force upstream HTTP/1.1 ALPN.
    #[serde(default)]
    pub upstream_force_http1: bool,
    /// When true, send `Connection: close` and avoid pooling idle upstream sockets.
    #[serde(default)]
    pub upstream_disable_keepalive: bool,
    /// Optional TLS curve override in OpenSSL group list syntax. Empty string uses defaults.
    #[serde(default)]
    pub upstream_tls_curves: String,
    /// Max seconds waiting for upstream response body bytes (0 = no limit).
    #[serde(default = "default_upstream_request_timeout_secs")]
    pub upstream_request_timeout_secs: u64,
    /// Max seconds per write when sending large request bodies upstream.
    #[serde(default = "default_upstream_write_timeout_secs")]
    pub upstream_write_timeout_secs: u64,
    /// TCP+TLS connect timeout to upstream (seconds).
    #[serde(default = "default_upstream_connection_timeout_secs")]
    pub upstream_connection_timeout_secs: u64,
}

fn default_upstream_request_timeout_secs() -> u64 {
    300
}
fn default_upstream_write_timeout_secs() -> u64 {
    300
}
fn default_upstream_connection_timeout_secs() -> u64 {
    60
}

/// Trace logging runtime config (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceLoggingConfigView {
    pub max_lines: u64,
    pub max_files: u64,
    pub max_payload_bytes: usize,
    pub max_response_preview_bytes: usize,
}

/// Raw capture runtime config (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCaptureConfigView {
    pub enabled: bool,
    /// Samle rate 0.0–1.0.
    pub sample_rate: f64,
    pub mask_api_keys: bool,
    pub sample_always_on_error: bool,
}

/// Hot-reloadable features config (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfigView {
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
    // --- P1-1: Multi-factor routing ---
    /// Route policy used when choosing among healthy upstream backends.
    #[serde(default)]
    pub backend_route_strategy: String,
    /// Enable per-backend runtime load checks when selecting upstream peers.
    #[serde(default)]
    pub backend_load_aware_routing_enabled: bool,
    /// Enable per-backend in-flight concurrency limits.
    #[serde(default)]
    pub backend_concurrency_limit_enabled: bool,
    /// Default max in-flight requests per backend.
    #[serde(default = "default_max_inflight_per_backend")]
    pub default_max_inflight_per_backend: usize,
    /// Mark a backend overloaded when observed prefill exceeds this threshold (0 disables).
    #[serde(default)]
    pub backend_prefill_overload_threshold_ms: u64,
    /// Cooldown window after a backend crosses the prefill threshold.
    #[serde(default = "default_backend_overload_cooldown_ms")]
    pub backend_overload_cooldown_ms: u64,
    /// Weights for multi-factor weighted Ketama routing.
    #[serde(default)]
    pub score_weights: ScoreWeightsView,
    // --- P1-2: Quota Preflight ---
    /// Pre-flight backend health checks before upstream.
    #[serde(default)]
    pub preflight: PreflightView,
}

fn default_max_inflight_per_backend() -> usize {
    8
}

fn default_backend_overload_cooldown_ms() -> u64 {
    30_000
}

/// Multi-factor score weights (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreWeightsView {
    pub health: f64,
    pub latency_inv: f64,
    pub load_inv: f64,
    pub affinity_hit: f64,
    pub rate_429_inv: f64,
}

impl Default for ScoreWeightsView {
    fn default() -> Self {
        Self {
            health: 0.30,
            latency_inv: 0.25,
            load_inv: 0.20,
            affinity_hit: 0.15,
            rate_429_inv: 0.10,
        }
    }
}

/// Quota preflight config (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightView {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_preflight_cooldown_ms")]
    pub cooldown_ms: u64,
    #[serde(default = "default_preflight_max_consecutive_429")]
    pub max_consecutive_429: u32,
    #[serde(default = "default_preflight_skip_threshold")]
    pub skip_threshold: f64,
}

fn default_preflight_cooldown_ms() -> u64 {
    60_000
}
fn default_preflight_max_consecutive_429() -> u32 {
    3
}
fn default_preflight_skip_threshold() -> f64 {
    0.5
}

impl Default for PreflightView {
    fn default() -> Self {
        Self {
            enabled: false,
            cooldown_ms: 60_000,
            max_consecutive_429: 3,
            skip_threshold: 0.5,
        }
    }
}

/// Hot-reloadable pricing config (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfigView {
    pub default_input_price_per_million: f64,
    pub default_output_price_per_million: f64,
    /// Keyed by model name: e.g. `{ "deepseek-chat": { "input": 0.27, "output": 1.10 } }`.
    #[serde(default)]
    pub model_overrides: std::collections::HashMap<String, ModelPricingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricingView {
    pub input: f64,
    pub output: f64,
}

/// Runtime limits config (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfigView {
    /// Max chat completion request body in bytes.
    pub max_request_body_bytes: usize,
    /// Max concurrent in-flight chat requests.
    pub max_concurrent_requests: usize,
    /// Allow Legacy api_key as client Bearer auth.
    pub legacy_api_key_as_client_auth: bool,
    /// CORS response headers on all paths.
    pub cors_enabled: bool,
}

/// Hot-reloadable L2 semantic cache settings (Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticRuntimeView {
    pub enabled: bool,
    pub similarity_threshold: f64,
    pub min_query_chars: usize,
    pub max_query_chars: usize,
    pub embed_only_on_exact_miss: bool,
}

/// Hot-reloadable reasoning / Cursor compatibility settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningRuntimeConfigView {
    pub thinking_mode: String,
    pub reasoning_effort: String,
    pub missing_reasoning_strategy: String,
    pub display_reasoning: bool,
    pub collapsible_reasoning: bool,
    /// True when `display_reasoning` changed on this PUT; clear L0/L1 or bump fingerprint.
    #[serde(default)]
    pub cache_invalidate_recommended: bool,
    /// `sqlite` or `redis`; read-only (requires restart to change).
    #[serde(default)]
    pub storage_backend: Option<String>,
    /// Path to the SQLite cache file (read-only).
    #[serde(default)]
    pub cache_db_path: Option<String>,
    /// Masked Redis URL for display (read-only).
    #[serde(default)]
    pub redis_url_masked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearReasoningCacheResponse {
    pub deleted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Cache invalidation scope. Use `all` only with `x-cache-invalidate-confirm: all`.
/// Prefix: `prefix:{namespace}`; single key: raw cache key string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheRequest {
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheResponse {
    pub scope: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateJobSnapshot {
    pub scope: String,
    pub phase: String,
    pub error: Option<String>,
    pub started_at_secs: u64,
    pub completed_at_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheStatus {
    pub all_in_progress: bool,
    pub job: Option<InvalidateJobSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfigRequest {
    pub version: u32,
    #[serde(default = "default_fingerprint_normalize")]
    pub normalize_content: bool,
}

fn default_fingerprint_normalize() -> bool {
    true
}

/// Per-backend routing info for the Dashboard (enriched with circuit breaker state).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileRoutingBackendView {
    pub name: String,
    pub addr: String,
    pub weight: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_sni: Option<String>,
    pub healthy: bool,
    pub last_check_ms: u64,
    pub latency_ms: u64,
    pub circuit_state: String,
    pub consecutive_failures: u32,
    pub half_open_successes: u32,
}

/// Circuit breaker config snapshot (read-only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerView {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_ms: u64,
}

/// Key pool summary for the routing view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingKeyPoolSummary {
    pub total: usize,
    pub available: usize,
}

/// Full routing view for a profile (Dashboard "路由与健康" tab).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRoutingView {
    pub profile_id: String,
    pub backends: Vec<ProfileRoutingBackendView>,
    pub circuit_breaker: CircuitBreakerView,
    pub key_pool: RoutingKeyPoolSummary,
}

/// Lightweight routing summary for the Overview card (default profile only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingSummaryView {
    pub backends_healthy: usize,
    pub backends_total: usize,
    pub backends_unhealthy: usize,
    pub upstream_keys_available: usize,
    pub upstream_keys_total: usize,
    pub profile_id: String,
}
