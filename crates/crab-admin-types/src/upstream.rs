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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
