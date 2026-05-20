use crate::network::NetworkInfo;
use crate::state::{AppState, KeyMetadata};
use crate::types::*;
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, patch, post, put},
};
use crab_control::{
    CreateGatewayKeyRequest, FingerprintConfigRequest, InvalidateCacheRequest, PutBackendsRequest,
    PutTtlConfigRequest, PutUpstreamKeysRequest, UpstreamKeyInput, UpstreamKeysPutMode,
    parse_upstream_base_url, validate_deepseek_key,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the admin API key from the environment, or a default dev key.
fn admin_api_key() -> String {
    std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| "admin".to_string())
}

/// Middleware that checks for a valid admin API key in the `X-Admin-Key` header.
async fn admin_auth(req: Request, next: Next) -> Result<Response, StatusCode> {
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
        crab_control::ControlError::Http { status, .. } if *status == 400 => {
            StatusCode::BAD_REQUEST
        }
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
        .route(
            "/api/admin/metrics/prefix-cache",
            get(get_prefix_cache_metrics),
        )
        .route("/api/admin/gateway/health", get(get_gateway_health))
        .route("/api/admin/network/info", get(get_network_info))
        .route("/api/admin/keys", get(list_keys).post(create_key))
        .route("/api/admin/keys/{id}", delete(revoke_key))
        .route(
            "/api/admin/cache/config",
            get(get_cache_config).put(update_cache_config),
        )
        .route("/api/admin/cache/ops", get(get_cache_ops))
        .route("/api/admin/cache/invalidate", post(post_cache_invalidate))
        .route("/api/admin/cache/fingerprint", put(put_cache_fingerprint))
        .route(
            "/api/admin/cache/stream_cache",
            get(get_stream_cache).put(put_stream_cache),
        )
        .route(
            "/api/admin/semantic/config",
            get(get_semantic_config).put(update_semantic_config),
        )
        .route(
            "/api/admin/connection/config",
            get(get_connection_config).put(update_connection_config),
        )
        .route(
            "/api/admin/upstream/config",
            get(get_upstream_config).put(update_upstream_config),
        )
        .route("/api/admin/upstream/test", post(post_upstream_test))
        .route(
            "/api/admin/upstream/keys",
            get(get_upstream_keys_pool).put(put_upstream_keys_pool),
        )
        .route(
            "/api/admin/upstream/keys/{id}",
            patch(patch_upstream_key_pool),
        )
        .route("/api/admin/models", get(get_models).post(sync_models))
        .route("/api/admin/models/detect", post(post_models_detect))
        .route("/api/admin/models/apply", post(post_models_apply))
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
    Json(NetworkInfo::build(
        crate::network::NetworkInfoConfig::from_env(),
    ))
}

