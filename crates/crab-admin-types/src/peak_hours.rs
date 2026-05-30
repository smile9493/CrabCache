//! Model peak hours analytics types.

use serde::{Deserialize, Serialize};

/// A single aggregated row: model + hour_bucket → counts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPeakHourRow {
    pub model: String,
    /// Hour-aligned unix timestamp in milliseconds.
    pub hour_bucket: i64,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Response for GET /api/admin/analytics/model-peak-hours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPeakHoursResponse {
    pub models: Vec<String>,
    pub data: Vec<ModelPeakHourRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_aggregated_at: Option<String>,
}

/// Request body for DELETE /api/admin/analytics/model-peak-hours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePeakHourRequest {
    pub model: String,
    /// 0 means delete all data for this model.
    #[serde(default)]
    pub hour_bucket: i64,
}
