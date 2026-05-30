use crate::types::*;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use crab_capture::{RawCaptureEntry, format_beijing_datetime_ms};
use serde::Deserialize;
use std::sync::Arc;

/// Resolve the raw capture directory path.
pub(crate) fn raw_capture_dir() -> String {
    std::env::var("CRABCACHE_RAW_CAPTURE_DIR")
        .unwrap_or_else(|_| "/var/log/crabcache/raw_capture".to_string())
}

/// Load index entries from `index.jsonl` tail, filtered by time window.
async fn load_capture_entries_async(dir: &str, hours: u32, limit: usize) -> Vec<RawCaptureEntry> {
    let path = format!("{}/index.jsonl", dir);
    tokio::task::spawn_blocking(move || {
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
        if start > 0 && file.seek(SeekFrom::Start(start as u64)).is_err() {
            return Vec::new();
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

        let mut entries: Vec<RawCaptureEntry> = lines
            .iter()
            .rev() // newest first
            .filter_map(|line| {
                if line.is_empty() {
                    return None;
                }
                let entry: RawCaptureEntry = match serde_json::from_str(line) {
                    Ok(e) => e,
                    Err(_) => return None,
                };
                if hours > 0 && entry.timestamp_ms < cutoff {
                    return None;
                }
                Some(entry)
            })
            .take(limit)
            .collect();

        entries.reverse(); // oldest first for display
        entries
    })
    .await
    .unwrap_or_default()
}

/// Query parameters for `GET /api/admin/capture/list`.
#[derive(Debug, Deserialize)]
pub struct CaptureListQuery {
    #[serde(default = "default_capture_hours")]
    pub hours: u32,
    #[serde(default = "default_capture_limit")]
    pub limit: usize,
    #[serde(default)]
    pub consumer: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub request_hash: Option<String>,
    #[serde(default)]
    pub session_fingerprint: Option<String>,
    #[serde(default)]
    pub backend_name: Option<String>,
    #[serde(default)]
    pub client_key_fingerprint: Option<String>,
    #[serde(default)]
    pub affinity_kind: Option<String>,
    #[serde(default)]
    pub client_wire_api: Option<String>,
    #[serde(default)]
    pub client_path_suffix: Option<String>,
}

fn default_capture_hours() -> u32 {
    24
}
fn default_capture_limit() -> usize {
    100
}

fn enrich_capture_timestamps(entries: &mut [RawCaptureEntry]) {
    for e in entries {
        e.timestamp_beijing = Some(format_beijing_datetime_ms(e.timestamp_ms));
    }
}

/// `GET /api/admin/capture/list` — list recent captures.
pub async fn get_capture_list(
    State(_state): State<Arc<crate::state::AppState>>,
    Query(query): Query<CaptureListQuery>,
) -> Json<CaptureListResponse> {
    let dir = raw_capture_dir();
    let limit = query.limit.min(1000);
    let entries = load_capture_entries_async(&dir, query.hours, limit).await;

    let mut filtered: Vec<RawCaptureEntry> = entries
        .into_iter()
        .filter(|e| {
            if let Some(ref consumer) = query.consumer
                && !consumer.is_empty()
                && e.consumer.as_deref() != Some(consumer.as_str())
            {
                return false;
            }
            if let Some(ref pid) = query.project_id
                && !pid.is_empty()
                && e.project_id.as_deref() != Some(pid.as_str())
            {
                return false;
            }
            if let Some(ref rh) = query.request_hash
                && !rh.is_empty()
                && e.request_hash.as_deref() != Some(rh.as_str())
            {
                return false;
            }
            if let Some(ref sf) = query.session_fingerprint
                && !sf.is_empty()
                && e.session_fingerprint.as_deref() != Some(sf.as_str())
            {
                return false;
            }
            if let Some(ref bn) = query.backend_name
                && !bn.is_empty()
                && e.backend_name.as_deref() != Some(bn.as_str())
            {
                return false;
            }
            if let Some(ref ck) = query.client_key_fingerprint
                && !ck.is_empty()
                && e.client_key_fingerprint.as_deref() != Some(ck.as_str())
            {
                return false;
            }
            if let Some(ref ak) = query.affinity_kind
                && !ak.is_empty()
                && e.affinity_kind.as_deref() != Some(ak.as_str())
            {
                return false;
            }
            if let Some(ref wire) = query.client_wire_api
                && !wire.is_empty()
            {
                let matches = e.client_wire_api.as_deref() == Some(wire.as_str())
                    || (e.client_wire_api.is_none()
                        && wire == "responses"
                        && e.client_path_suffix.as_deref() == Some("/v1/responses"));
                if !matches {
                    return false;
                }
            }
            if let Some(ref path) = query.client_path_suffix
                && !path.is_empty()
                && e.client_path_suffix.as_deref() != Some(path.as_str())
            {
                return false;
            }
            true
        })
        .collect();

    enrich_capture_timestamps(&mut filtered);
    let total = filtered.len();
    Json(CaptureListResponse {
        entries: filtered,
        total,
    })
}

/// `GET /api/admin/capture/{request_id}` — load capture detail with body files.
pub async fn get_capture_detail(
    State(_state): State<Arc<crate::state::AppState>>,
    Path(request_id): Path<String>,
) -> Result<Json<CaptureDetailResponse>, (axum::http::StatusCode, String)> {
    let dir = raw_capture_dir();
    let index_path = format!("{}/index.jsonl", dir);

    // Find the index entry.
    let entry = tokio::task::spawn_blocking({
        let index_path = index_path.clone();
        let request_id = request_id.clone();
        move || {
            use std::fs::File;
            use std::io::{BufRead, BufReader};

            let file = match File::open(&index_path) {
                Ok(f) => f,
                Err(_) => return None,
            };
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                if line.is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<RawCaptureEntry>(&line)
                    && entry.request_id == request_id
                {
                    return Some(entry);
                }
            }
            None
        }
    })
    .await
    .unwrap_or(None);

    let mut entry = match entry {
        Some(e) => e,
        None => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                format!("Capture {} not found", request_id),
            ));
        }
    };
    enrich_capture_timestamps(std::slice::from_mut(&mut entry));

    // Load body files.
    let client_body = load_body_file(&dir, &request_id, "client").await;
    let upstream_body = load_body_file(&dir, &request_id, "upstream").await;

    Ok(Json(CaptureDetailResponse {
        entry,
        client_body,
        upstream_body,
    }))
}

