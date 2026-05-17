use crate::state::{AppState, KeyMetadata};
use crate::types::*;
use crate::network::NetworkInfo;
use crab_control::{
    CreateGatewayKeyRequest, FingerprintConfigRequest, InvalidateCacheRequest,
    PutBackendsRequest, PutTtlConfigRequest,
};
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post, put},
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the admin API key from the environment, or a default dev key.
fn admin_api_key() -> String {
    std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| "admin".to_string())
}

/// Middleware that checks for a valid admin API key in the `X-Admin-Key` header.
async fn admin_auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let expected_key = admin_api_key();
    let provided_key = req
        .headers()
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if provided_key != expected_key {
        tracing::warn!("Admin API authentication failed");
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

fn gateway_status_code(err: &crab_control::ControlError) -> StatusCode {
    match err {
        crab_control::ControlError::Http { status, .. } if *status == 404 => StatusCode::NOT_FOUND,
        crab_control::ControlError::Http { status, .. } if *status == 409 => StatusCode::CONFLICT,
        crab_control::ControlError::Http { status, .. } if *status == 429 => {
            StatusCode::TOO_MANY_REQUESTS
        }
        crab_control::ControlError::Http { status, .. } if *status == 400 => StatusCode::BAD_REQUEST,
        crab_control::ControlError::Http { status, .. } if (400..500).contains(status) => {
            StatusCode::BAD_GATEWAY
        }
        _ => StatusCode::BAD_GATEWAY,
    }
}

fn gateway_error_message(err: &crab_control::ControlError) -> String {
    match err {
        crab_control::ControlError::Http { body, .. } if !body.is_empty() => body.clone(),
        other => other.to_string(),
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/admin/metrics", get(get_metrics))
        .route("/api/admin/gateway/health", get(get_gateway_health))
        .route("/api/admin/network/info", get(get_network_info))
        .route("/api/admin/keys", get(list_keys).post(create_key))
        .route("/api/admin/keys/{id}", delete(revoke_key))
        .route("/api/admin/cache/config", get(get_cache_config).put(update_cache_config))
        .route("/api/admin/cache/ops", get(get_cache_ops))
        .route("/api/admin/cache/invalidate", post(post_cache_invalidate))
        .route(
            "/api/admin/cache/fingerprint",
            put(put_cache_fingerprint),
        )
        .route(
            "/api/admin/cache/stream_cache",
            get(get_stream_cache).put(put_stream_cache),
        )
        .route("/api/admin/semantic/config", get(get_semantic_config).put(update_semantic_config))
        .route("/api/admin/connection/config", get(get_connection_config).put(update_connection_config))
        .route("/api/admin/upstream/config", get(get_upstream_config).put(update_upstream_config))
        .route("/api/admin/models", get(get_models).post(sync_models))
        .route("/api/admin/routing/status", get(get_routing_status))
        .route("/api/admin/logs", get(get_logs))
        .route("/api/admin/logs/{id}", get(get_log_detail))
        .route("/api/admin/trace/analysis", get(get_trace_analysis))
        .layer(middleware::from_fn(admin_auth))
        .with_state(state)
}

async fn get_gateway_health(State(state): State<Arc<AppState>>) -> Json<GatewayHealthView> {
    match state.gateway.ready().await {
        Ok(()) => match state.gateway.status().await {
            Ok(s) => Json(GatewayHealthView {
                healthy: true,
                uptime_secs: s.uptime_secs,
                active_keys: s.active_keys,
                backend_count: s.backend_count,
                stream_cache_enabled: s.stream_cache_enabled,
                error: None,
            }),
            Err(e) => Json(GatewayHealthView {
                healthy: true,
                uptime_secs: 0,
                active_keys: 0,
                backend_count: 0,
                stream_cache_enabled: false,
                error: Some(gateway_error_message(&e)),
            }),
        },
        Err(e) => Json(GatewayHealthView {
            healthy: false,
            uptime_secs: 0,
            active_keys: 0,
            backend_count: 0,
            stream_cache_enabled: false,
            error: Some(gateway_error_message(&e)),
        }),
    }
}

async fn get_network_info() -> Json<NetworkInfo> {
    let use_https = std::env::var("CRABCACHE_HTTPS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    Json(NetworkInfo::new(8080, use_https))
}

async fn get_metrics(State(state): State<Arc<AppState>>) -> Result<Json<MetricsSnapshot>, StatusCode> {
    let metrics_url = std::env::var("CRABCACHE_GATEWAY_METRICS_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9090/metrics".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client.get(&metrics_url).send().await.map_err(|e| {
        tracing::warn!(url = %metrics_url, error = %e, "Failed to fetch gateway metrics");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    let body = resp.text().await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    // l2_hits = tier L2_semantic; semantic_* = semantic guard status (different meaning).
    let l0_hits = sum_prometheus_counter(
        &body,
        "gateway_cache_requests_total",
        &[("tier", "L0_moka"), ("result", "hit")],
    );
    let l1_hits = sum_prometheus_counter(
        &body,
        "gateway_cache_requests_total",
        &[("tier", "L1_redis"), ("result", "hit")],
    );
    let l2_hits = sum_prometheus_counter(
        &body,
        "gateway_cache_requests_total",
        &[("tier", "L2_semantic"), ("result", "hit")],
    );
    let cache_misses = sum_prometheus_counter(
        &body,
        "gateway_cache_requests_total",
        &[("tier", "miss"), ("result", "miss")],
    );
    let total_input_tokens_hit = sum_prometheus_counter(
        &body,
        "gateway_deepseek_input_tokens_total",
        &[("cache_status", "hit")],
    );
    let total_input_tokens_miss = sum_prometheus_counter(
        &body,
        "gateway_deepseek_input_tokens_total",
        &[("cache_status", "miss")],
    );
    let total_output_tokens = sum_prometheus_counter(&body, "gateway_deepseek_output_tokens_total", &[]);
    let semantic_hits = sum_prometheus_counter(
        &body,
        "gateway_semantic_cache_requests_total",
        &[("status", "hit_above_threshold")],
    ) + sum_prometheus_counter(
        &body,
        "gateway_semantic_cache_requests_total",
        &[("status", "hit_below_threshold")],
    );
    let semantic_rejected = sum_prometheus_counter(
        &body,
        "gateway_semantic_cache_requests_total",
        &[("status", "rejected_by_guard")],
    );
    let semantic_skipped = sum_prometheus_counter(&body, "gateway_semantic_skipped_total", &[]);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let uptime_secs = now.saturating_sub(state.start_time);

    let total_requests = l0_hits + l1_hits + l2_hits + cache_misses;
    let qps = if uptime_secs > 0 {
        total_requests as f64 / uptime_secs as f64
    } else {
        0.0
    };

    let tps = if uptime_secs > 0 {
        (total_input_tokens_hit + total_input_tokens_miss + total_output_tokens) as f64 / uptime_secs as f64
    } else {
        0.0
    };

    let hourly_stats = generate_hourly_stats_mock(uptime_secs, total_requests, total_input_tokens_hit + total_input_tokens_miss + total_output_tokens, l0_hits + l1_hits + l2_hits);
    let daily_stats = generate_daily_stats_mock(uptime_secs, total_requests, total_input_tokens_hit + total_input_tokens_miss + total_output_tokens, l0_hits + l1_hits + l2_hits);
    let weekly_stats = generate_weekly_stats_mock(uptime_secs, total_requests, total_input_tokens_hit + total_input_tokens_miss + total_output_tokens, l0_hits + l1_hits + l2_hits);
    let monthly_stats = generate_monthly_stats_mock(uptime_secs, total_requests, total_input_tokens_hit + total_input_tokens_miss + total_output_tokens, l0_hits + l1_hits + l2_hits);

    Ok(Json(MetricsSnapshot {
        qps,
        tps,
        l0_hits,
        l1_hits,
        l2_hits,
        cache_misses,
        cache_hit_tokens: total_input_tokens_hit,
        cache_miss_tokens: total_input_tokens_miss,
        total_input_tokens: total_input_tokens_hit + total_input_tokens_miss,
        total_output_tokens,
        total_tokens: total_input_tokens_hit + total_input_tokens_miss + total_output_tokens,
        latency_l0_ms: avg_prometheus_histogram_ms(
            &body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L0_moka")],
        ),
        latency_l1_ms: avg_prometheus_histogram_ms(
            &body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L1_redis")],
        ),
        latency_l2_ms: avg_prometheus_histogram_ms(
            &body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L2_semantic")],
        ),
        latency_upstream_ms: avg_prometheus_histogram_ms(&body, "gateway_upstream_latency_seconds", &[]),
        active_keys: state.keys_meta.len() as u64,
        uptime_hours: uptime_secs / 3600,
        hourly_stats,
        daily_stats,
        weekly_stats,
        monthly_stats,
        semantic_hits,
        semantic_rejected,
        semantic_skipped,
    }))
}

fn labels_match(labels: &str, required: &[(&str, &str)]) -> bool {
    required
        .iter()
        .all(|(key, val)| labels.contains(&format!("{key}=\"{val}\"")))
}

/// Sum all float samples for `metric` whose labels contain every required pair.
fn sum_prometheus_sample(body: &str, metric: &str, required: &[(&str, &str)]) -> f64 {
    let mut total = 0.0;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with(metric) {
            continue;
        }
        if let Some(open) = line.find('{') {
            if let Some(close) = line.find('}') {
                let labels = &line[open + 1..close];
                if !labels_match(labels, required) {
                    continue;
                }
                let value_part = line[close + 1..].trim();
                if let Ok(v) = value_part.parse::<f64>() {
                    total += v;
                }
            }
        } else if required.is_empty() {
            let value_part = line[metric.len()..].trim();
            if let Ok(v) = value_part.parse::<f64>() {
                total += v;
            }
        }
    }
    total
}

/// Average latency in milliseconds from Prometheus histogram `_sum` / `_count` series.
fn avg_prometheus_histogram_ms(body: &str, metric: &str, required: &[(&str, &str)]) -> f64 {
    let sum = sum_prometheus_sample(body, &format!("{metric}_sum"), required);
    let count = sum_prometheus_sample(body, &format!("{metric}_count"), required);
    if count > 0.0 {
        (sum / count) * 1000.0
    } else {
        0.0
    }
}

/// Sum all counter samples for `metric` whose labels contain every required pair.
fn sum_prometheus_counter(body: &str, metric: &str, required: &[(&str, &str)]) -> u64 {
    let mut total = 0u64;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.starts_with(metric) {
            continue;
        }
        if let Some(open) = line.find('{') {
            if let Some(close) = line.find('}') {
                let labels = &line[open + 1..close];
                if !labels_match(labels, required) {
                    continue;
                }
                let value_part = line[close + 1..].trim();
                if let Ok(v) = value_part.parse::<u64>() {
                    total += v;
                }
            }
        } else if required.is_empty() {
            let value_part = line[metric.len()..].trim();
            if let Ok(v) = value_part.parse::<u64>() {
                total += v;
            }
        }
    }
    total
}

#[cfg(test)]
mod metrics_tests {
    use super::sum_prometheus_counter;

    #[test]
    fn sum_across_multiple_model_labels() {
        let body = r#"
gateway_deepseek_input_tokens_total{cache_status="hit",model="m1",consumer="c"} 10
gateway_deepseek_input_tokens_total{cache_status="hit",model="m2",consumer="c"} 20
gateway_deepseek_input_tokens_total{cache_status="miss",model="m1",consumer="c"} 5
"#;
        assert_eq!(
            sum_prometheus_counter(body, "gateway_deepseek_input_tokens_total", &[("cache_status", "hit")]),
            30
        );
        assert_eq!(
            sum_prometheus_counter(body, "gateway_deepseek_input_tokens_total", &[("cache_status", "miss")]),
            5
        );
    }

    #[test]
    fn avg_histogram_across_models() {
        let body = r#"
gateway_cache_fetch_latency_seconds_sum{tier="L0_moka",model="m1"} 0.002
gateway_cache_fetch_latency_seconds_sum{tier="L0_moka",model="m2"} 0.004
gateway_cache_fetch_latency_seconds_count{tier="L0_moka",model="m1"} 10
gateway_cache_fetch_latency_seconds_count{tier="L0_moka",model="m2"} 30
"#;
        let avg = super::avg_prometheus_histogram_ms(
            body,
            "gateway_cache_fetch_latency_seconds",
            &[("tier", "L0_moka")],
        );
        // (0.006 / 40) * 1000 = 0.15 ms
        assert!((avg - 0.15).abs() < 1e-6);
    }
}

fn generate_hourly_stats_mock(uptime_secs: u64, total_requests: u64, total_tokens: u64, total_cache_hits: u64) -> Vec<TimeSeriesPoint> {
    let hours = (uptime_secs / 3600).min(24) as usize;
    let mut stats = Vec::new();
    for i in 0..hours {
        let hour_ago = hours - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::hours(hour_ago as i64);
        let divisor = (hours as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%H:00").to_string(),
            requests: if i == 0 { total_requests } else { total_requests / divisor },
            tokens: if i == 0 { total_tokens } else { total_tokens / divisor },
            cache_hits: if i == 0 { total_cache_hits } else { total_cache_hits / divisor },
            avg_latency_ms: 0.0,
        });
    }
    stats
}

fn generate_daily_stats_mock(uptime_secs: u64, total_requests: u64, total_tokens: u64, total_cache_hits: u64) -> Vec<TimeSeriesPoint> {
    let days = (uptime_secs / 86400).min(7) as usize;
    let mut stats = Vec::new();
    for i in 0..days {
        let day_ago = days - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days(day_ago as i64);
        let divisor = (days as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%m-%d").to_string(),
            requests: if i == 0 { total_requests } else { total_requests / divisor },
            tokens: if i == 0 { total_tokens } else { total_tokens / divisor },
            cache_hits: if i == 0 { total_cache_hits } else { total_cache_hits / divisor },
            avg_latency_ms: 0.0,
        });
    }
    stats
}

fn generate_weekly_stats_mock(uptime_secs: u64, total_requests: u64, total_tokens: u64, total_cache_hits: u64) -> Vec<TimeSeriesPoint> {
    let weeks = (uptime_secs / 604800).min(4) as usize;
    let mut stats = Vec::new();
    for i in 0..weeks {
        let week_ago = weeks - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::weeks(week_ago as i64);
        let divisor = (weeks as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("W%U").to_string(),
            requests: if i == 0 { total_requests } else { total_requests / divisor },
            tokens: if i == 0 { total_tokens } else { total_tokens / divisor },
            cache_hits: if i == 0 { total_cache_hits } else { total_cache_hits / divisor },
            avg_latency_ms: 0.0,
        });
    }
    stats
}

fn generate_monthly_stats_mock(uptime_secs: u64, total_requests: u64, total_tokens: u64, total_cache_hits: u64) -> Vec<TimeSeriesPoint> {
    let months = (uptime_secs / 2592000).min(12) as usize;
    let mut stats = Vec::new();
    for i in 0..months {
        let month_ago = months - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days((month_ago * 30) as i64);
        let divisor = (months as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%Y-%m").to_string(),
            requests: if i == 0 { total_requests } else { total_requests / divisor },
            tokens: if i == 0 { total_tokens } else { total_tokens / divisor },
            cache_hits: if i == 0 { total_cache_hits } else { total_cache_hits / divisor },
            avg_latency_ms: 0.0,
        });
    }
    stats
}