async fn get_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MetricsSnapshot>, StatusCode> {
    let metrics_url = std::env::var("CRABCACHE_GATEWAY_METRICS_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9090/metrics".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .http1_only()
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client.get(&metrics_url).send().await.map_err(|e| {
        tracing::warn!(url = %metrics_url, error = %e, "Failed to fetch gateway metrics");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    let body = resp
        .text()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

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
    let total_output_tokens =
        sum_prometheus_counter(&body, "gateway_deepseek_output_tokens_total", &[]);
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

    let prefix_cache_hit_tokens = sum_prometheus_counter(
        &body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "hit")],
    );
    let prefix_cache_miss_tokens = sum_prometheus_counter(
        &body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "miss")],
    );
    let prefix_cache_hit_ratio =
        prefix_hit_ratio(prefix_cache_hit_tokens, prefix_cache_miss_tokens);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let admin_uptime_secs = now.saturating_sub(state.start_time);
    let mut uptime_secs = admin_uptime_secs;
    let mut active_keys = state.keys_meta.len() as u64;
    if let Ok(status) = state.gateway.status().await {
        if status.uptime_secs > 0 {
            uptime_secs = status.uptime_secs;
        }
        active_keys = status.active_keys;
    }

    let total_requests = l0_hits + l1_hits + l2_hits + cache_misses;
    let qps = if uptime_secs > 0 {
        total_requests as f64 / uptime_secs as f64
    } else {
        0.0
    };

    let tps = if uptime_secs > 0 {
        (total_input_tokens_hit + total_input_tokens_miss + total_output_tokens) as f64
            / uptime_secs as f64
    } else {
        0.0
    };

    let hourly_stats = generate_hourly_stats_mock(
        uptime_secs,
        total_requests,
        total_input_tokens_hit + total_input_tokens_miss + total_output_tokens,
        l0_hits + l1_hits + l2_hits,
    );
    let daily_stats = generate_daily_stats_mock(
        uptime_secs,
        total_requests,
        total_input_tokens_hit + total_input_tokens_miss + total_output_tokens,
        l0_hits + l1_hits + l2_hits,
    );
    let weekly_stats = generate_weekly_stats_mock(
        uptime_secs,
        total_requests,
        total_input_tokens_hit + total_input_tokens_miss + total_output_tokens,
        l0_hits + l1_hits + l2_hits,
    );
    let monthly_stats = generate_monthly_stats_mock(
        uptime_secs,
        total_requests,
        total_input_tokens_hit + total_input_tokens_miss + total_output_tokens,
        l0_hits + l1_hits + l2_hits,
    );

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
        latency_upstream_ms: avg_prometheus_histogram_ms(
            &body,
            "gateway_upstream_latency_seconds",
            &[],
        ),
        active_keys,
        uptime_hours: uptime_secs / 3600,
        uptime_secs,
        hourly_stats,
        daily_stats,
        weekly_stats,
        monthly_stats,
        semantic_hits,
        semantic_rejected,
        semantic_skipped,
        prefix_cache_hit_tokens,
        prefix_cache_miss_tokens,
        prefix_cache_hit_ratio,
    }))
}

