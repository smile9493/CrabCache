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