fn generate_hourly_stats(metrics: &crate::state::StoredMetrics, uptime_secs: u64) -> Vec<TimeSeriesPoint> {
    let hours = (uptime_secs / 3600).min(24) as usize;
    let mut stats = Vec::new();
    
    for i in 0..hours {
        let hour_ago = hours - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::hours(hour_ago as i64);
        
        let requests = if i == 0 { metrics.total_requests } else { metrics.total_requests / (hours as u64).max(1) };
        let tokens = if i == 0 { metrics.total_input_tokens + metrics.total_output_tokens } else { (metrics.total_input_tokens + metrics.total_output_tokens) / (hours as u64).max(1) };
        let cache_hits = if i == 0 { metrics.l0_hits + metrics.l1_hits + metrics.l2_hits } else { (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (hours as u64).max(1) };
        let avg_latency_ms = if metrics.upstream_latency_count > 0 {
            metrics.upstream_latency_sum_ms / metrics.upstream_latency_count as f64
        } else {
            0.0
        };
        
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%H:00").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
        });
    }
    
    stats
}

fn generate_daily_stats(metrics: &crate::state::StoredMetrics, uptime_secs: u64) -> Vec<TimeSeriesPoint> {
    let days = (uptime_secs / 86400).min(7) as usize;
    let mut stats = Vec::new();
    
    for i in 0..days {
        let day_ago = days - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days(day_ago as i64);
        
        let requests = if i == 0 { metrics.total_requests } else { metrics.total_requests / (days as u64).max(1) };
        let tokens = if i == 0 { metrics.total_input_tokens + metrics.total_output_tokens } else { (metrics.total_input_tokens + metrics.total_output_tokens) / (days as u64).max(1) };
        let cache_hits = if i == 0 { metrics.l0_hits + metrics.l1_hits + metrics.l2_hits } else { (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (days as u64).max(1) };
        let avg_latency_ms = if metrics.upstream_latency_count > 0 {
            metrics.upstream_latency_sum_ms / metrics.upstream_latency_count as f64
        } else {
            0.0
        };
        
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%m-%d").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
        });
    }
    
    stats
}

