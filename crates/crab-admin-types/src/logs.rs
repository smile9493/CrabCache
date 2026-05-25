use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_length: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id_audit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestDetail {
    pub cache_path: String,
    pub request_payload: String,
    pub response_body: String,
    pub route_backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_cluster: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogsPageResponse {
    pub items: Vec<RequestLog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    #[serde(default)]
    pub total_in_window: u64,
}
