use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const GATEWAY_ADMIN_KEY_HEADER: &str = "x-gateway-admin-key";

/// Required when POST `/v1/cache/invalidate` uses `scope=all`.
pub const CACHE_INVALIDATE_CONFIRM_HEADER: &str = "x-cache-invalidate-confirm";

/// Header value that must accompany `scope=all`.
pub const CACHE_INVALIDATE_CONFIRM_ALL: &str = "all";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub uptime_secs: u64,
    pub active_keys: u64,
    pub backend_count: usize,
    pub stream_cache_enabled: bool,
    pub upstream_key_count: usize,
    pub upstream_keys_available: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
}

/// Runtime upstream relay target (hot-reloadable via Management API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamRelayConfigView {
    pub base_url: String,
    pub model: String,
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
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
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
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
}

fn default_enabled() -> bool {
    true
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
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
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
    pub pipeline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile: Option<String>,
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
pub struct TtlConfigView {
    pub default_ttl_secs: u64,
    pub model_overrides: HashMap<String, u64>,
    pub consumer_overrides: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutTtlConfigRequest {
    pub default_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: HashMap<String, u64>,
    #[serde(default)]
    pub consumer_overrides: HashMap<String, u64>,
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

/// Hot-reloadable reasoning / Cursor compatibility settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningRuntimeConfigView {
    pub thinking_mode: String,
    pub reasoning_effort: String,
    pub missing_reasoning_strategy: String,
    pub display_reasoning: bool,
    pub collapsible_reasoning: bool,
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