fn generate_weekly_stats(metrics: &crate::state::StoredMetrics, uptime_secs: u64) -> Vec<TimeSeriesPoint> {
    let weeks = (uptime_secs / 604800).min(4) as usize;
    let mut stats = Vec::new();
    
    for i in 0..weeks {
        let week_ago = weeks - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::weeks(week_ago as i64);
        
        let requests = if i == 0 { metrics.total_requests } else { metrics.total_requests / (weeks as u64).max(1) };
        let tokens = if i == 0 { metrics.total_input_tokens + metrics.total_output_tokens } else { (metrics.total_input_tokens + metrics.total_output_tokens) / (weeks as u64).max(1) };
        let cache_hits = if i == 0 { metrics.l0_hits + metrics.l1_hits + metrics.l2_hits } else { (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (weeks as u64).max(1) };
        let avg_latency_ms = if metrics.upstream_latency_count > 0 {
            metrics.upstream_latency_sum_ms / metrics.upstream_latency_count as f64
        } else {
            0.0
        };
        
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("W%U").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
        });
    }
    
    stats
}

fn generate_monthly_stats(metrics: &crate::state::StoredMetrics, uptime_secs: u64) -> Vec<TimeSeriesPoint> {
    let months = (uptime_secs / 2592000).min(12) as usize;
    let mut stats = Vec::new();
    
    for i in 0..months {
        let month_ago = months - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days((month_ago * 30) as i64);
        
        let requests = if i == 0 { metrics.total_requests } else { metrics.total_requests / (months as u64).max(1) };
        let tokens = if i == 0 { metrics.total_input_tokens + metrics.total_output_tokens } else { (metrics.total_input_tokens + metrics.total_output_tokens) / (months as u64).max(1) };
        let cache_hits = if i == 0 { metrics.l0_hits + metrics.l1_hits + metrics.l2_hits } else { (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (months as u64).max(1) };
        let avg_latency_ms = if metrics.upstream_latency_count > 0 {
            metrics.upstream_latency_sum_ms / metrics.upstream_latency_count as f64
        } else {
            0.0
        };
        
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%Y-%m").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
        });
    }
    
    stats
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ApiKey>>, StatusCode> {
    let specs = state
        .gateway
        .list_keys()
        .await
        .map_err(|e| gateway_status_code(&e))?;

    let keys: Vec<ApiKey> = specs
        .into_iter()
        .map(|spec| {
            let meta = state.keys_meta.get(&spec.id);
            ApiKey {
                id: spec.id.clone(),
                name: spec.name,
                key_preview: spec.key_preview,
                key_full: meta
                    .as_ref()
                    .and_then(|m| {
                        if m.token.is_empty() {
                            None
                        } else {
                            Some(m.token.clone())
                        }
                    })
                    .or(spec.key_full),
                active: spec.enabled,
                rpm_limit: meta.as_ref().map(|m| m.rpm_limit as u32).unwrap_or(0),
                monthly_token_budget: meta
                    .as_ref()
                    .map(|m| m.monthly_token_limit)
                    .unwrap_or(0),
                tokens_used_this_month: meta
                    .as_ref()
                    .map(|m| m.tokens_this_month)
                    .unwrap_or(0),
                expired_at: meta.as_ref().and_then(|m| m.expired_at),
                model_limits: meta
                    .as_ref()
                    .map(|m| m.model_limits.clone())
                    .unwrap_or_default(),
                remain_quota: meta.as_ref().map(|m| m.remain_quota).unwrap_or(-1),
                unlimited_quota: meta
                    .as_ref()
                    .map(|m| m.unlimited_quota)
                    .unwrap_or(true),
            }
        })
        .collect();

    Ok(Json(keys))
}

