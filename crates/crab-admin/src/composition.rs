use crate::trace_log::{load_trace_entries_async, trace_log_path};
use crate::types::*;
use axum::{
    Json,
    extract::{Query, State},
};
use crab_composition::{CompositionDebugEntry, RequestComposition, aggregate_composition};
use serde::Deserialize;
use std::sync::Arc;

/// Load composition debug entries from the debug JSONL file with a time window filter.
async fn load_debug_entries_async(path: &str, hours: u32) -> Vec<CompositionDebugEntry> {
    let path = path.to_string();
    match tokio::task::spawn_blocking(move || {
        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};
        use std::path::Path;

        let path_ref = Path::new(&path);
        if !path_ref.is_file() {
            return Vec::new();
        }

        let mut file = match File::open(path_ref) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let len = match file.metadata() {
            Ok(m) => m.len() as usize,
            Err(_) => return Vec::new(),
        };

        let read_len = len.min(32 * 1024 * 1024);
        let truncated = len > read_len;
        let start = if truncated {
            len.saturating_sub(read_len)
        } else {
            0
        };
        if start > 0 {
            if file.seek(SeekFrom::Start(start as u64)).is_err() {
                return Vec::new();
            }
        }

        let mut buf = vec![0u8; read_len];
        if file.read_exact(&mut buf).is_err() {
            return Vec::new();
        }

        let text = String::from_utf8_lossy(&buf);
        let mut lines: Vec<&str> = text.lines().collect();
        if truncated && !lines.is_empty() {
            lines.remove(0);
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let cutoff = now_ms.saturating_sub(u64::from(hours) * 3_600_000);

        lines
            .iter()
            .filter_map(|line| {
                if line.is_empty() {
                    return None;
                }
                let entry: CompositionDebugEntry = match serde_json::from_str(line) {
                    Ok(e) => e,
                    Err(_) => return None,
                };
                if hours > 0 && entry.timestamp_ms < cutoff {
                    return None;
                }
                Some(entry)
            })
            .collect()
    })
    .await
    {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, "debug entry spawn_blocking join failed");
            Vec::new()
        }
    }
}

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

    Json(CompositionTrendsResponse { hours: 24, points })
}

// ── Composition Debug ────────────────────────────────────────────

/// Auto-derive the composition debug log path from the trace log path.
pub fn composition_debug_path() -> String {
    std::env::var("CRABCACHE_COMPOSITION_DEBUG_PATH").unwrap_or_else(|_| {
        let trace_path = trace_log_path();
        if trace_path.ends_with("trace.jsonl") {
            trace_path.replace("trace.jsonl", "trace-debug.jsonl")
        } else {
            format!("{}-debug", trace_path)
        }
    })
}

/// Query parameters for `GET /api/admin/composition/debug`.
#[derive(Debug, Deserialize)]
pub struct CompositionDebugQuery {
    #[serde(default = "default_debug_hours")]
    pub hours: u32,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub request_hash: Option<String>,
    #[serde(default)]
    pub consumer: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

fn default_debug_hours() -> u32 {
    24
}

/// `GET /api/admin/composition/debug` — list debug entries with optional filters.
pub async fn get_composition_debug(
    State(_state): State<Arc<crate::state::AppState>>,
    Query(query): Query<CompositionDebugQuery>,
) -> Json<crate::types::CompositionDebugResponse> {
    let path = composition_debug_path();
    let entries = load_debug_entries_async(&path, query.hours).await;
    let limit = query.limit.unwrap_or(100).min(1000);

    let mut filtered: Vec<CompositionDebugEntry> = entries
        .into_iter()
        .filter(|e| {
            if let Some(ref rh) = query.request_hash {
                if !rh.is_empty() && e.request_hash != *rh {
                    return false;
                }
            }
            if let Some(ref c) = query.consumer {
                if !c.is_empty() && e.consumer != *c {
                    return false;
                }
            }
            if let Some(ref pid) = query.project_id {
                if !pid.is_empty() && e.project_id.as_deref() != Some(pid.as_str()) {
                    return false;
                }
            }
            true
        })
        .collect();
    let total = filtered.len();
    filtered.truncate(limit);
    Json(crate::types::CompositionDebugResponse {
        entries: filtered,
        total,
    })
}
