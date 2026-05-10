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
    pub latency_l0_ms: f64,
    pub latency_l1_ms: f64,
    pub latency_l2_ms: f64,
    pub latency_upstream_ms: f64,
    pub active_keys: u64,
    pub uptime_hours: u64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConfig {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestDetail {
    pub cache_path: String,
    pub request_payload: String,
    pub response_body: String,
    pub route_backend: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCacheConfigRequest {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSemanticConfigRequest {
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
pub struct UpstreamConfig {
    pub base_url: String,
    pub api_key: String,
    pub api_key_masked: String,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUpstreamConfigRequest {
    pub base_url: String,
    pub api_key: Option<String>,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
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
    pub synced_at: Option<String>,
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