async fn create_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<ApiKey>, StatusCode> {
    let created = state
        .gateway
        .create_key(&CreateGatewayKeyRequest {
            name: req.name.clone(),
            enabled: true,
            token: None,
        })
        .await
        .map_err(|e| gateway_status_code(&e))?;

    let model_limits = req.model_limits.clone().unwrap_or_default();
    let remain_quota = req.remain_quota.unwrap_or(-1);
    let unlimited_quota = req.unlimited_quota.unwrap_or(true);

    let meta = KeyMetadata {
        id: created.id.clone(),
        token: created.key_full.clone(),
        rpm_limit: req.rpm_limit as u64,
        monthly_token_limit: req.monthly_token_budget,
        current_rpm: 0,
        tokens_this_month: 0,
        input_tokens: 0,
        output_tokens: 0,
        expired_at: req.expired_at,
        model_limits: model_limits.clone(),
        remain_quota,
        unlimited_quota,
    };
    state.keys_meta.insert(created.id.clone(), meta);

    Ok(Json(ApiKey {
        id: created.id,
        name: created.name,
        key_preview: created.key_preview,
        key_full: Some(created.key_full),
        active: created.enabled,
        rpm_limit: req.rpm_limit,
        monthly_token_budget: req.monthly_token_budget,
        tokens_used_this_month: 0,
        expired_at: req.expired_at,
        model_limits,
        remain_quota,
        unlimited_quota,
    }))
}

