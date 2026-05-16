use crab_control::GatewayAdminClient;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Extended metadata for an API key (quota/UI fields not stored on the gateway).
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    pub id: String,
    pub token: String,
    pub rpm_limit: u64,
    pub monthly_token_limit: u64,
    pub current_rpm: u64,
    pub tokens_this_month: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub expired_at: Option<u64>,
    pub model_limits: Vec<String>,
    pub remain_quota: i64,
    pub unlimited_quota: bool,
}

pub struct AppState {
    pub start_time: u64,
    pub upstream_api_key: String,
    pub gateway: GatewayAdminClient,
    /// API key metadata indexed by key id (gateway-assigned).
    pub keys_meta: DashMap<String, KeyMetadata>,
    pub request_logs: RwLock<Vec<StoredRequestLog>>,
    pub cache_config: RwLock<StoredCacheConfig>,
    pub semantic_config: RwLock<StoredSemanticConfig>,
    pub connection_config: RwLock<StoredConnectionConfig>,
    pub upstream_config: RwLock<StoredUpstreamConfig>,
    pub models: RwLock<StoredModelList>,
    pub backends: RwLock<Vec<StoredBackend>>,
    pub metrics: RwLock<StoredMetrics>,
    pub trace_entries: RwLock<Vec<StoredTraceEntry>>,
}

#[derive(Debug, Clone)]
pub struct StoredRequestLog {
    pub id: String,
    pub timestamp: u64,
    pub model: String,
    pub consumer: String,
    pub duration_ms: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_status: String,
    pub cache_tier: String,
    pub status_code: u16,
    pub conversation_id: String,
    pub request_payload: serde_json::Value,
    pub response_body: String,
    pub cache_path: Vec<String>,
    pub route_backend: String,
}

#[derive(Debug, Clone)]
pub struct StoredCacheConfig {
    pub l0_max_capacity: u64,
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
    pub model_overrides: Vec<(String, u64)>,
    pub consumer_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone)]
pub struct StoredSemanticConfig {
    pub enabled: bool,
    pub similarity_threshold: f32,
    pub ttl_secs: u64,
    pub collection_size: u64,
}

#[derive(Debug, Clone)]
pub struct StoredBackend {
    pub name: String,
    pub addr: String,
    pub weight: u32,
    pub healthy: bool,
    pub request_count: u64,
}

#[derive(Debug, Clone)]
pub struct StoredUpstreamConfig {
    pub base_url: String,
    pub api_key: String,
    pub endpoints: Vec<String>,
}

impl Default for StoredUpstreamConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".to_string(),
            api_key: String::new(),
            endpoints: vec!["api.deepseek.com:443".to_string()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredConnectionConfig {
    pub tcp_keepalive_idle_secs: u64,
    pub tcp_keepalive_interval_secs: u64,
    pub tcp_keepalive_count: usize,
    pub idle_timeout_secs: u64,
    pub h2_ping_interval_secs: u64,
}

#[derive(Debug, Clone)]
pub struct StoredModel {
    pub id: String,
    pub owned_by: String,
    pub context_length: Option<u64>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct StoredModelList {
    pub models: Vec<StoredModel>,
    pub synced_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredMetrics {
    pub total_requests: u64,
    pub l0_hits: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub cache_misses: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub l0_latency_sum_ms: f64,
    pub l0_latency_count: u64,
    pub l1_latency_sum_ms: f64,
    pub l1_latency_count: u64,
    pub l2_latency_sum_ms: f64,
    pub l2_latency_count: u64,
    pub upstream_latency_sum_ms: f64,
    pub upstream_latency_count: u64,
    pub ttft_sum_ms: f64,
    pub ttft_count: u64,
}

#[derive(Debug, Clone)]
pub struct StoredTraceEntry {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub content_length: usize,
    pub semantic_cluster: usize,
    pub conversation_id: Option<String>,
    pub model: String,
    pub prompt_tokens: usize,
    pub latency_ms: f64,
    pub cache_hit: bool,
}

impl Default for StoredMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            l0_hits: 0,
            l1_hits: 0,
            l2_hits: 0,
            cache_misses: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            l0_latency_sum_ms: 0.0,
            l0_latency_count: 0,
            l1_latency_sum_ms: 0.0,
            l1_latency_count: 0,
            l2_latency_sum_ms: 0.0,
            l2_latency_count: 0,
            upstream_latency_sum_ms: 0.0,
            upstream_latency_count: 0,
            ttft_sum_ms: 0.0,
            ttft_count: 0,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            start_time: now,
            upstream_api_key: std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
            gateway: GatewayAdminClient::from_env(),
            keys_meta: DashMap::new(),
            request_logs: RwLock::new(Vec::new()),
            cache_config: RwLock::new(StoredCacheConfig {
                l0_max_capacity: 10000,
                l0_ttl_secs: 3600,
                l1_ttl_secs: 3600,
                default_ttl_secs: 3600,
                model_overrides: vec![
                    ("deepseek-chat".into(), 7200),
                    ("deepseek-coder".into(), 300),
                ],
                consumer_overrides: vec![("reporting-job".into(), 1800)],
            }),
            semantic_config: RwLock::new(StoredSemanticConfig {
                enabled: true,
                similarity_threshold: 0.95,
                ttl_secs: 86400,
                collection_size: 0,
            }),
            connection_config: RwLock::new(StoredConnectionConfig {
                tcp_keepalive_idle_secs: 60,
                tcp_keepalive_interval_secs: 10,
                tcp_keepalive_count: 3,
                idle_timeout_secs: 90,
                h2_ping_interval_secs: 30,
            }),
            upstream_config: RwLock::new(StoredUpstreamConfig::default()),
            models: RwLock::new(StoredModelList {
                models: Vec::new(),
                synced_at: None,
            }),
            backends: RwLock::new(Vec::new()),
            metrics: RwLock::new(StoredMetrics::default()),
            trace_entries: RwLock::new(Vec::new()),
        }
    }
}