async fn get_prefix_cache_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PrefixCacheMetricsSnapshot>, StatusCode> {
    let metrics_url = std::env::var("CRABCACHE_GATEWAY_METRICS_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:9090/metrics".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .http1_only()
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client.get(&metrics_url).send().await.map_err(|e| {
        tracing::warn!(url = %metrics_url, error = %e, "Failed to fetch gateway metrics");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    let body = resp
        .text()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    let hit_tokens = sum_prometheus_counter(
        &body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "hit")],
    );
    let miss_tokens = sum_prometheus_counter(
        &body,
        "gateway_upstream_prompt_cache_tokens_total",
        &[("status", "miss")],
    );
    let by_model = prefix_cache_by_model(&body);

    let _ = state;

    Ok(Json(PrefixCacheMetricsSnapshot {
        hit_tokens,
        miss_tokens,
        hit_ratio: prefix_hit_ratio(hit_tokens, miss_tokens),
        by_model,
    }))
}

fn prefix_hit_ratio(hit: u64, miss: u64) -> f64 {
    let total = hit + miss;
    if total == 0 {
        0.0
    } else {
        hit as f64 / total as f64
    }
}

fn prefix_cache_by_model(body: &str) -> Vec<PrefixCacheModelBucket> {
    use std::collections::HashMap;
    let mut per_model: HashMap<String, (u64, u64)> = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || !line.starts_with("gateway_upstream_prompt_cache_tokens_total")
        {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        let labels = &line[open + 1..close];
        let status = label_value(labels, "status");
        let model = label_value(labels, "model").unwrap_or_else(|| "unknown".to_string());
        let value_part = line[close + 1..].trim();
        let Ok(tokens) = value_part.parse::<u64>() else {
            continue;
        };
        let entry = per_model.entry(model).or_insert((0, 0));
        match status.as_deref() {
            Some("hit") => entry.0 += tokens,
            Some("miss") => entry.1 += tokens,
            _ => {}
        }
    }
    let mut buckets: Vec<PrefixCacheModelBucket> = per_model
        .into_iter()
        .map(|(model, (hit, miss))| PrefixCacheModelBucket {
            hit_tokens: hit,
            miss_tokens: miss,
            hit_ratio: prefix_hit_ratio(hit, miss),
            model,
        })
        .collect();
    buckets.sort_by(|a, b| a.model.cmp(&b.model));
    buckets
}

fn label_value(labels: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = labels.find(&needle)? + needle.len();
    let rest = &labels[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
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

fn ensure_current_period_stats(
    mut stats: Vec<TimeSeriesPoint>,
    uptime_secs: u64,
    current_label: &str,
    total_requests: u64,
    total_tokens: u64,
    total_cache_hits: u64,
) -> Vec<TimeSeriesPoint> {
    if stats.is_empty() && uptime_secs > 0 {
        stats.push(TimeSeriesPoint {
            timestamp: current_label.to_string(),
            requests: total_requests,
            tokens: total_tokens,
            cache_hits: total_cache_hits,
            avg_latency_ms: 0.0,
        });
    }
    stats
}

fn generate_hourly_stats_mock(
    uptime_secs: u64,
    total_requests: u64,
    total_tokens: u64,
    total_cache_hits: u64,
) -> Vec<TimeSeriesPoint> {
    let hours = (uptime_secs / 3600).min(24) as usize;
    let mut stats = Vec::new();
    for i in 0..hours {
        let hour_ago = hours - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::hours(hour_ago as i64);
        let divisor = (hours as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%H:00").to_string(),
            requests: if i == 0 {
                total_requests
            } else {
                total_requests / divisor
            },
            tokens: if i == 0 {
                total_tokens
            } else {
                total_tokens / divisor
            },
            cache_hits: if i == 0 {
                total_cache_hits
            } else {
                total_cache_hits / divisor
            },
            avg_latency_ms: 0.0,
        });
    }
    ensure_current_period_stats(
        stats,
        uptime_secs,
        "now",
        total_requests,
        total_tokens,
        total_cache_hits,
    )
}

fn generate_daily_stats_mock(
    uptime_secs: u64,
    total_requests: u64,
    total_tokens: u64,
    total_cache_hits: u64,
) -> Vec<TimeSeriesPoint> {
    let days = (uptime_secs / 86400).min(7) as usize;
    let mut stats = Vec::new();
    for i in 0..days {
        let day_ago = days - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days(day_ago as i64);
        let divisor = (days as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%m-%d").to_string(),
            requests: if i == 0 {
                total_requests
            } else {
                total_requests / divisor
            },
            tokens: if i == 0 {
                total_tokens
            } else {
                total_tokens / divisor
            },
            cache_hits: if i == 0 {
                total_cache_hits
            } else {
                total_cache_hits / divisor
            },
            avg_latency_ms: 0.0,
        });
    }
    ensure_current_period_stats(
        stats,
        uptime_secs,
        "today",
        total_requests,
        total_tokens,
        total_cache_hits,
    )
}

fn generate_weekly_stats_mock(
    uptime_secs: u64,
    total_requests: u64,
    total_tokens: u64,
    total_cache_hits: u64,
) -> Vec<TimeSeriesPoint> {
    let weeks = (uptime_secs / 604800).min(4) as usize;
    let mut stats = Vec::new();
    for i in 0..weeks {
        let week_ago = weeks - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::weeks(week_ago as i64);
        let divisor = (weeks as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("W%U").to_string(),
            requests: if i == 0 {
                total_requests
            } else {
                total_requests / divisor
            },
            tokens: if i == 0 {
                total_tokens
            } else {
                total_tokens / divisor
            },
            cache_hits: if i == 0 {
                total_cache_hits
            } else {
                total_cache_hits / divisor
            },
            avg_latency_ms: 0.0,
        });
    }
    ensure_current_period_stats(
        stats,
        uptime_secs,
        "this week",
        total_requests,
        total_tokens,
        total_cache_hits,
    )
}