async fn revoke_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let token = state
        .keys_meta
        .get(&id)
        .map(|m| m.token.clone())
        .ok_or(StatusCode::NOT_FOUND)?;

    state
        .gateway
        .revoke_key(&token)
        .await
        .map_err(|e| gateway_status_code(&e))?;

    state.keys_meta.remove(&id);
    Ok(StatusCode::NO_CONTENT)
}

async fn get_cache_ops(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CacheOpsView>, (StatusCode, String)> {
    let status = state
        .gateway
        .status()
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    let fingerprint = state
        .gateway
        .get_fingerprint()
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    let last_invalidate = state.last_invalidate.read().clone().map(|li| LastInvalidateView {
        scope: li.scope,
        status: li.status,
        at_secs: li.at_secs,
        error: li.error,
    });

    let invalidate_status = state
        .gateway
        .get_invalidate_status()
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    let invalidate_job = invalidate_status.job.map(|j| InvalidateJobView {
        scope: j.scope,
        phase: j.phase,
        error: j.error,
        started_at_secs: j.started_at_secs,
        completed_at_secs: j.completed_at_secs,
    });

    Ok(Json(CacheOpsView {
        fingerprint_version: fingerprint.version,
        fingerprint_normalize: fingerprint.normalize_content,
        stream_cache_enabled: status.stream_cache_enabled,
        last_invalidate,
        invalidate_all_in_progress: invalidate_status.all_in_progress,
        invalidate_job,
    }))
}

async fn post_cache_invalidate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InvalidateCacheBody>,
) -> Result<Json<InvalidateCacheResult>, (StatusCode, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match state
        .gateway
        .invalidate_cache(&InvalidateCacheRequest {
            scope: req.scope.clone(),
        })
        .await
    {
        Ok(resp) => {
            *state.last_invalidate.write() = Some(crate::state::LastInvalidate {
                scope: resp.scope.clone(),
                status: resp.status.clone(),
                at_secs: now,
                error: None,
            });
            tracing::info!(scope = %resp.scope, status = %resp.status, "Cache invalidation accepted by gateway");
            Ok(Json(InvalidateCacheResult {
                scope: resp.scope,
                status: resp.status,
            }))
        }
        Err(e) => {
            let msg = gateway_error_message(&e);
            *state.last_invalidate.write() = Some(crate::state::LastInvalidate {
                scope: req.scope.clone(),
                status: "failed".to_string(),
                at_secs: now,
                error: Some(msg.clone()),
            });
            Err((gateway_status_code(&e), msg))
        }
    }
}

async fn put_cache_fingerprint(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FingerprintConfigBody>,
) -> Result<Json<FingerprintConfigBody>, (StatusCode, String)> {
    let updated = state
        .gateway
        .put_fingerprint(&FingerprintConfigRequest {
            version: req.version,
            normalize_content: req.normalize_content,
        })
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    Ok(Json(FingerprintConfigBody {
        version: updated.version,
        normalize_content: updated.normalize_content,
    }))
}

