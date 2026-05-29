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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_key_id: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_kind: Option<String>,
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

// ── Log Management Types ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogDiskUsage {
    pub trace_bytes: u64,
    pub trace_file_count: usize,
    pub debug_trace_bytes: u64,
    pub debug_trace_file_count: usize,
    pub capture_index_bytes: u64,
    pub capture_body_bytes: u64,
    pub capture_body_file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Max age in hours. Files older than this are deleted. 0 = disabled.
    pub max_age_hours: u32,
    /// Max total disk usage in MB. Oldest files pruned first. 0 = disabled.
    pub max_disk_mb: u32,
    /// Max number of rotated trace files to keep (overrides gateway's max_files).
    pub max_trace_files: usize,
    /// Max number of capture body files to keep.
    pub max_capture_body_files: usize,
    /// PG trace_logs/request_logs retention in days. 0 = disabled.
    #[serde(default)]
    pub pg_retention_days: u64,
    /// Compress rotated JSONL files to .gz before deletion. Default: false.
    #[serde(default)]
    pub compress_before_delete: bool,
    /// Retention days for compressed .gz files. 0 = disabled.
    #[serde(default)]
    pub compressed_retention_days: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_hours: 168,
            max_disk_mb: 500,
            max_trace_files: 20,
            max_capture_body_files: 5000,
            pg_retention_days: 7,
            compress_before_delete: false,
            compressed_retention_days: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClearLogsRequest {
    pub target: ClearTarget,
    /// Only clear files older than this many hours. None = clear all matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_than_hours: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearTarget {
    /// All rotated trace files (not the active one).
    TraceRotated,
    /// All rotated debug trace files.
    DebugRotated,
    /// All capture files (index + bodies).
    Capture,
    /// Everything except active files.
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClearLogsResponse {
    pub deleted_files: Vec<String>,
    pub freed_bytes: u64,
}
