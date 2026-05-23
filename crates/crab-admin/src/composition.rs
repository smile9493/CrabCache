use crate::trace_log::{load_trace_entries_async, trace_log_path};
use crate::types::*;
use axum::{
    Json,
    extract::{Query, State},
};
use crab_composition::{RequestComposition, aggregate_composition};
use serde::Deserialize;
use std::sync::Arc;

/// Query parameters for `GET /api/admin/composition/summary`.
#[derive(Debug, Deserialize)]
pub struct CompositionSummaryQuery {
    #[serde(default = "default_composition_hours")]
    pub hours: u32,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub consumer: Option<String>,
}

fn default_composition_hours() -> u32 {
    24
}

/// `GET /api/admin/composition/summary` — aggregate composition data from trace logs.
pub async fn get_composition_summary(
    State(_state): State<Arc<crate::state::AppState>>,
    Query(query): Query<CompositionSummaryQuery>,
) -> Json<CompositionSummaryResponse> {
    let path = trace_log_path();
    let entries = load_trace_entries_async(&path, query.hours).await;

    // Filter entries that have composition data, applying optional filters.
    let composed: Vec<(RequestComposition, f64, u64)> = entries
        .into_iter()
        .filter_map(|e| {
            let comp = e.composition.clone()?;
            // Apply project_id filter.
            if let Some(ref pid) = query.project_id {
                if !pid.is_empty() && comp.project_id.as_deref() != Some(pid.as_str()) {
                    return None;
                }
            }
            // Apply consumer filter.
            if let Some(ref consumer) = query.consumer {
                if !consumer.is_empty() && comp.consumer != *consumer {
                    return None;
                }
            }
            let latency = e.latency_ms;
            let tokens = e.resolved_input_tokens() + e.resolved_output_tokens();
            Some((comp, latency, tokens))
        })
        .collect();

    let total_matching = composed.len();
    let summary = aggregate_composition(&composed);

    Json(CompositionSummaryResponse {
        total_entries_in_window: total_matching,
        summary,
    })
}

/// `GET /api/admin/composition/trends` — hourly request volume for composed requests.
pub async fn get_composition_trends(
    State(_state): State<Arc<crate::state::AppState>>,
) -> Json<CompositionTrendsResponse> {
    let path = trace_log_path();
    let entries = load_trace_entries_async(&path, 24).await;

    // Bucket by hour.
    use std::collections::BTreeMap;
    let mut hourly: BTreeMap<u64, u32> = BTreeMap::new();

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let window_start = now_ms.saturating_sub(24 * 3_600_000);

    for e in &entries {
        if e.composition.is_none() {
            continue;
        }
        if e.timestamp_ms < window_start {
            continue;
        }
        // Round down to hour boundary.
        let hour_ms = (e.timestamp_ms / 3_600_000) * 3_600_000;
        *hourly.entry(hour_ms).or_insert(0) += 1;
    }

    let points: Vec<HourlyPoint> = hourly
        .into_iter()
        .map(|(ts, count)| HourlyPoint {
            timestamp_ms: ts,
            request_count: count,
        })
        .collect();

    Json(CompositionTrendsResponse {
        hours: 24,
        points,
    })
}