async fn get_stream_cache(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StreamCacheConfig>, (StatusCode, String)> {
    let cfg = state
        .gateway
        .get_stream_cache()
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    Ok(Json(StreamCacheConfig {
        enabled: cfg.enabled,
    }))
}

async fn put_stream_cache(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StreamCacheConfig>,
) -> Result<Json<StreamCacheConfig>, (StatusCode, String)> {
    let cfg = state
        .gateway
        .put_stream_cache(&crab_control::StreamCacheConfig {
            enabled: req.enabled,
        })
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    Ok(Json(StreamCacheConfig {
        enabled: cfg.enabled,
    }))
}

async fn get_cache_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CacheConfig>, StatusCode> {
    let config = state.cache_config.read().clone();
    Ok(Json(CacheConfig {
        l0_ttl_secs: config.l0_ttl_secs,
        l1_ttl_secs: config.l1_ttl_secs,
        default_ttl_secs: config.default_ttl_secs,
    }))
}

async fn update_cache_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateCacheConfigRequest>,
) -> Result<Json<CacheConfig>, StatusCode> {
    let put_req = {
        let mut config = state.cache_config.write();
        config.l0_ttl_secs = req.l0_ttl_secs;
        config.l1_ttl_secs = req.l1_ttl_secs;
        PutTtlConfigRequest {
            default_ttl_secs: config.l1_ttl_secs,
            model_overrides: config
                .model_overrides
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            consumer_overrides: config
                .consumer_overrides
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        }
    };

    let ttl = state
        .gateway
        .put_ttl(&put_req)
        .await
        .map_err(|e| gateway_status_code(&e))?;

    let config = {
        let mut config = state.cache_config.write();
        config.default_ttl_secs = ttl.default_ttl_secs;
        config.clone()
    };

    Ok(Json(CacheConfig {
        l0_ttl_secs: config.l0_ttl_secs,
        l1_ttl_secs: config.l1_ttl_secs,
        default_ttl_secs: config.default_ttl_secs,
    }))
}

async fn get_semantic_config(State(state): State<Arc<AppState>>) -> Json<SemanticConfig> {
    let config = state.semantic_config.read().clone();
    Json(SemanticConfig {
        similarity_threshold: config.similarity_threshold as f64,
    })
}

async fn update_semantic_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateSemanticConfigRequest>,
) -> Json<SemanticConfig> {
    let mut config = state.semantic_config.write();
    config.similarity_threshold = req.similarity_threshold as f32;

    Json(SemanticConfig {
        similarity_threshold: config.similarity_threshold as f64,
    })
}

async fn get_routing_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RoutingStatus>, StatusCode> {
    let view = state
        .gateway
        .get_backends()
        .await
        .map_err(|e| gateway_status_code(&e))?;

    let backends: Vec<BackendStatus> = view
        .backends
        .into_iter()
        .map(|b| BackendStatus {
            name: b.name,
            request_count: 0,
            healthy: b.healthy,
        })
        .collect();
    let active_count = backends.len();

    Ok(Json(RoutingStatus {
        total_backends: backends.len(),
        active_backends: active_count,
        total_requests: 0,
        backends,
    }))
}

async fn get_logs(State(state): State<Arc<AppState>>) -> Json<Vec<RequestLog>> {
    let logs = state.request_logs.read().clone();
    let result: Vec<RequestLog> = logs
        .into_iter()
        .map(|log| {
            let ts = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(log.timestamp);
            let datetime: chrono::DateTime<chrono::Utc> = ts.into();
            RequestLog {
                id: log.id,
                timestamp: datetime.format("%H:%M:%S").to_string(),
                model: log.model.clone(),
                consumer: log.consumer.clone(),
                latency_ms: log.duration_ms as u64,
                total_tokens: log.input_tokens + log.output_tokens,
                cache_status: log.cache_tier.clone(),
                request_payload: serde_json::to_string_pretty(&log.request_payload).unwrap_or_default(),
                response_preview: log.response_body.chars().take(200).collect(),
            }
        })
        .collect();

    Json(result)
}

async fn get_log_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RequestDetail>, StatusCode> {
    let logs = state.request_logs.read().clone();
    let log = logs.iter().find(|l| l.id == id).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(RequestDetail {
        cache_path: log.cache_path.join(" → "),
        request_payload: serde_json::to_string_pretty(&log.request_payload).unwrap_or_default(),
        response_body: log.response_body.clone(),
        route_backend: log.route_backend.clone(),
    }))
}

async fn get_connection_config(State(state): State<Arc<AppState>>) -> Json<ConnectionConfig> {
    let config = state.connection_config.read().clone();
    Json(ConnectionConfig {
        tcp_keepalive_idle_secs: config.tcp_keepalive_idle_secs,
        tcp_keepalive_interval_secs: config.tcp_keepalive_interval_secs,
        tcp_keepalive_count: config.tcp_keepalive_count,
        idle_timeout_secs: config.idle_timeout_secs,
        h2_ping_interval_secs: config.h2_ping_interval_secs,
    })
}

async fn get_models(State(state): State<Arc<AppState>>) -> Json<ModelListResponse> {
    let stored = state.models.read();
    let models: Vec<ModelInfo> = stored
        .models
        .iter()
        .map(|m| ModelInfo {
            id: m.id.clone(),
            owned_by: m.owned_by.clone(),
            context_length: m.context_length,
            input_price_per_mtok: m.input_price_per_mtok,
            output_price_per_mtok: m.output_price_per_mtok,
            available: m.available,
        })
        .collect();
    let total = models.len();
    Json(ModelListResponse {
        models,
        total,
        synced_at: stored.synced_at.clone(),
    })
}

