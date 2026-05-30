use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionConfig {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConnectionConfigRequest {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineProfileView {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub fallback_model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineRuntimeConfig {
    pub pipeline_mode: String,
    pub default_upstream_profile: String,
    pub profiles: Vec<PipelineProfileView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackendStatus {
    pub name: String,
    pub request_count: u64,
    pub healthy: bool,
    #[serde(default)]
    pub addr: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingStatus {
    pub backends: Vec<BackendStatus>,
    pub total_backends: usize,
    pub active_backends: usize,
    pub total_requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEndpoint {
    pub name: String,
    pub addr: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_request_body_bytes: usize,
    pub max_concurrent_requests: usize,
    pub legacy_api_key_as_client_auth: bool,
    pub cors_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutBackendsRequest {
    pub backends: Vec<BackendEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorModelAlias {
    pub model: String,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorModelsConfig {
    pub aliases: Vec<CursorModelAlias>,
}
