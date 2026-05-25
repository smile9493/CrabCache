use serde::{Deserialize, Serialize};

pub use crab_composition::{CompositionDebugEntry, CompositionSummary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionDebugResponse {
    pub entries: Vec<CompositionDebugEntry>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionSummaryResponse {
    pub total_entries_in_window: usize,
    pub summary: CompositionSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionTrendsResponse {
    pub hours: u32,
    pub points: Vec<HourlyPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyPoint {
    pub timestamp_ms: u64,
    pub request_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarItem {
    pub label: String,
    pub value: f64,
}