async fn load_body_file(dir: &str, request_id: &str, suffix: &str) -> Option<String> {
    let path = format!("{}/bodies/{}.{}.json", dir, request_id, suffix);
    tokio::task::spawn_blocking(move || std::fs::read_to_string(&path).ok())
        .await
        .unwrap_or(None)
}

/// `GET /api/admin/capture/stats` — aggregate capture statistics.
pub async fn get_capture_stats(
    State(_state): State<Arc<crate::state::AppState>>,
    Query(query): Query<CaptureStatsQuery>,
) -> Json<CaptureStatsResponse> {
    let dir = raw_capture_dir();
    let entries = load_capture_entries_async(&dir, query.hours, 10000).await;

    let total = entries.len();
    if total == 0 {
        return Json(CaptureStatsResponse {
            hours: query.hours,
            total_captures: 0,
            avg_delta_bytes: 0.0,
            reasoning_injection_rate: 0.0,
            message_count_p99: 0,
            thinking_markup_rate: 0.0,
            avg_upstream_body_bytes: 0.0,
            avg_client_body_bytes: 0.0,
        });
    }

    let sum_delta: i64 = entries.iter().map(|e| e.delta_bytes).sum();
    let reasoning_count = entries
        .iter()
        .filter(|e| e.structure.reasoning_was_injected)
        .count();
    let thinking_count = entries
        .iter()
        .filter(|e| {
            e.structure.client.has_thinking_markup || e.structure.upstream.has_thinking_markup
        })
        .count();
    let sum_upstream_bytes: u64 = entries.iter().map(|e| e.upstream_body_bytes).sum();
    let sum_client_bytes: u64 = entries.iter().map(|e| e.client_body_bytes).sum();
    let upstream_nonzero: Vec<u64> = entries
        .iter()
        .filter(|e| e.upstream_body_bytes > 0)
        .map(|e| e.upstream_body_bytes)
        .collect();

    // P99 message count (client body; upstream may be empty on older captures).
    let mut msg_counts: Vec<u32> = entries
        .iter()
        .map(|e| {
            e.structure
                .client
                .message_count
                .max(e.structure.upstream.message_count)
        })
        .collect();
    msg_counts.sort_unstable();
    let p99_idx = ((total as f64) * 0.99).ceil() as usize;
    let p99 = msg_counts
        .get(p99_idx.saturating_sub(1))
        .copied()
        .unwrap_or(0);

    Json(CaptureStatsResponse {
        hours: query.hours,
        total_captures: total,
        avg_delta_bytes: sum_delta as f64 / total as f64,
        reasoning_injection_rate: reasoning_count as f64 / total as f64,
        message_count_p99: p99,
        thinking_markup_rate: thinking_count as f64 / total as f64,
        avg_upstream_body_bytes: if upstream_nonzero.is_empty() {
            sum_upstream_bytes as f64 / total as f64
        } else {
            upstream_nonzero.iter().sum::<u64>() as f64 / upstream_nonzero.len() as f64
        },
        avg_client_body_bytes: sum_client_bytes as f64 / total as f64,
    })
}

#[derive(Debug, Deserialize)]
pub struct CaptureStatsQuery {
    #[serde(default = "default_capture_hours")]
    pub hours: u32,
}
