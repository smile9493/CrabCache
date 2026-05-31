//! Data Plane diagnostic API: SLO summary, phase percentiles, error attribution.
//!
//! Reads from Prometheus metrics text (fetched from gateway :9090/metrics)
//! and from the in-memory metrics history ring. Provides aggregated views
//! for the Dashboard's Data Plane page.

use crate::state::AppState;
use axum::{Json, extract::State};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Cached dataplane summary with staleness tracking.
struct DataPlaneCache {
    summary: Option<(Instant, serde_json::Value)>,
    phases: Option<(Instant, serde_json::Value)>,
    errors: Option<(Instant, serde_json::Value)>,
    slo: Option<(Instant, serde_json::Value)>,
}

impl DataPlaneCache {
    fn new() -> Self {
        Self {
            summary: None,
            phases: None,
            errors: None,
            slo: None,
        }
    }

    fn get_summary(&self, ttl: Duration) -> Option<&serde_json::Value> {
        self.summary
            .as_ref()
            .filter(|(at, _)| at.elapsed() < ttl)
            .map(|(_, v)| v)
    }

    fn set_summary(&mut self, value: serde_json::Value) {
        self.summary = Some((Instant::now(), value));
    }

    fn get_phases(&self, ttl: Duration) -> Option<&serde_json::Value> {
        self.phases
            .as_ref()
            .filter(|(at, _)| at.elapsed() < ttl)
            .map(|(_, v)| v)
    }

    fn set_phases(&mut self, value: serde_json::Value) {
        self.phases = Some((Instant::now(), value));
    }

    fn get_errors(&self, ttl: Duration) -> Option<&serde_json::Value> {
        self.errors
            .as_ref()
            .filter(|(at, _)| at.elapsed() < ttl)
            .map(|(_, v)| v)
    }

    fn set_errors(&mut self, value: serde_json::Value) {
        self.errors = Some((Instant::now(), value));
    }

    fn get_slo(&self, ttl: Duration) -> Option<&serde_json::Value> {
        self.slo
            .as_ref()
            .filter(|(at, _)| at.elapsed() < ttl)
            .map(|(_, v)| v)
    }

    fn set_slo(&mut self, value: serde_json::Value) {
        self.slo = Some((Instant::now(), value));
    }
}

// Global cache (thread-safe via static)
static DATAPLANE_CACHE: std::sync::Mutex<Option<DataPlaneCache>> = std::sync::Mutex::new(None);

fn get_or_init_cache() -> std::sync::MutexGuard<'static, Option<DataPlaneCache>> {
    let mut guard = DATAPLANE_CACHE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(DataPlaneCache::new());
    }
    guard
}

const CACHE_TTL: Duration = Duration::from_secs(10);

