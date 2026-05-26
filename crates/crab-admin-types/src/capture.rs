pub use crab_capture::{
    PacketStructureSummary, RawCaptureEntry, StructureDiff, format_beijing_datetime_ms,
    format_beijing_datetime_secs_ms, format_beijing_hour_label, normalize_epoch_ms,
};

use serde::{Deserialize, Serialize};

/// Response for `GET /api/admin/capture/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureListResponse {
    pub entries: Vec<RawCaptureEntry>,
    pub total: usize,
}

/// Response for `GET /api/admin/capture/{request_id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureDetailResponse {
    pub entry: RawCaptureEntry,
    /// Full client body JSON (if file exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_body: Option<String>,
    /// Full upstream body JSON (if file exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_body: Option<String>,
}

/// Response for `GET /api/admin/capture/stats`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureStatsResponse {
    pub hours: u32,
    pub total_captures: usize,
    /// Mean delta_bytes across all captures in window.
    pub avg_delta_bytes: f64,
    /// Percentage of captures that have reasoning content in upstream.
    pub reasoning_injection_rate: f64,
    /// P99 message_count across captures.
    pub message_count_p99: u32,
    /// Percentage of captures where `has_thinking_markup` is true in upstream.
    pub thinking_markup_rate: f64,
    /// Mean upstream_body_bytes.
    pub avg_upstream_body_bytes: f64,
    /// Mean client_body_bytes.
    pub avg_client_body_bytes: f64,
}