async fn sync_models(State(state): State<Arc<AppState>>) -> Result<Json<SyncResult>, (StatusCode, String)> {
    let upstream_config = state.upstream_config.read().clone();
    let upstream_url = format!("{}/v1/models", upstream_config.base_url.trim_end_matches('/'));
    let api_key = upstream_config.api_key.clone();

    tracing::info!(url = %upstream_url, "Syncing models from upstream");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create HTTP client: {}", e)))?;

    let mut request = client.get(&upstream_url);
    if api_key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Please configure upstream API key first in the Upstream settings page.".into()));
    }
    request = request.header("Authorization", format!("Bearer {}", &api_key));
    let resp = request
        .send()
        .await
        .map_err(|e| {
            tracing::error!(url = %upstream_url, error = %e, "Upstream request failed");
            (StatusCode::BAD_GATEWAY, format!("Cannot reach upstream: {}", e))
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::error!(url = %upstream_url, status = %status, body = %body, "Upstream returned error");
        let msg = match status.as_u16() {
            401 | 403 => format!("Upstream authentication failed ({}). Check your API key.", status.as_u16()),
            404 => format!("Models endpoint not found at {}. Check the Base URL.", upstream_url),
            _ => format!("Upstream returned {}: {}", status.as_u16(), body.chars().take(200).collect::<String>()),
        };
        return Err((StatusCode::BAD_GATEWAY, msg));
    }

    let upstream: UpstreamModelsResponse = resp
        .json()
        .await
        .map_err(|e| {
            tracing::error!(url = %upstream_url, error = %e, "Failed to parse upstream models response");
            (StatusCode::BAD_GATEWAY, format!("Failed to parse upstream response: {}", e))
        })?;

    let upstream_ids: Vec<String> = upstream.data.iter().map(|m| m.id.clone()).collect();

    let mut stored = state.models.write();
    let existing_ids: Vec<String> = stored.models.iter().map(|m| m.id.clone()).collect();

    let added: Vec<String> = upstream_ids
        .iter()
        .filter(|id| !existing_ids.contains(id))
        .cloned()
        .collect();

    let removed: Vec<String> = existing_ids
        .iter()
        .filter(|id| !upstream_ids.contains(id))
        .cloned()
        .collect();

    let unchanged = upstream_ids
        .iter()
        .filter(|id| existing_ids.contains(id))
        .count();

    let upstream_models: Vec<crate::state::StoredModel> = upstream
        .data
        .into_iter()
        .map(|m| {
            let existing = stored.models.iter().find(|e| e.id == m.id);
            crate::state::StoredModel {
                id: m.id,
                owned_by: m.owned_by,
                context_length: existing.and_then(|e| e.context_length),
                input_price_per_mtok: existing.and_then(|e| e.input_price_per_mtok),
                output_price_per_mtok: existing.and_then(|e| e.output_price_per_mtok),
                available: true,
            }
        })
        .collect();

    let total = upstream_models.len();
    stored.models = upstream_models;
    stored.synced_at = Some(
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
    );

    tracing::info!(added = added.len(), removed = removed.len(), unchanged, total, "Models synced successfully");

    Ok(Json(SyncResult {
        added,
        removed,
        unchanged,
        total,
    }))
}

async fn update_connection_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateConnectionConfigRequest>,
) -> Json<ConnectionConfig> {
    let mut config = state.connection_config.write();
    config.tcp_keepalive_idle_secs = req.tcp_keepalive_idle_secs;
    config.tcp_keepalive_interval_secs = req.tcp_keepalive_interval_secs;
    config.tcp_keepalive_count = req.tcp_keepalive_count;
    config.idle_timeout_secs = req.idle_timeout_secs;
    config.h2_ping_interval_secs = req.h2_ping_interval_secs;

    Json(ConnectionConfig {
        tcp_keepalive_idle_secs: config.tcp_keepalive_idle_secs,
        tcp_keepalive_interval_secs: config.tcp_keepalive_interval_secs,
        tcp_keepalive_count: config.tcp_keepalive_count,
        idle_timeout_secs: config.idle_timeout_secs,
        h2_ping_interval_secs: config.h2_ping_interval_secs,
    })
}

async fn get_upstream_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<UpstreamConfig>, StatusCode> {
    let config = state.upstream_config.read().clone();
    let api_key_masked = mask_api_key(&config.api_key);
    Ok(Json(UpstreamConfig {
        base_url: config.base_url,
        api_key: api_key_masked.clone(),
        api_key_masked,
        endpoints: config.endpoints,
    }))
}

async fn update_upstream_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateUpstreamConfigRequest>,
) -> Result<Json<UpstreamConfig>, StatusCode> {
    let put_req = {
        let mut config = state.upstream_config.write();
        config.base_url = req.base_url.clone();
        if let Some(key) = &req.api_key {
            if !key.is_empty() && !key.contains("****") {
                config.api_key = key.clone();
            }
        }
        config.endpoints = req.endpoints.clone();
        PutBackendsRequest {
            endpoints: req.endpoints.clone(),
            default_weight: 1,
            tls_sni: "api.deepseek.com".to_string(),
        }
    };

    state
        .gateway
        .put_backends(&put_req)
        .await
        .map_err(|e| gateway_status_code(&e))?;

    let response = {
        let config = state.upstream_config.read().clone();
        let mut backends = state.backends.write();
        *backends = config
            .endpoints
            .iter()
            .enumerate()
            .map(|(i, ep)| crate::state::StoredBackend {
                name: format!("backend-{}", i + 1),
                addr: ep.clone(),
                weight: 1,
                healthy: true,
                request_count: 0,
            })
            .collect();

        let api_key_masked = mask_api_key(&config.api_key);
        UpstreamConfig {
            base_url: config.base_url,
            api_key: api_key_masked.clone(),
            api_key_masked,
            endpoints: config.endpoints,
        }
    };

    Ok(Json(response))
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len()-4..])
}