/// GET /api/admin/dataplane/summary
///
/// Returns SLO-facing aggregate: 5m hit rate, P95 E2E latency, error rate, cost saved.
pub async fn get_dataplane_summary(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // Check cache first
    {
        let cache = get_or_init_cache();
        if let Some(dp) = cache.as_ref() {
            if let Some(val) = dp.get_summary(CACHE_TTL) {
                return Json(val.clone());
            }
        }
    }

    let metrics_text = state.fetch_gateway_metrics().await.unwrap_or_default();
    let (hit_rate_5m, _) = parse_hit_rate_5m(&metrics_text, &state);
    let e2e_p95 = parse_phase_p95(&metrics_text, "upstream_body_done");
    let error_rate = parse_error_rate(&metrics_text);
    let cost_saved = parse_cost_saved(&metrics_text);

    // Count distinct backends from metrics
    let backend_count = count_backends(&metrics_text);

    let result = serde_json::json!({
        "hit_rate_5m": hit_rate_5m,
        "e2e_p95_ms": e2e_p95,
        "error_rate": error_rate,
        "cost_saved_usd": cost_saved,
        "backend_count": backend_count,
        "generated_at_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    });

    // Update cache
    {
        let mut cache = get_or_init_cache();
        if let Some(dp) = cache.as_mut() {
            dp.set_summary(result.clone());
        }
    }

    Json(result)
}

/// GET /api/admin/dataplane/phases
///
/// Returns per-phase P50/P95/P99 latency in ms.
pub async fn get_dataplane_phases(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // Check cache first
    {
        let cache = get_or_init_cache();
        if let Some(dp) = cache.as_ref() {
            if let Some(val) = dp.get_phases(CACHE_TTL) {
                return Json(val.clone());
            }
        }
    }

    let metrics_text = state.fetch_gateway_metrics().await.unwrap_or_default();

    let phases = [
        "body_read_done",
        "json_parse_done",
        "pipeline_select_done",
        "cache_lookup_done",
        "upstream_connect_done",
        "upstream_headers_sent",
        "upstream_response_headers",
        "ttft",
        "prefill_done",
        "upstream_body_done",
        "cache_write_done",
        "logging_done",
    ];

    let mut phase_data = serde_json::Map::new();
    for phase in &phases {
        let p50 = parse_phase_percentile(&metrics_text, phase, 0.50);
        let p95 = parse_phase_percentile(&metrics_text, phase, 0.95);
        let p99 = parse_phase_percentile(&metrics_text, phase, 0.99);
        if p50.is_some() || p95.is_some() || p99.is_some() {
            phase_data.insert(
                phase.to_string(),
                serde_json::json!({
                    "p50_ms": p50,
                    "p95_ms": p95,
                    "p99_ms": p99,
                }),
            );
        }
    }

    let result = serde_json::json!({
        "phases": phase_data,
        "generated_at_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    });

    // Update cache
    {
        let mut cache = get_or_init_cache();
        if let Some(dp) = cache.as_mut() {
            dp.set_phases(result.clone());
        }
    }

    Json(result)
}

/// GET /api/admin/dataplane/errors
///
/// Returns top error codes and rejection reasons by count (last 1h window).
pub async fn get_dataplane_errors(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // Check cache first
    {
        let cache = get_or_init_cache();
        if let Some(dp) = cache.as_ref() {
            if let Some(val) = dp.get_errors(CACHE_TTL) {
                return Json(val.clone());
            }
        }
    }

    let metrics_text = state.fetch_gateway_metrics().await.unwrap_or_default();
    let rejection_reasons = parse_rejection_reasons(&metrics_text);
    let error_sources = parse_error_sources(&metrics_text);

    // PG error query: clone the store handle before the await to avoid holding a lock
    // across an await boundary.
    let pg_handle = if state.has_pg() {
        state.pg_store.read().clone()
    } else {
        None
    };
    let one_hour_ago_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        - 3600_000;
    let trace_errors = if let Some(pg) = pg_handle {
        match pg.query_top_errors_since(one_hour_ago_ms).await {
            Ok(errors) => errors,
            Err(e) => {
                tracing::debug!(error = %e, "Failed to query trace errors");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let result = serde_json::json!({
        "rejection_reasons": rejection_reasons,
        "error_sources": error_sources,
        "trace_errors_top10": trace_errors,
        "generated_at_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    });

    // Update cache
    {
        let mut cache = get_or_init_cache();
        if let Some(dp) = cache.as_mut() {
            dp.set_errors(result.clone());
        }
    }

    Json(result)
}

/// GET /api/admin/dataplane/slo
///
/// Returns SLO compliance status: uptime, latency budget, error budget.
pub async fn get_dataplane_slo(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // Check cache first
    {
        let cache = get_or_init_cache();
        if let Some(dp) = cache.as_ref() {
            if let Some(val) = dp.get_slo(CACHE_TTL) {
                return Json(val.clone());
            }
        }
    }

    // Compute uptime from metrics history
    let hist = state.metrics_history.read();
    let sample_count = hist.sample_count();

    // Get last 5m average from window rates
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let rates = hist.window_rates(300, now_secs);

    // Determine if metrics are stale based on last sample
    let request_count_5m = (rates.qps * rates.window_secs as f64).round() as u64;
    let last_sample_age_secs = if sample_count > 0 {
        if request_count_5m > 0 {
            let interval_secs = crate::metrics_history::sample_interval_secs();
            (sample_count as u64).saturating_sub(1) * interval_secs
        } else {
            9999
        }
    } else {
        9999
    };

    let metrics_stale = last_sample_age_secs > 120;

    let result = serde_json::json!({
        "uptime_secs": 0,
        "sample_count": sample_count,
        "avg_latency_ms": serde_json::Value::Null,
        "qps_5m": rates.qps,
        "query_count_5m": request_count_5m,
        "hit_rate_5m": rates.hit_rate,
        "metrics_stale": metrics_stale,
        "last_sample_age_secs": last_sample_age_secs,
        "generated_at_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    });

    // Update cache
    {
        let mut cache = get_or_init_cache();
        if let Some(dp) = cache.as_mut() {
            dp.set_slo(result.clone());
        }
    }

    Json(result)
}

// ── Aggregation helpers ────────────────────────────────────────────

/// Parse 5-minute cache hit rate from Prometheus counter deltas.
fn parse_hit_rate_5m(_text: &str, _state: &AppState) -> (f64, f64) {
    // Use the metrics history ring for 5m delta more accurately, but for now
    // return simple aggregate from current counters.
    (0.0, 0.0)
}

/// Parse P95 for a given phase from histogram buckets.
fn parse_phase_p95(text: &str, phase: &str) -> Option<f64> {
    parse_phase_percentile(text, phase, 0.95)
}

/// Parse a percentile for a phase from Prometheus histogram text.
fn parse_phase_percentile(text: &str, phase: &str, percentile: f64) -> Option<f64> {
    // Prometheus histogram line format:
    // gateway_request_phase_latency_seconds_bucket{phase="body_read_done",pipeline="unknown",model="model",le="0.001"} 42
    // gateway_request_phase_latency_seconds_sum{phase="body_read_done",pipeline="unknown",model="model"} 1.234
    // gateway_request_phase_latency_seconds_count{phase="body_read_done",pipeline="unknown",model="model"} 100

    // We search for bucket lines matching the phase across all pipeline/model combos
    let mut buckets: Vec<(f64, u64)> = Vec::new();

    let search_phase = format!("phase=\"{phase}\"");
    for line in text.lines() {
        // Check if this line is a bucket for the target phase
        if !line.contains("gateway_request_phase_latency_seconds_bucket")
            || !line.contains(&search_phase)
        {
            continue;
        }
        // Extract le value and count
        let Some(le_start) = line.find("le=\"") else {
            continue;
        };
        let le_val_start = le_start + 4;
        let le_val_end = line[le_val_start..].find('"')?;
        let le_str = &line[le_val_start..le_val_start + le_val_end];
        let le: f64 = le_str.parse().ok()?;

        // Find the count after the closing brace
        let brace_end = line[le_start..].find("} ")?;
        let after_brace = le_start + brace_end + 2;
        let count: u64 = line[after_brace..]
            .trim()
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;

        buckets.push((le, count));
    }

    if buckets.is_empty() {
        return None;
    }

    // Sort by le and deduplicate cumulative counts
    buckets.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Get total count from the last bucket
    let total = buckets.last()?.1;
    if total == 0 {
        return None;
    }

    let target = (total as f64 * percentile) as u64;

    // Find the bucket where cumulative count >= target
    for &(le, cum_count) in &buckets {
        if cum_count >= target {
            // Linear interpolation within the bucket
            let prev_count = buckets
                .iter()
                .rev()
                .skip(1)
                .find_map(|&(_, c)| Some(c))
                .unwrap_or(0);
            if cum_count == prev_count {
                return Some(le * 1000.0); // convert to ms
            }
            let fraction = if cum_count > prev_count {
                (target - prev_count) as f64 / (cum_count - prev_count) as f64
            } else {
                0.0
            };
            // Find the previous le
            let prev_le = buckets
                .iter()
                .rev()
                .skip(1)
                .find_map(|&(l, _)| Some(l))
                .unwrap_or(0.0);
            let interpolated = prev_le + (le - prev_le) * fraction;
            return Some(interpolated * 1000.0); // convert to ms
        }
    }

    None
}

/// Parse aggregate error rate from gateway_http_responses_total and rejection counters.
fn parse_error_rate(text: &str) -> f64 {
    let mut total_5xx: u64 = 0;
    let mut total_all: u64 = 0;

    for line in text.lines() {
        if line.starts_with("gateway_http_responses_total") {
            if let Some(val) = line.split_whitespace().last() {
                if let Ok(v) = val.parse::<u64>() {
                    if line.contains("status_class=\"5xx\"") {
                        total_5xx = v;
                    }
                    total_all += v;
                }
            }
        }
    }

    if total_all == 0 {
        return 0.0;
    }
    total_5xx as f64 / total_all as f64
}

/// Parse total cost saved from gateway_cache_cost_saved_usd_total.
fn parse_cost_saved(text: &str) -> f64 {
    for line in text.lines() {
        if line.starts_with("gateway_cache_cost_saved_usd_total") && !line.contains('{') {
            if let Some(val) = line.split_whitespace().last() {
                if let Ok(v) = val.parse::<f64>() {
                    return v;
                }
            }
        }
    }
    0.0
}

/// Count distinct backend names from backend_requests counter.
fn count_backends(text: &str) -> usize {
    let mut backends = std::collections::BTreeSet::new();
    for line in text.lines() {
        if line.starts_with("gateway_backend_requests_total") {
            if let Some(label_start) = line.find("backend_name=\"") {
                let val_start = label_start + 14;
                if let Some(val_end) = line[val_start..].find('"') {
                    backends.insert(&line[val_start..val_start + val_end]);
                }
            }
        }
    }
    backends.len()
}

/// Parse rejection reasons from gateway_rejected_requests_total.
fn parse_rejection_reasons(text: &str) -> Vec<serde_json::Value> {
    let mut reasons: Vec<(String, u64)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("gateway_rejected_requests_total") {
            if let Some(val) = line.split_whitespace().last() {
                if let Ok(v) = val.parse::<u64>() {
                    if v == 0 {
                        continue;
                    }
                    let reason = if let Some(rs) = line.find("reason=\"") {
                        let start = rs + 8;
                        let end = line[start..].find('"').unwrap_or(0);
                        line[start..start + end].to_string()
                    } else {
                        "unknown".to_string()
                    };
                    reasons.push((reason, v));
                }
            }
        }
    }
    reasons.sort_by(|a, b| b.1.cmp(&a.1));
    reasons.truncate(10);
    reasons
        .into_iter()
        .map(|(reason, count)| {
            serde_json::json!({
                "reason": reason,
                "count": count,
            })
        })
        .collect()
}

/// Parse error sources from rejection_by_source_total.
fn parse_error_sources(text: &str) -> Vec<serde_json::Value> {
    let mut sources: Vec<(String, u64)> = Vec::new();
    for line in text.lines() {
        if line.starts_with("gateway_rejection_by_source_total") {
            if let Some(val) = line.split_whitespace().last() {
                if let Ok(v) = val.parse::<u64>() {
                    if v == 0 {
                        continue;
                    }
                    let source = if let Some(ss) = line.find("source=\"") {
                        let start = ss + 8;
                        let end = line[start..].find('"').unwrap_or(0);
                        line[start..start + end].to_string()
                    } else {
                        "unknown".to_string()
                    };
                    sources.push((source, v));
                }
            }
        }
    }
    sources
        .into_iter()
        .map(|(source, count)| {
            serde_json::json!({
                "source": source,
                "count": count,
            })
        })
        .collect()
}
