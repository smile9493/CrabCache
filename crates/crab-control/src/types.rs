use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const GATEWAY_ADMIN_KEY_HEADER: &str = "x-gateway-admin-key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStatus {
    pub uptime_secs: u64,
    pub active_keys: u64,
    pub backend_count: usize,
    pub stream_cache_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeySpec {
    pub id: String,
    pub name: String,
    pub key_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_full: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGatewayKeyRequest {
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchGatewayKeyRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
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
pub struct ErrorResponse {
    pub error: String,
}