async fn get_trace_analysis(State(state): State<Arc<AppState>>) -> Json<TraceAnalysis> {
    use std::collections::HashMap;
    
    let trace_entries = state.trace_entries.read().clone();
    
    if trace_entries.is_empty() {
        return Json(TraceAnalysis {
            total_requests: 0,
            unique_requests: 0,
            repeat_ratio: 0.0,
            semantic_cluster_ratio: 0.0,
            estimated_zipf_alpha: 0.0,
            estimated_hit_rate: 0.0,
            avg_latency_ms: 0.0,
            avg_prompt_tokens: 0.0,
            cache_hit_ratio: 0.0,
            top_models: vec![],
            cluster_distribution: vec![],
        });
    }
    
    let total_requests = trace_entries.len();
    let mut hash_counts: HashMap<String, usize> = HashMap::new();
    let mut cluster_counts: HashMap<usize, usize> = HashMap::new();
    let mut model_counts: HashMap<String, usize> = HashMap::new();
    let mut total_latency = 0.0;
    let mut total_prompt_tokens = 0;
    let mut cache_hits = 0;
    
    for entry in &trace_entries {
        *hash_counts.entry(entry.request_hash.clone()).or_insert(0) += 1;
        *cluster_counts.entry(entry.semantic_cluster).or_insert(0) += 1;
        *model_counts.entry(entry.model.clone()).or_insert(0) += 1;
        total_latency += entry.latency_ms;
        total_prompt_tokens += entry.prompt_tokens;
        if entry.cache_hit {
            cache_hits += 1;
        }
    }
    
    let unique_requests = hash_counts.len();
    let repeat_ratio = if total_requests > 0 {
        1.0 - (unique_requests as f64 / total_requests as f64)
    } else {
        0.0
    };
    
    let multi_member_clusters: usize = cluster_counts.values().filter(|&&c| c > 1).sum();
    let semantic_cluster_ratio = if total_requests > 0 {
        multi_member_clusters as f64 / total_requests as f64
    } else {
        0.0
    };
    
    let estimated_hit_rate = repeat_ratio + (1.0 - repeat_ratio) * semantic_cluster_ratio;
    
    let mut freqs: Vec<usize> = hash_counts.values().cloned().collect();
    freqs.sort_by(|a, b| b.cmp(a));
    let estimated_zipf_alpha = if freqs.len() >= 2 {
        compute_zipf_alpha(&freqs)
    } else {
        0.0
    };
    
    let avg_latency_ms = if total_requests > 0 {
        total_latency / total_requests as f64
    } else {
        0.0
    };
    
    let avg_prompt_tokens = if total_requests > 0 {
        total_prompt_tokens as f64 / total_requests as f64
    } else {
        0.0
    };
    
    let cache_hit_ratio = if total_requests > 0 {
        cache_hits as f64 / total_requests as f64
    } else {
        0.0
    };
    
    let mut top_models: Vec<ModelUsage> = model_counts
        .into_iter()
        .map(|(model, count)| ModelUsage {
            model,
            count,
            percentage: if total_requests > 0 {
                count as f64 / total_requests as f64 * 100.0
            } else {
                0.0
            },
        })
        .collect();
    top_models.sort_by(|a, b| b.count.cmp(&a.count));
    top_models.truncate(5);
    
    let mut cluster_distribution: Vec<ClusterInfo> = cluster_counts
        .into_iter()
        .map(|(cluster_id, count)| ClusterInfo {
            cluster_id,
            count,
            percentage: if total_requests > 0 {
                count as f64 / total_requests as f64 * 100.0
            } else {
                0.0
            },
        })
        .collect();
    cluster_distribution.sort_by(|a, b| b.count.cmp(&a.count));
    cluster_distribution.truncate(10);
    
    Json(TraceAnalysis {
        total_requests,
        unique_requests,
        repeat_ratio,
        semantic_cluster_ratio,
        estimated_zipf_alpha,
        estimated_hit_rate,
        avg_latency_ms,
        avg_prompt_tokens,
        cache_hit_ratio,
        top_models,
        cluster_distribution,
    })
}

fn compute_zipf_alpha(freqs: &[usize]) -> f64 {
    if freqs.len() < 2 {
        return 0.0;
    }
    
    let n = freqs.len();
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;
    
    for (rank, &freq) in freqs.iter().enumerate() {
        if freq == 0 {
            continue;
        }
        let r = (rank + 1) as f64;
        let f = freq as f64;
        let log_r = r.ln();
        let log_f = f.ln();
        
        sum_x += log_r;
        sum_y += log_f;
        sum_xy += log_r * log_f;
        sum_xx += log_r * log_r;
    }
    
    let denominator = n as f64 * sum_xx - sum_x * sum_x;
    if denominator == 0.0 {
        return 0.0;
    }
    
    let alpha = (n as f64 * sum_xy - sum_x * sum_y) / denominator;
    -alpha
}