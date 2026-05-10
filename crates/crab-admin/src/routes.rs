use crate::state::AppState;
use crate::types::*;
use crate::network::NetworkInfo;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/admin/metrics", get(get_metrics))
        .route("/api/admin/network/info", get(get_network_info))
        .route("/api/admin/keys", get(list_keys).post(create_key))
        .route("/api/admin/keys/{id}", delete(revoke_key))
        .route("/api/admin/cache/config", get(get_cache_config).put(update_cache_config))
        .route("/api/admin/semantic/config", get(get_semantic_config).put(update_semantic_config))
        .route("/api/admin/connection/config", get(get_connection_config).put(update_connection_config))
        .route("/api/admin/upstream/config", get(get_upstream_config).put(update_upstream_config))
        .route("/api/admin/models", get(get_models).post(sync_models))
        .route("/api/admin/routing/status", get(get_routing_status))
        .route("/api/admin/logs", get(get_logs))
        .route("/api/admin/logs/{id}", get(get_log_detail))
        .route("/api/admin/trace/analysis", get(get_trace_analysis))
        .with_state(state)
}

async fn get_network_info() -> Json<NetworkInfo> {
    let use_https = std::env::var("CRABCACHE_HTTPS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    Json(NetworkInfo::new(8080, use_https))
}

async fn get_metrics(State(state): State<Arc<AppState>>) -> Json<MetricsSnapshot> {
    let metrics = state.metrics.read().clone();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let uptime_secs = now.saturating_sub(state.start_time);

    let qps = if uptime_secs > 0 {
        metrics.total_requests as f64 / uptime_secs as f64
    } else {
        0.0
    };

    let tps = if uptime_secs > 0 {
        (metrics.total_input_tokens + metrics.total_output_tokens) as f64 / uptime_secs as f64
    } else {
        0.0
    };

    let hourly_stats = generate_hourly_stats(&metrics, uptime_secs);
    let daily_stats = generate_daily_stats(&metrics, uptime_secs);
    let weekly_stats = generate_weekly_stats(&metrics, uptime_secs);
    let monthly_stats = generate_monthly_stats(&metrics, uptime_secs);

    Json(MetricsSnapshot {
        qps,
        tps,
        l0_hits: metrics.l0_hits,
        l1_hits: metrics.l1_hits,
        l2_hits: metrics.l2_hits,
        cache_misses: metrics.cache_misses,
        cache_hit_tokens: metrics.cache_hit_tokens,
        cache_miss_tokens: metrics.cache_miss_tokens,
        total_input_tokens: metrics.total_input_tokens,
        total_output_tokens: metrics.total_output_tokens,
        total_tokens: metrics.total_input_tokens + metrics.total_output_tokens,
        latency_l0_ms: if metrics.l0_latency_count > 0 {
            metrics.l0_latency_sum_ms / metrics.l0_latency_count as f64
        } else {
            0.0
        },
        latency_l1_ms: if metrics.l1_latency_count > 0 {
            metrics.l1_latency_sum_ms / metrics.l1_latency_count as f64
        } else {
            0.0
        },
        latency_l2_ms: if metrics.l2_latency_count > 0 {
            metrics.l2_latency_sum_ms / metrics.l2_latency_count as f64
        } else {
            0.0
        },
        latency_upstream_ms: if metrics.upstream_latency_count > 0 {
            metrics.upstream_latency_sum_ms / metrics.upstream_latency_count as f64
        } else {
            0.0
        },
        active_keys: state.keys.len() as u64,
        uptime_hours: uptime_secs / 3600,
        hourly_stats,
        daily_stats,
        weekly_stats,
        monthly_stats,
    })
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

async fn list_keys(State(state): State<Arc<AppState>>) -> Json<Vec<ApiKey>> {
    let keys: Vec<ApiKey> = state
        .keys
        .iter()
        .map(|entry| {
            let key = entry.value();
            ApiKey {
                id: key.id.clone(),
                name: key.name.clone(),
                key_preview: key.key_hash[..10.min(key.key_hash.len())].to_string(),
                key_full: Some(key.key_hash.clone()),
                active: key.enabled,
                rpm_limit: key.rpm_limit as u32,
                monthly_token_budget: key.monthly_token_limit,
                tokens_used_this_month: key.tokens_this_month,
                expired_at: key.expired_at,
                model_limits: key.model_limits.clone(),
                remain_quota: key.remain_quota,
                unlimited_quota: key.unlimited_quota,
            }
        })
        .collect();

    Json(keys)
}

async fn create_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<ApiKey>, StatusCode> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let key_full = format!("sk-cc-{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..24]);

    let stored = crate::state::StoredKey {
        id: id.clone(),
        name: req.name.clone(),
        key_hash: key_full.clone(),
        created_at: now,
        enabled: true,
        rpm_limit: req.rpm_limit as u64,
        monthly_token_limit: req.monthly_token_budget,
        current_rpm: 0,
        tokens_this_month: 0,
        input_tokens: 0,
        output_tokens: 0,
        expired_at: req.expired_at,
        model_limits: req.model_limits.unwrap_or_default(),
        remain_quota: req.remain_quota.unwrap_or(-1),
        unlimited_quota: req.unlimited_quota.unwrap_or(true),
    };

    let api_key = ApiKey {
        id: stored.id.clone(),
        name: stored.name.clone(),
        key_preview: stored.key_hash[..10.min(stored.key_hash.len())].to_string(),
        key_full: Some(stored.key_hash.clone()),
        active: stored.enabled,
        rpm_limit: stored.rpm_limit as u32,
        monthly_token_budget: stored.monthly_token_limit,
        tokens_used_this_month: 0,
        expired_at: stored.expired_at,
        model_limits: stored.model_limits.clone(),
        remain_quota: stored.remain_quota,
        unlimited_quota: stored.unlimited_quota,
    };

    state.keys.insert(id, stored);

    Ok(Json(api_key))
}

async fn revoke_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> StatusCode {
    if state.keys.remove(&id).is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn get_cache_config(State(state): State<Arc<AppState>>) -> Json<CacheConfig> {
    let config = state.cache_config.read().clone();
    Json(CacheConfig {
        l0_ttl_secs: config.l0_ttl_secs,
        l1_ttl_secs: config.l1_ttl_secs,
        default_ttl_secs: config.default_ttl_secs,
    })
}

async fn update_cache_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateCacheConfigRequest>,
) -> Json<CacheConfig> {
    let mut config = state.cache_config.write();
    config.l0_ttl_secs = req.l0_ttl_secs;
    config.l1_ttl_secs = req.l1_ttl_secs;

    Json(CacheConfig {
        l0_ttl_secs: config.l0_ttl_secs,
        l1_ttl_secs: config.l1_ttl_secs,
        default_ttl_secs: config.default_ttl_secs,
    })
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

async fn get_routing_status(State(state): State<Arc<AppState>>) -> Json<RoutingStatus> {
    let backends = state.backends.read().clone();
    let total_requests: u64 = backends.iter().map(|b| b.request_count).sum();
    let active_count = backends.iter().filter(|b| b.healthy).count();

    Json(RoutingStatus {
        total_backends: backends.len(),
        active_backends: active_count,
        total_requests,
        backends: backends
            .into_iter()
            .map(|b| BackendStatus {
                name: b.name,
                request_count: b.request_count,
                healthy: b.healthy,
            })
            .collect(),
    })
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

async fn get_upstream_config(State(state): State<Arc<AppState>>) -> Json<UpstreamConfig> {
    let config = state.upstream_config.read().clone();
    let api_key_masked = mask_api_key(&config.api_key);
    Json(UpstreamConfig {
        base_url: config.base_url,
        api_key: config.api_key.clone(),
        api_key_masked,
        endpoints: config.endpoints,
    })
}

async fn update_upstream_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateUpstreamConfigRequest>,
) -> Json<UpstreamConfig> {
    let mut config = state.upstream_config.write();
    config.base_url = req.base_url;
    if let Some(key) = req.api_key {
        if !key.is_empty() && !key.contains("****") {
            config.api_key = key;
        }
    }
    config.endpoints = req.endpoints;

    let api_key_masked = mask_api_key(&config.api_key);
    Json(UpstreamConfig {
        base_url: config.base_url.clone(),
        api_key: config.api_key.clone(),
        api_key_masked,
        endpoints: config.endpoints.clone(),
    })
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