//! Dashboard wire types shared with Admin BFF (`crab-admin-types`).

pub use crab_admin_types::*;

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
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KeyRoutingBackend {
    pub backend_name: String,
    pub request_count: u64,
    pub affinity_kind: Option<String>,
    pub avg_latency_ms: f64,
    pub cache_hit_rate: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AffinityMigration {
    pub session_fingerprint: String,
    pub from_backend: String,
    pub to_backend: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KeyRoutingResponse {
    pub key_id: String,
    pub window_secs: u32,
    pub backends: Vec<KeyRoutingBackend>,
    pub migrations: Vec<AffinityMigration>,
    pub prefix_break_count: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KeyConcurrencyEntry {
    pub request_hash: String,
    pub timestamp_ms: u64,
    pub model: String,
    pub consumer: Option<String>,
    pub affinity_key: Option<String>,
    pub affinity_kind: Option<String>,
    pub backend_name: Option<String>,
    pub session_fingerprint: Option<String>,
    pub is_coalesced: bool,
    pub latency_ms: f64,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KeyConcurrencyResponse {
    pub key_id: String,
    pub window_secs: u32,
    pub total_requests: usize,
    pub concurrent_peak: u32,
    pub active_now: usize,
    pub entries: Vec<KeyConcurrencyEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SessionEvent {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub model: String,
    pub consumer: Option<String>,
    pub affinity_key: Option<String>,
    pub backend_name: Option<String>,
    pub is_coalesced: bool,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
    pub latency_ms: f64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SessionTimelineResponse {
    pub session_fingerprint: String,
    pub window_secs: u32,
    pub total_events: usize,
    pub unique_keys: Vec<String>,
    pub events: Vec<SessionEvent>,
}