fn generate_monthly_stats_mock(
    uptime_secs: u64,
    total_requests: u64,
    total_tokens: u64,
    total_cache_hits: u64,
) -> Vec<TimeSeriesPoint> {
    let months = (uptime_secs / 2592000).min(12) as usize;
    let mut stats = Vec::new();
    for i in 0..months {
        let month_ago = months - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days((month_ago * 30) as i64);
        let divisor = (months as u64).max(1);
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%Y-%m").to_string(),
            requests: if i == 0 {
                total_requests
            } else {
                total_requests / divisor
            },
            tokens: if i == 0 {
                total_tokens
            } else {
                total_tokens / divisor
            },
            cache_hits: if i == 0 {
                total_cache_hits
            } else {
                total_cache_hits / divisor
            },
            avg_latency_ms: 0.0,
        });
    }
    ensure_current_period_stats(
        stats,
        uptime_secs,
        "this month",
        total_requests,
        total_tokens,
        total_cache_hits,
    )
}

fn generate_hourly_stats(
    metrics: &crate::state::StoredMetrics,
    uptime_secs: u64,
) -> Vec<TimeSeriesPoint> {
    let hours = (uptime_secs / 3600).min(24) as usize;
    let mut stats = Vec::new();

    for i in 0..hours {
        let hour_ago = hours - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::hours(hour_ago as i64);

        let requests = if i == 0 {
            metrics.total_requests
        } else {
            metrics.total_requests / (hours as u64).max(1)
        };
        let tokens = if i == 0 {
            metrics.total_input_tokens + metrics.total_output_tokens
        } else {
            (metrics.total_input_tokens + metrics.total_output_tokens) / (hours as u64).max(1)
        };
        let cache_hits = if i == 0 {
            metrics.l0_hits + metrics.l1_hits + metrics.l2_hits
        } else {
            (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (hours as u64).max(1)
        };
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

fn generate_daily_stats(
    metrics: &crate::state::StoredMetrics,
    uptime_secs: u64,
) -> Vec<TimeSeriesPoint> {
    let days = (uptime_secs / 86400).min(7) as usize;
    let mut stats = Vec::new();

    for i in 0..days {
        let day_ago = days - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days(day_ago as i64);

        let requests = if i == 0 {
            metrics.total_requests
        } else {
            metrics.total_requests / (days as u64).max(1)
        };
        let tokens = if i == 0 {
            metrics.total_input_tokens + metrics.total_output_tokens
        } else {
            (metrics.total_input_tokens + metrics.total_output_tokens) / (days as u64).max(1)
        };
        let cache_hits = if i == 0 {
            metrics.l0_hits + metrics.l1_hits + metrics.l2_hits
        } else {
            (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (days as u64).max(1)
        };
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

fn generate_weekly_stats(
    metrics: &crate::state::StoredMetrics,
    uptime_secs: u64,
) -> Vec<TimeSeriesPoint> {
    let weeks = (uptime_secs / 604800).min(4) as usize;
    let mut stats = Vec::new();

    for i in 0..weeks {
        let week_ago = weeks - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::weeks(week_ago as i64);

        let requests = if i == 0 {
            metrics.total_requests
        } else {
            metrics.total_requests / (weeks as u64).max(1)
        };
        let tokens = if i == 0 {
            metrics.total_input_tokens + metrics.total_output_tokens
        } else {
            (metrics.total_input_tokens + metrics.total_output_tokens) / (weeks as u64).max(1)
        };
        let cache_hits = if i == 0 {
            metrics.l0_hits + metrics.l1_hits + metrics.l2_hits
        } else {
            (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (weeks as u64).max(1)
        };
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

fn generate_monthly_stats(
    metrics: &crate::state::StoredMetrics,
    uptime_secs: u64,
) -> Vec<TimeSeriesPoint> {
    let months = (uptime_secs / 2592000).min(12) as usize;
    let mut stats = Vec::new();

    for i in 0..months {
        let month_ago = months - i - 1;
        let timestamp = chrono::Utc::now() - chrono::Duration::days((month_ago * 30) as i64);

        let requests = if i == 0 {
            metrics.total_requests
        } else {
            metrics.total_requests / (months as u64).max(1)
        };
        let tokens = if i == 0 {
            metrics.total_input_tokens + metrics.total_output_tokens
        } else {
            (metrics.total_input_tokens + metrics.total_output_tokens) / (months as u64).max(1)
        };
        let cache_hits = if i == 0 {
            metrics.l0_hits + metrics.l1_hits + metrics.l2_hits
        } else {
            (metrics.l0_hits + metrics.l1_hits + metrics.l2_hits) / (months as u64).max(1)
        };
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

async fn list_keys(State(state): State<Arc<AppState>>) -> Result<Json<Vec<ApiKey>>, StatusCode> {
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
                monthly_token_budget: meta.as_ref().map(|m| m.monthly_token_limit).unwrap_or(0),
                tokens_used_this_month: meta.as_ref().map(|m| m.tokens_this_month).unwrap_or(0),
                expired_at: meta.as_ref().and_then(|m| m.expired_at),
                model_limits: meta
                    .as_ref()
                    .map(|m| m.model_limits.clone())
                    .unwrap_or_default(),
                remain_quota: meta.as_ref().map(|m| m.remain_quota).unwrap_or(-1),
                unlimited_quota: meta.as_ref().map(|m| m.unlimited_quota).unwrap_or(true),
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

    let last_invalidate = state
        .last_invalidate
        .read()
        .clone()
        .map(|li| LastInvalidateView {
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
    let memory_logs = state.request_logs.read().clone();
    let trace_path = crate::trace_log::trace_log_path();
    let trace_entries = crate::trace_log::load_recent_trace_entries(&trace_path, 500);

    if !memory_logs.is_empty() {
        let result: Vec<RequestLog> = memory_logs
            .into_iter()
            .map(|log| {
                RequestLog {
                    id: log.id,
                    timestamp: crate::trace_log::format_beijing_from_unix_secs(log.timestamp),
                    model: log.model.clone(),
                    consumer: log.consumer.clone(),
                    latency_ms: log.duration_ms as u64,
                    total_tokens: log.input_tokens + log.output_tokens,
                    cache_status: log.cache_tier.clone(),
                    request_payload: serde_json::to_string_pretty(&log.request_payload)
                        .unwrap_or_default(),
                    response_preview: log.response_body.chars().take(200).collect(),
                }
            })
            .collect();
        return Json(result);
    }

    let result: Vec<RequestLog> = trace_entries
        .into_iter()
        .map(|e| {
            let datetime =
                crate::trace_log::format_beijing_from_millis(e.timestamp_ms as i64);
            let consumer = e
                .conversation_id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "—".to_string());
            let summary = serde_json::json!({
                "request_hash": e.request_hash,
                "content_length": e.content_length,
                "semantic_cluster": e.semantic_cluster,
                "prompt_tokens": e.prompt_tokens,
                "cache_hit": e.cache_hit,
                "cache_tier": e.cache_tier,
            });
            RequestLog {
                id: e.id(),
                timestamp: datetime,
                model: e.model.clone(),
                consumer,
                latency_ms: e.latency_ms.round() as u64,
                total_tokens: e.prompt_tokens as u64,
                cache_status: e.cache_status_label(),
                request_payload: serde_json::to_string_pretty(&summary).unwrap_or_default(),
                response_preview: String::new(),
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
    if let Some(log) = logs.iter().find(|l| l.id == id) {
        return Ok(Json(RequestDetail {
            cache_path: log.cache_path.join(" → "),
            request_payload: serde_json::to_string_pretty(&log.request_payload).unwrap_or_default(),
            response_body: log.response_body.clone(),
            route_backend: log.route_backend.clone(),
        }));
    }

    let trace_path = crate::trace_log::trace_log_path();
    if let Some(entry) = crate::trace_log::find_trace_entry(&trace_path, &id) {
        let cache_path = if entry.cache_hit {
            entry
                .cache_tier
                .clone()
                .unwrap_or_else(|| "gateway-cache".to_string())
        } else {
            "upstream".to_string()
        };
        let payload = serde_json::json!({
            "request_hash": entry.request_hash,
            "content_length": entry.content_length,
            "semantic_cluster": entry.semantic_cluster,
            "conversation_id": entry.conversation_id,
            "model": entry.model,
            "prompt_tokens": entry.prompt_tokens,
            "latency_ms": entry.latency_ms,
            "cache_hit": entry.cache_hit,
            "cache_tier": entry.cache_tier,
        });
        return Ok(Json(RequestDetail {
            cache_path,
            request_payload: serde_json::to_string_pretty(&payload).unwrap_or_default(),
            response_body: "(影子日志不含响应正文；仅记录脱敏元数据)".to_string(),
            route_backend: "—".to_string(),
        }));
    }

    Err(StatusCode::NOT_FOUND)
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

async fn sync_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SyncResult>, (StatusCode, String)> {
    crate::upstream::sync_models_internal(&state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn post_models_detect(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::types::ModelDetectResponse>, (StatusCode, String)> {
    crate::upstream::detect_models_internal(&state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn post_models_apply(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::types::ModelApplyBody>,
) -> Result<Json<SyncResult>, StatusCode> {
    let result = crate::upstream::apply_models_internal(&state, body.add, body.remove);
    Ok(Json(result))
}

async fn post_upstream_test(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::types::UpstreamTestBody>,
) -> Json<crab_control::UpstreamTestResult> {
    let result = crate::upstream::test_upstream_connection(&body.base_url, &body.api_key).await;
    *state.last_upstream_test.write() = Some(result.clone());
    state.flush_persist();
    Json(result)
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

async fn get_upstream_keys_pool(
    State(state): State<Arc<AppState>>,
) -> Result<Json<UpstreamKeysView>, (StatusCode, String)> {
    state
        .gateway
        .get_upstream_keys()
        .await
        .map(Json)
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))
}

async fn put_upstream_keys_pool(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PutUpstreamKeysRequest>,
) -> Result<Json<UpstreamKeysView>, (StatusCode, String)> {
    for (i, k) in req.keys.iter().enumerate() {
        if let Err(e) = validate_deepseek_key(&k.secret) {
            return Err((StatusCode::BAD_REQUEST, format!("key #{}: {e}", i + 1)));
        }
    }

    if req.mode == UpstreamKeysPutMode::Replace {
        state.replace_upstream_pool_secrets(&req.keys);
    } else {
        let mut merged = state.upstream_pool_secrets.read().clone();
        let mut seen: std::collections::HashSet<String> =
            merged.iter().map(|s| s.secret.clone()).collect();
        for k in &req.keys {
            let secret = k.secret.trim().to_string();
            if secret.is_empty() || seen.contains(&secret) {
                continue;
            }
            seen.insert(secret.clone());
            merged.push(crate::state::UpstreamPoolSecret {
                id: if k.id.is_empty() {
                    format!("key-{}", merged.len() + 1)
                } else {
                    k.id.clone()
                },
                secret,
                enabled: k.enabled,
            });
        }
        let inputs: Vec<UpstreamKeyInput> = merged
            .iter()
            .map(|s| UpstreamKeyInput {
                id: s.id.clone(),
                secret: s.secret.clone(),
                enabled: s.enabled,
            })
            .collect();
        state.replace_upstream_pool_secrets(&inputs);
    }

    let view = state
        .gateway
        .put_upstream_keys(&req)
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    state.flush_persist();
    Ok(Json(view))
}

async fn patch_upstream_key_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchUpstreamKeyRequest>,
) -> Result<Json<UpstreamKeyView>, (StatusCode, String)> {
    state
        .gateway
        .patch_upstream_key(&id, &req)
        .await
        .map(Json)
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))
}

fn build_upstream_config_view(state: &AppState) -> UpstreamConfig {
    let config = state.upstream_config.read().clone();
    let pool_count = state
        .upstream_pool_secrets
        .read()
        .iter()
        .filter(|k| k.enabled && !k.secret.is_empty())
        .count();
    let key_pool_count = pool_count.max(usize::from(!state.upstream_api_key.is_empty()));
    UpstreamConfig {
        base_url: config.base_url,
        model: config.model,
        endpoints: config.endpoints,
        key_pool_count,
        gateway_reachable: *state.gateway_reachable.read(),
        last_test: state.last_upstream_test.read().clone(),
        api_key: String::new(),
        api_key_masked: String::new(),
    }
}

async fn get_upstream_config(State(state): State<Arc<AppState>>) -> Json<UpstreamConfig> {
    state.reconcile_upstream_from_gateway().await;
    Json(build_upstream_config_view(&state))
}

async fn update_upstream_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateUpstreamConfigRequest>,
) -> Result<Json<crate::types::UpdateUpstreamConfigResponse>, (StatusCode, String)> {
    let parsed =
        parse_upstream_base_url(&req.base_url).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let model = req.model.trim();
    if model.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "model must not be empty".to_string(),
        ));
    }

    let endpoints = if req.endpoints.is_empty() {
        None
    } else {
        Some(req.endpoints.clone())
    };

    let relay_req = crab_control::PutUpstreamRelayConfigRequest {
        base_url: parsed.normalized.clone(),
        model: Some(model.to_string()),
        endpoints,
        tls_sni: Some(parsed.tls_sni.clone()),
    };

    state
        .gateway
        .put_upstream_relay(&relay_req)
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    {
        let mut config = state.upstream_config.write();
        config.base_url = parsed.normalized.clone();
        config.model = model.to_string();
        if req.endpoints.is_empty() {
            config.endpoints = vec![parsed.endpoint.clone()];
        } else {
            config.endpoints = req.endpoints.clone();
        }
        if let Some(key) = &req.api_key {
            if !key.is_empty() && !key.contains("****") {
                if let Err(e) = validate_deepseek_key(key) {
                    return Err((StatusCode::BAD_REQUEST, e));
                }
                config.api_key = key.clone();
            }
        }
    }

    let mut keys_to_push: Vec<String> = req.keys_to_append.clone();
    if let Some(key) = &req.api_key {
        if !key.is_empty() && !key.contains("****") {
            keys_to_push.push(key.clone());
        }
    }
    keys_to_push.retain(|k| !k.trim().is_empty());
    if !keys_to_push.is_empty() {
        let inputs: Vec<UpstreamKeyInput> = keys_to_push
            .into_iter()
            .enumerate()
            .map(|(i, secret)| UpstreamKeyInput {
                id: format!("key-{}", i + 1),
                secret,
                enabled: true,
            })
            .collect();
        let put_req = PutUpstreamKeysRequest {
            keys: inputs.clone(),
            mode: UpstreamKeysPutMode::Append,
        };
        let mut merged = state.upstream_pool_secrets.read().clone();
        let mut seen: std::collections::HashSet<String> =
            merged.iter().map(|s| s.secret.clone()).collect();
        for k in &inputs {
            if k.secret.is_empty() || seen.contains(&k.secret) {
                continue;
            }
            seen.insert(k.secret.clone());
            merged.push(crate::state::UpstreamPoolSecret {
                id: if k.id.is_empty() {
                    format!("key-{}", merged.len() + 1)
                } else {
                    k.id.clone()
                },
                secret: k.secret.clone(),
                enabled: k.enabled,
            });
        }
        let merged_inputs: Vec<UpstreamKeyInput> = merged
            .iter()
            .map(|s| UpstreamKeyInput {
                id: s.id.clone(),
                secret: s.secret.clone(),
                enabled: s.enabled,
            })
            .collect();
        state.replace_upstream_pool_secrets(&merged_inputs);
        let _ = state.gateway.put_upstream_keys(&put_req).await;
    }

    {
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
    }

    *state.gateway_reachable.write() = true;

    let sync = if state.pick_sync_api_key().is_some() {
        match crate::upstream::sync_models_internal(&state).await {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!(error = %e, "Auto model sync after upstream save failed");
                None
            }
        }
    } else {
        None
    };

    state.flush_persist();

    Ok(Json(crate::types::UpdateUpstreamConfigResponse {
        config: build_upstream_config_view(&state),
        sync,
    }))
}

async fn get_trace_analysis(State(_state): State<Arc<AppState>>) -> Json<TraceAnalysis> {
    use std::collections::HashMap;

    let trace_path = std::env::var("CRABCACHE_TRACE_LOG_PATH")
        .unwrap_or_else(|_| "/app/logs/trace.jsonl".to_string());

    let trace_entries: Vec<crate::state::StoredTraceEntry> =
        match std::fs::read_to_string(&trace_path) {
            Ok(content) => content
                .lines()
                .filter_map(|line| {
                    let entry: serde_json::Value = serde_json::from_str(line).ok()?;
                    Some(crate::state::StoredTraceEntry {
                        timestamp_ms: entry.get("timestamp_ms")?.as_u64()?,
                        request_hash: entry.get("request_hash")?.as_str()?.to_string(),
                        content_length: entry.get("content_length")?.as_u64()? as usize,
                        semantic_cluster: entry.get("semantic_cluster")?.as_u64()? as usize,
                        conversation_id: entry
                            .get("conversation_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        model: entry.get("model")?.as_str()?.to_string(),
                        prompt_tokens: entry.get("prompt_tokens")?.as_u64()? as usize,
                        latency_ms: entry.get("latency_ms")?.as_f64()?,
                        cache_hit: entry.get("cache_hit")?.as_bool()?,
                    })
                })
                .collect(),
            Err(e) => {
                tracing::warn!("Failed to read trace log file {}: {}", trace_path, e);
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
        };

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

#[cfg(test)]
mod metrics_tests {
    use super::{
        ensure_current_period_stats, generate_hourly_stats_mock, sum_prometheus_counter,
    };

    #[test]
    fn hourly_stats_include_current_bucket_under_one_hour() {
        let stats = generate_hourly_stats_mock(372, 3, 100, 1);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].timestamp, "now");
        assert_eq!(stats[0].requests, 3);
    }

    #[test]
    fn ensure_current_period_noop_when_nonempty() {
        let existing = vec![super::TimeSeriesPoint {
            timestamp: "12:00".into(),
            requests: 1,
            tokens: 2,
            cache_hits: 0,
            avg_latency_ms: 0.0,
        }];
        let out = ensure_current_period_stats(existing.clone(), 100, "now", 9, 9, 9);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].timestamp, "12:00");
    }

    #[test]
    fn sum_across_multiple_model_labels() {
        let body = r#"
gateway_deepseek_input_tokens_total{cache_status="hit",model="m1",consumer="c"} 10
gateway_deepseek_input_tokens_total{cache_status="hit",model="m2",consumer="c"} 20
gateway_deepseek_input_tokens_total{cache_status="miss",model="m1",consumer="c"} 5
"#;
        assert_eq!(
            sum_prometheus_counter(
                body,
                "gateway_deepseek_input_tokens_total",
                &[("cache_status", "hit")]
            ),
            30
        );
        assert_eq!(
            sum_prometheus_counter(
                body,
                "gateway_deepseek_input_tokens_total",
                &[("cache_status", "miss")]
            ),
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
