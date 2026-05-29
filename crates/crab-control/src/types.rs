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
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub account_id: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileKeysView {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyView>,
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
