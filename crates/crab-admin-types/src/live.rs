use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsQuery {
    pub consumer: String,
    #[serde(default = "default_live_window_secs")]
    pub window_secs: u32,
    #[serde(default = "default_live_bucket_secs")]
    pub bucket_secs: u32,
    /// v2: optional group-by dimensions (model, key_id, cache_hit, backend_name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub group_by: Vec<String>,
    /// v2: optional key_id filter.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key_id: String,
    /// v2: optional session_fingerprint filter.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session_fingerprint: String,
}

pub fn default_live_window_secs() -> u32 {
    300
}

fn default_live_bucket_secs() -> u32 {
    5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsResponse {
    pub consumer: String,
    pub window_secs: u32,
    pub bucket_secs: u32,
    pub trace_available: bool,
    pub buckets: Vec<LiveMetricsBucket>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_consumers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<LiveRequestPoint>,
    pub summary: LiveMetricsSummary,
    /// v2: optional grouped series (populated when `group_by` is non-empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<LiveMetricsSeries>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsBucket {
    pub timestamp_ms: u64,
    pub request_count: u32,
    pub e2e_latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_header_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub upstream_sample_count: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub pre_header_sample_count: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub ttft_sample_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub cache_hit_count: u32,
    /// v2: OHLC for input tokens within this bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_ohlc: Option<Ohlc>,
    /// v2: OHLC for output tokens within this bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens_ohlc: Option<Ohlc>,
    /// v2: max concurrent inflight requests observed in this bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_inflight: Option<u32>,
    /// Most frequent model name in this bucket.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub top_model: String,
    /// Most frequent upstream API key ID in this bucket.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub top_upstream_key: String,
    /// Most frequent downstream key (consumer) in this bucket.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub top_downstream_key: String,
    /// Most frequent client IP in this bucket.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub top_client_ip: String,
    /// Geolocation of the most frequent client IP (city, country).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub top_client_ip_location: String,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

/// OHLC (Open/High/Low/Close) for token counts within a time bucket.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ohlc {
    pub open: u64,
    pub high: u64,
    pub low: u64,
    pub close: u64,
}

/// A single grouped series with its own bucket timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsSeries {
    /// Human-readable label for this series (e.g. model name, key id).
    pub label: String,
    /// The dimension this series was grouped by (e.g. "model", "key_id").
    pub group_key: String,
    /// CSS color hint for the frontend.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color_hint: String,
    /// Per-bucket aggregated data for this series.
    pub buckets: Vec<LiveMetricsBucket>,
    /// Summary for this series.
    pub summary: LiveMetricsSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRequestPoint {
    pub timestamp_ms: u64,
    pub model: String,
    pub e2e_latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_header_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsSummary {
    pub request_count: u32,
    pub avg_e2e_latency_ms: f64,
    pub avg_upstream_latency_ms: f64,
    /// Average time from request start to upstream response headers (`e2e - upstream`).
    pub avg_pre_header_ms: f64,
    pub avg_ttft_ms: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_hit_ratio: f64,
}

/// Color palette for series lines (matches the CSS variables used in the frontend).
pub const SERIES_COLORS: &[&str] = &[
    "var(--accent-primary)",
    "var(--info)",
    "var(--success)",
    "var(--warning)",
    "var(--error)",
    "var(--cc-purple)",
];
