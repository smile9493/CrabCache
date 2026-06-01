use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamProfileAdminView {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    pub endpoints: Vec<String>,
    pub tls_sni: String,
    pub key_pool_count: usize,
    pub keys_available: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_profile_id: Option<String>,
    #[serde(default = "default_fallback_max_retries")]
    pub fallback_max_retries: u32,
}

fn default_fallback_max_retries() -> u32 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_max_retries: Option<u32>,
}

/// Alias for profile key pool entries (same wire shape as [`UpstreamKeyView`]).
pub type UpstreamKeyPoolEntry = UpstreamKeyView;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamProfileKeysAdminView {
    pub profile_id: String,
    pub keys: Vec<UpstreamKeyView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamKeyView {
    pub id: String,
    pub preview: String,
    #[serde(default)]
    pub account_id: String,
    pub enabled: bool,
    pub inflight: usize,
    pub cooldown_remaining_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<KeyQuotaInfo>,
    /// Key priority: 0 = highest, higher values = lower priority.
    #[serde(default)]
    pub priority: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub account_id: String,
    /// Key priority: 0 = highest (default), higher values = lower priority.
    #[serde(default)]
    pub priority: u32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u32>,
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

/// Balance / quota info returned by per-key upstream testing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyQuotaInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_granted: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_used: Option<f64>,
    /// ChatGPT plan (`plus`, `free`, …) from WHAM / JWT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    /// Codex primary window used % (typically 5h).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_used_percent: Option<f64>,
    /// Codex secondary window used % (typically 7d).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_used_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_reset_after_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_reset_after_secs: Option<u64>,
    /// Primary (5h) window absolute reset time as Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_reset_at_secs: Option<i64>,
    /// Secondary (weekly) window absolute reset time as Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_reset_at_secs: Option<i64>,
    /// Full list of Codex quota windows (5h, weekly, code_review, additional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_windows: Option<Vec<CodexQuotaWindowItem>>,
}

/// A single Codex quota window for dashboard display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexQuotaWindowItem {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at_secs: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamTestResult {
    pub ok: bool,
    pub status_code: u16,
    pub latency_ms: u64,
    pub model_count: Option<usize>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<KeyQuotaInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamTestBody {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamConfig {
    pub base_url: String,
    pub model: String,
    pub endpoints: Vec<String>,
    pub key_pool_count: usize,
    pub gateway_reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateUpstreamConfigResponse {
    pub config: UpstreamConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDetectResponse {
    pub to_add: Vec<String>,
    pub to_remove: Vec<String>,
    pub unchanged: usize,
    pub upstream_total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    #[serde(default)]
    pub profile_id: String,
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
    /// ChatGPT account UUIDs that expose this model (Codex multi-key pools).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_ids: Vec<String>,
    /// Upstream key pool IDs that expose this model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub models: Vec<ModelInfo>,
    pub total: usize,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelApplyBody {
    pub profile_id: String,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamModel {
    pub id: String,
    pub owned_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamModelsResponse {
    pub data: Vec<UpstreamModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncResult {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayInfo {
    pub base_url: String,
    pub listen_addr: String,
}

/// Per-backend routing info for the Dashboard (enriched with circuit breaker state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircuitBreakerView {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_ms: u64,
}

/// Key pool summary for the routing view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingKeyPoolSummary {
    pub total: usize,
    pub available: usize,
}

/// Full routing view for a profile (Dashboard "路由与健康" tab).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileRoutingView {
    pub profile_id: String,
    pub backends: Vec<ProfileRoutingBackendView>,
    pub circuit_breaker: CircuitBreakerView,
    pub key_pool: RoutingKeyPoolSummary,
}

/// Lightweight routing summary for the Overview card (default profile only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingSummaryView {
    pub backends_healthy: usize,
    pub backends_total: usize,
    pub circuit_open_count: usize,
    pub upstream_keys_available: usize,
    pub upstream_keys_total: usize,
    pub profile_id: String,
}
