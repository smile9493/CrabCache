use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsQuery {
    pub consumer: String,
    #[serde(default = "default_live_window_secs")]
    pub window_secs: u32,
    #[serde(default = "default_live_bucket_secs")]
    pub bucket_secs: u32,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMetricsBucket {
    pub timestamp_ms: u64,
    pub request_count: u32,
    pub e2e_latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub upstream_sample_count: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub ttft_sample_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub cache_hit_count: u32,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRequestPoint {
    pub timestamp_ms: u64,
    pub model: String,
    pub e2e_latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
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
    pub avg_ttft_ms: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_hit_ratio: f64,
}
