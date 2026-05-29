use crate::metrics_history::{
    domain_consumer_buckets, domain_tier_deltas_5m, domain_token_buckets,
};
use crate::network::NetworkInfo;
use crate::state::{AppState, KeyMetadata};
use crate::types::*;
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use crab_control::{
    CreateGatewayKeyRequest, CursorModelsConfigView, FingerprintConfigRequest,
    InvalidateCacheRequest, PutTtlConfigRequest, constant_time_eq_str, parse_upstream_base_url,
    validate_deepseek_key,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, serde::Serialize)]
struct ErrorResponse {
    error: String,
}

async fn api_not_found(Path(path): Path<String>) -> impl IntoResponse {
    // Backward-compatible helper (kept for compatibility; prefer `OriginalUri` fallback).
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("not_found: /api/admin/{path}"),
        }),
    )
}

/// Middleware that checks for a valid admin API key in the `X-Admin-Key` header.
async fn admin_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let expected_key = state.admin_key.read().clone();
    let provided_key = req
        .headers()
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !constant_time_eq_str(provided_key, &expected_key) {
        tracing::warn!("Admin API authentication failed");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        )
            .into_response());
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
    let unauthenticated = Router::new()
        .route(
            "/api/admin/infra/speed-test/upload",
            post(post_infra_speed_test_upload),
        )
        .route("/api/admin/events", get(crate::sse::sse_events))
        .with_state(state.clone());

    let protected = Router::new()
        .route("/api/admin/events/token", get(crate::sse::sse_token))
        .route("/api/admin/metrics", get(get_metrics))
        .route("/api/admin/overview", get(get_overview))
        .route("/api/admin/overview/core", get(get_overview_core))
        .route(
            "/api/admin/overview/timeseries",
            get(get_overview_timeseries),
        )
        .route("/api/admin/overview/trace", get(get_overview_trace))
        .route("/api/admin/domains", get(list_domains))
        .route("/api/admin/domains/:domain", get(get_domain_detail))
        .route(
            "/api/admin/domains/policies",
            get(get_domain_policies).put(put_domain_policies),
        )
        .route(
            "/api/admin/metrics/prefix-cache",
            get(get_prefix_cache_metrics),
        )
        .route("/api/admin/gateway/health", get(get_gateway_health))
        .route("/api/admin/pg/health", get(get_pg_health))
        .route("/api/admin/network/info", get(get_network_info))
        .route("/api/admin/keys", get(list_keys).post(create_key))
        .route("/api/admin/keys/:id", delete(revoke_key).patch(patch_key))
        .route("/api/admin/keys/:id/concurrency", get(get_key_concurrency))
        .route("/api/admin/keys/:id/routing", get(get_key_routing))
        .route(
            "/api/admin/sessions/:fingerprint",
            get(get_session_timeline),
        )
        .route("/api/admin/keys/batch-revoke", post(batch_revoke_keys))
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
            "/api/admin/runtime/pipeline",
            get(get_pipeline_runtime).put(put_pipeline_runtime),
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
            "/api/admin/reasoning/config",
            get(get_reasoning_config).put(put_reasoning_config),
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
            "/api/admin/upstream/keys/:id",
            patch(patch_upstream_key_pool),
        )
        .route(
            "/api/admin/upstream/profiles",
            get(crate::upstream_profiles::list_profiles_json),
        )
        .route(
            "/api/admin/upstream/profiles/:id",
            axum::routing::put(put_upstream_profile).delete(delete_upstream_profile),
        )
        .route(
            "/api/admin/upstream/profiles/:id/keys",
            get(get_upstream_profile_keys).put(put_upstream_profile_keys),
        )
        .route(
            "/api/admin/upstream/profiles/:id/keys/:key_id",
            patch(patch_upstream_profile_key),
        )
        .route(
            "/api/admin/upstream/profiles/:id/keys/:key_id/test",
            post(post_upstream_profile_key_test),
        )
        .route(
            "/api/admin/upstream/profiles/:id/test",
            post(post_upstream_profile_test),
        )
        .route(
            "/api/admin/upstream/profiles/:id/routing",
            get(get_upstream_profile_routing),
        )
        .route("/api/admin/models", get(get_models).post(sync_models))
        .route("/api/admin/models/detect", post(post_models_detect))
        .route("/api/admin/models/apply", post(post_models_apply))
        .route("/api/admin/routing/status", get(get_routing_status))
        .route("/api/admin/routing/backends", put(put_routing_backends))
        // ── Codex OAuth Device Login ──
        .route(
            "/api/admin/upstream/profiles/:id/oauth/codex/device/start",
            post(crate::oauth_codex::start_device_login),
        )
        .route(
            "/api/admin/upstream/profiles/:id/oauth/codex/device/:session_id",
            get(crate::oauth_codex::poll_device_status)
                .delete(crate::oauth_codex::cancel_device_login),
        )
        .route(
            "/api/admin/upstream/profiles/:id/oauth/codex/import",
            post(crate::oauth_codex::import_codex_credential),
        )
        .route(
            "/api/admin/oauth/codex/credentials",
            get(crate::oauth_codex::list_codex_credentials),
        )
        // ── Codex OAuth PKCE Login ──
        .route(
            "/api/admin/upstream/profiles/:id/oauth/codex/pkce/start",
            post(crate::oauth_codex::start_pkce_login),
        )
        .route(
            "/api/admin/upstream/profiles/:id/oauth/codex/pkce/:session_id",
            get(crate::oauth_codex::poll_pkce_status)
                .delete(crate::oauth_codex::cancel_pkce_login),
        )
        .route(
            "/api/admin/upstream/profiles/:id/oauth/codex/pkce/:session_id/exchange",
            post(crate::oauth_codex::exchange_pkce),
        )
        .route(
            "/api/admin/cursor/models",
            get(get_cursor_models).put(put_cursor_models),
        )
        .route("/api/admin/logs", get(get_logs))
        .route("/api/admin/logs/:id", get(get_log_detail))
        // ── Log Management ──
        .route("/api/admin/logs/usage", get(get_log_disk_usage))
        .route("/api/admin/logs/clear", post(post_clear_logs))
        .route(
            "/api/admin/logs/retention",
            get(get_retention_policy).put(put_retention_policy),
        )
        .route("/api/admin/trace/analysis", get(get_trace_analysis))
        .route("/api/admin/live-metrics", get(get_live_metrics))
        .route("/api/admin/live-metrics/consumers", get(get_live_consumers))
        .route("/api/admin/system/admin-key", put(put_admin_key))
        .route("/api/admin/system/version", get(get_system_version))
        .route("/api/admin/system/check-update", post(post_check_update))
        .route("/api/admin/system/update", post(post_system_update))
        .route(
            "/api/admin/composition/summary",
            get(crate::composition::get_composition_summary),
        )
        .route(
            "/api/admin/composition/trends",
            get(crate::composition::get_composition_trends),
        )
        .route(
            "/api/admin/composition/debug",
            get(crate::composition::get_composition_debug),
        )
        // ── Raw Capture ──
        .route(
            "/api/admin/capture/list",
            get(crate::raw_capture::get_capture_list),
        )
        .route(
            "/api/admin/capture/stats",
            get(crate::raw_capture::get_capture_stats),
        )
        .route(
            "/api/admin/capture/:request_id",
            get(crate::raw_capture::get_capture_detail),
        )
        // ── Audit Log ──
        .route("/api/admin/audit-log", get(get_audit_log))
        // ── Infra (container / host monitoring) ──
        .route("/api/admin/infra/snapshot", get(get_infra_snapshot))
        .route("/api/admin/infra/status", get(get_infra_status))
        .route("/api/admin/infra/timeseries", get(get_infra_timeseries))
        .route("/api/admin/infra/speed-test", post(post_infra_speed_test))
        .route(
            "/api/admin/infra/speed-test/:job_id",
            get(get_infra_speed_test_job),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), admin_auth))
        .with_state(state);

    unauthenticated.merge(protected)
}

/// Request body for changing the admin API key.
#[derive(Debug, Deserialize)]
struct ChangeAdminKeyRequest {
    old_key: String,
    new_key: String,
}

async fn put_admin_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChangeAdminKeyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current = state.admin_key.read().clone();
    if !constant_time_eq_str(&req.old_key, &current) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if req.new_key.is_empty() || req.new_key.len() < 4 {
        return Ok(Json(serde_json::json!({
            "error": "New key must be at least 4 characters"
        })));
    }

    let state_dir = std::env::var("CRABCACHE_ADMIN_STATE_PATH")
        .ok()
        .and_then(|p| std::path::Path::new(&p).parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("data"));
    let key_path = state_dir.join("admin-key.txt");

    if let Err(e) = std::fs::create_dir_all(&state_dir) {
        return Ok(Json(serde_json::json!({
            "error": format!("Failed to create data dir: {e}")
        })));
    }
    if let Err(e) = std::fs::write(&key_path, &req.new_key) {
        return Ok(Json(serde_json::json!({
            "error": format!("Failed to persist key: {e}")
        })));
    }

    *state.admin_key.write() = req.new_key;
    tracing::info!("Admin API key changed successfully");

    Ok(Json(serde_json::json!({"success": true})))
}

/// Version information including the latest GitHub release if reachable.
async fn get_system_version(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current_version = &state.current_version;
    // Try to fetch latest release from GitHub (non-blocking, soft-fail on error).
    let latest = match crate::update::check_latest_release().await {
        Ok(release) => {
            let latest_version = release.tag_name.trim_start_matches('v');
            serde_json::json!({
                "tag_name": release.tag_name,
                "published_at": release.published_at,
                "assets": release.assets.iter().map(|a| serde_json::json!({
                    "name": a.name,
                    "download_url": a.browser_download_url,
                    "size": a.size,
                })).collect::<Vec<_>>(),
                "update_available": latest_version != current_version,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to fetch latest GitHub release for version endpoint");
            serde_json::json!(null)
        }
    };

    Ok(Json(serde_json::json!({
        "current_version": current_version,
        "latest": latest,
    })))
}

/// Explicitly check for updates from GitHub.
async fn post_check_update(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match crate::update::check_update(&state.current_version).await {
        Ok(result) => Ok(Json(serde_json::json!({
            "current_version": result.current_version,
            "latest_version": result.latest_version,
            "update_available": result.update_available,
            "release": result.release.map(|r| serde_json::json!({
                "tag_name": r.tag_name,
                "published_at": r.published_at,
                "assets": r.assets.iter().map(|a| serde_json::json!({
                    "name": a.name,
                    "download_url": a.browser_download_url,
                    "size": a.size,
                })).collect::<Vec<_>>(),
            })),
        }))),
        Err(e) => {
            tracing::warn!(error = %e, "Update check failed");
            Ok(Json(serde_json::json!({
                "error": e
            })))
        }
    }
}

/// Trigger a full system update: download latest binaries from GitHub,
/// replace gateway and admin, and restart both services.
async fn post_system_update(
    State(_state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Step 1: Get the latest release from GitHub
    let release = match crate::update::check_latest_release().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "check_release",
                "error": e,
            })));
        }
    };

    let download_dir = std::path::PathBuf::from("/tmp/crabcache-update");
    let _ = std::fs::create_dir_all(&download_dir);

    // Step 2: Download gateway binary
    let gateway_asset = release.assets.iter().find(|a| a.name == "crab-gateway");

    let gateway_path = download_dir.join("crab-gateway");
    if let Some(asset) = gateway_asset {
        if let Err(e) =
            crate::update::download_asset(&asset.browser_download_url, &gateway_path).await
        {
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "download_gateway",
                "error": e,
            })));
        }
        // Verify checksum
        if let Err(e) =
            crate::update::verify_checksum("crab-gateway", &gateway_path, &release).await
        {
            tracing::warn!(error = %e, "Gateway checksum verification failed");
            let _ = std::fs::remove_file(&gateway_path);
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "verify_gateway",
                "error": e,
            })));
        }
    } else {
        tracing::warn!("No crab-gateway asset found in release");
    }

    // Step 3: Download admin binary
    let admin_asset = release.assets.iter().find(|a| a.name == "crab-admin");
    let admin_path = download_dir.join("crab-admin");
    if let Some(asset) = admin_asset {
        if let Err(e) =
            crate::update::download_asset(&asset.browser_download_url, &admin_path).await
        {
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "download_admin",
                "error": e,
            })));
        }
        // Verify checksum
        if let Err(e) = crate::update::verify_checksum("crab-admin", &admin_path, &release).await {
            tracing::warn!(error = %e, "Admin checksum verification failed");
            let _ = std::fs::remove_file(&admin_path);
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "verify_admin",
                "error": e,
            })));
        }
    } else {
        tracing::warn!("No crab-admin asset found in release");
    }

    // Step 4: Replace gateway binary and restart it
    let gateway_target = std::env::var("CRABCACHE_GATEWAY_BINARY_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/app/crab-gateway"));

    if gateway_path.exists() {
        if let Err(e) = crate::update::replace_binary(&gateway_path, &gateway_target) {
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "replace_gateway",
                "error": e,
            })));
        }

        let gateway_control_url = std::env::var("CRABCACHE_GATEWAY_CONTROL_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:9080".to_string());
        let gateway_admin_key =
            std::env::var("CRABCACHE_GATEWAY_ADMIN_KEY").unwrap_or_else(|_| String::new());

        if let Err(e) =
            crate::update::restart_gateway(&gateway_control_url, &gateway_admin_key).await
        {
            return Ok(Json(serde_json::json!({
                "success": false,
                "stage": "restart_gateway",
                "error": e,
            })));
        }

        // Wait a moment for gateway to begin restarting
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Step 5: Self-update admin binary and restart
    let result = if admin_path.exists() {
        match crate::update::self_update_and_restart(&admin_path) {
            Ok(()) => {
                // The restart script is now running. The current process will exit
                // after this response. Give the script a moment to take over.
                tracing::info!("Self-update initiated, exiting after response");
                serde_json::json!({
                    "success": true,
                    "message": "Update complete. Admin is restarting.",
                    "tag": release.tag_name,
                })
            }
            Err(e) => {
                serde_json::json!({
                    "success": false,
                    "stage": "self_update",
                    "error": e,
                })
            }
        }
    } else if gateway_asset.is_some() {
        serde_json::json!({
            "success": true,
            "message": "Gateway updated and restarting. No admin binary in release.",
            "tag": release.tag_name,
        })
    } else {
        serde_json::json!({
            "success": false,
            "stage": "no_assets",
            "error": "No gateway or admin assets found in release",
        })
    };

    Ok(Json(result))
}

async fn get_gateway_health(State(state): State<Arc<AppState>>) -> Json<GatewayHealthView> {
    Json(crate::overview::build_gateway_health(&state).await)
}

const PG_HEALTH_TTL: std::time::Duration = std::time::Duration::from_secs(10);

async fn get_pg_health(State(state): State<Arc<AppState>>) -> Json<crate::types::PgHealth> {
    // Check TTL cache first.
    {
        let cache = state.pg_health_cache.read();
        if let Some((at, health)) = cache.as_ref() {
            if at.elapsed() < PG_HEALTH_TTL {
                return Json(health.clone());
            }
        }
    }

    // Clone PgStore out of the lock before any await points.
    let pg_clone = state.pg_store.read().clone();

    let health = match pg_clone {
        None => {
            if state.pg_pending_config.read().is_some() {
                crate::types::PgHealth {
                    configured: true,
                    connected: false,
                    pool_available: None,
                    pool_max: None,
                    error: Some("connection pending (retry in progress)".to_string()),
                }
            } else {
                crate::types::PgHealth {
                    configured: false,
                    connected: false,
                    pool_available: None,
                    pool_max: None,
                    error: None,
                }
            }
        }
        Some(pg) => {
            let pool = pg.pool();
            let status = pool.status();
            match pool.get().await {
                Ok(_client) => crate::types::PgHealth {
                    configured: true,
                    connected: true,
                    pool_available: Some(status.available),
                    pool_max: Some(status.max_size),
                    error: None,
                },
                Err(e) => crate::types::PgHealth {
                    configured: true,
                    connected: false,
                    pool_available: Some(status.available),
                    pool_max: Some(status.max_size),
                    error: Some(e.to_string()),
                },
            }
        }
    };

    // Update cache and return.
    *state.pg_health_cache.write() = Some((std::time::Instant::now(), health.clone()));
    Json(health)
}

async fn get_network_info(State(state): State<Arc<AppState>>) -> Json<NetworkInfo> {
    match state.gateway.get_client_endpoint().await {
        Ok(view) => {
            let snap = crab_client_endpoint::ClientEndpointSnapshot {
                gateway_url: view.gateway_url,
                gateway_url_lan: view.gateway_url_lan,
                gateway_url_public: view.gateway_url_public,
                public_source: view.public_source.and_then(|s| match s.as_str() {
                    "env" => Some(crab_client_endpoint::PublicUrlSource::Env),
                    "observed" => Some(crab_client_endpoint::PublicUrlSource::Observed),
                    "frp" => Some(crab_client_endpoint::PublicUrlSource::Frp),
                    "openresty" => Some(crab_client_endpoint::PublicUrlSource::Openresty),
                    _ => None,
                }),
            };
            Json(NetworkInfo::from_snapshot(snap))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Gateway client-endpoint API unavailable; using local FRP/OpenResty discovery"
            );
            Json(NetworkInfo::build(
                crate::network::NetworkInfoConfig::from_env(),
            ))
        }
    }
}

async fn get_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MetricsSnapshot>, StatusCode> {
    let body = state.fetch_gateway_metrics().await.map_err(|e| {
        tracing::warn!(error = %e, "Failed to fetch gateway metrics");
        StatusCode::SERVICE_UNAVAILABLE
    })?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let counters = crate::metrics_history::scrape_gateway_counters(&body, now);
    {
        let mut history = state.metrics_history.write();
        if history.sample_count() == 0 {
            history.append(counters);
        }
    }
    let probe = crate::overview::fetch_gateway_probe_cached(&state).await;
    crate::overview::build_metrics_snapshot(&body, &state, now, probe.status.as_ref())
        .await
        .map(Json)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

async fn get_overview(
    State(state): State<Arc<AppState>>,
) -> Result<Json<OverviewBundle>, StatusCode> {
    crate::overview::build_overview(&state)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to build overview");
            StatusCode::SERVICE_UNAVAILABLE
        })
}

#[derive(Debug, Deserialize)]
struct OverviewTimeseriesQuery {
    #[serde(default = "default_timeseries_window")]
    window: String,
}

fn default_timeseries_window() -> String {
    "1h".to_string()
}

async fn get_overview_core(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let (core, etag_val) = crate::overview::get_overview_core_cached(&state)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to build overview core");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    // If-None-Match → 304 when content unchanged.
    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && if_none_match == etag_val.as_str()
    {
        let mut resp = Response::new(axum::body::Body::empty());
        *resp.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(hv) = HeaderValue::from_str(&etag_val) {
            resp.headers_mut().insert(header::ETAG, hv);
        }
        return Ok(resp);
    }

    // 200 with ETag header for client-side caching.
    let mut resp = Json(core).into_response();
    if let Ok(hv) = HeaderValue::from_str(&etag_val) {
        resp.headers_mut().insert(header::ETAG, hv);
    }
    Ok(resp)
}

async fn get_overview_timeseries(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<OverviewTimeseriesQuery>,
) -> Result<Response, StatusCode> {
    let window = match query.window.as_str() {
        "1h" | "24h" | "7d" => query.window.as_str(),
        _ => "1h",
    };

    let (points, etag_val) = crate::overview::get_overview_timeseries_cached(&state, window)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to build overview timeseries");
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && if_none_match == etag_val.as_str()
    {
        let mut resp = Response::new(axum::body::Body::empty());
        *resp.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(hv) = HeaderValue::from_str(&etag_val) {
            resp.headers_mut().insert(header::ETAG, hv);
        }
        return Ok(resp);
    }

    let payload = OverviewTimeseriesResponse {
        window: window.to_string(),
        points,
    };
    let mut resp = Json(payload).into_response();
    if let Ok(hv) = HeaderValue::from_str(&etag_val) {
        resp.headers_mut().insert(header::ETAG, hv);
    }
    Ok(resp)
}

async fn get_overview_trace(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TraceSummary>, StatusCode> {
    let summary = crate::overview::build_overview_trace(&state).await;
    Ok(Json(summary))
}

async fn list_domains(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DomainMetricsBucket>>, StatusCode> {
    let body = state
        .fetch_gateway_metrics()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut buckets = domain_token_buckets(&body, 50);
    let history = state.metrics_history.read();
    for bucket in &mut buckets {
        bucket.qps_5m = history.domain_qps_5m(&bucket.domain, now);
    }
    Ok(Json(buckets))
}

async fn get_domain_detail(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
) -> Result<Json<DomainDetailBundle>, StatusCode> {
    let body = state
        .fetch_gateway_metrics()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut buckets = domain_token_buckets(&body, 50);
    let history = state.metrics_history.read();
    let bucket = buckets
        .iter_mut()
        .find(|b| b.domain == domain)
        .cloned()
        .unwrap_or_else(|| DomainMetricsBucket {
            domain: domain.clone(),
            hit_tokens: 0,
            miss_tokens: 0,
            hit_ratio: 0.0,
            cost_saved_usd: 0.0,
            qps_5m: history.domain_qps_5m(&domain, now),
            alert: None,
        });
    let policy = state
        .domain_policies
        .read()
        .iter()
        .find(|p| p.domain == domain)
        .cloned();
    Ok(Json(DomainDetailBundle {
        domain: domain.clone(),
        bucket,
        consumer_buckets: domain_consumer_buckets(&body, &domain),
        history_7d: history.domain_token_hit_rate_series(&domain, 7 * 24 * 3600, now),
        history_30d: history.domain_token_hit_rate_series(&domain, 30 * 24 * 3600, now),
        tier_deltas_5m: domain_tier_deltas_5m(&body, &domain),
        policy,
    }))
}

async fn get_domain_policies(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DomainPolicy>>, StatusCode> {
    Ok(Json(state.domain_policies.read().clone()))
}

async fn put_domain_policies(
    State(state): State<Arc<AppState>>,
    Json(policies): Json<Vec<DomainPolicy>>,
) -> Result<Json<Vec<DomainPolicy>>, StatusCode> {
    *state.domain_policies.write() = policies;
    state.flush_persist();
    state.sync_domain_policies_to_gateway().await;

    audit_log(
        &state,
        "put_domain_policies",
        None,
        Some(serde_json::json!({ "count": state.domain_policies.read().len() })),
    )
    .await;

    Ok(Json(state.domain_policies.read().clone()))
}

async fn get_prefix_cache_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PrefixCacheMetricsSnapshot>, StatusCode> {
    let body = state.fetch_gateway_metrics().await.map_err(|e| {
        tracing::warn!(error = %e, "Failed to fetch gateway metrics");
        StatusCode::SERVICE_UNAVAILABLE
    })?;
    Ok(Json(crate::metrics_history::build_prefix_cache_snapshot(
        &body,
    )))
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
            hit_rate: 0.0,
            ..Default::default()
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
            hit_rate: 0.0,
            ..Default::default()
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
            hit_rate: 0.0,
            ..Default::default()
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
            hit_rate: 0.0,
            ..Default::default()
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
            hit_rate: 0.0,
            ..Default::default()
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

        let hit_rate = if requests > 0 {
            cache_hits as f64 / requests as f64
        } else {
            0.0
        };
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%H:00").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
            hit_rate,
            ..Default::default()
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

        let hit_rate = if requests > 0 {
            cache_hits as f64 / requests as f64
        } else {
            0.0
        };
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%m-%d").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
            hit_rate,
            ..Default::default()
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

        let hit_rate = if requests > 0 {
            cache_hits as f64 / requests as f64
        } else {
            0.0
        };
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("W%U").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
            hit_rate,
            ..Default::default()
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

        let hit_rate = if requests > 0 {
            cache_hits as f64 / requests as f64
        } else {
            0.0
        };
        stats.push(TimeSeriesPoint {
            timestamp: timestamp.format("%Y-%m").to_string(),
            requests,
            tokens,
            cache_hits,
            avg_latency_ms,
            hit_rate,
            ..Default::default()
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
                domain: spec.domain,
                project_id: spec.project_id,
                pipeline: spec.pipeline,
                upstream_profile: spec.upstream_profile,
                rpm_limit: spec.rpm_limit,
                monthly_token_budget: meta.as_ref().map(|m| m.monthly_token_limit).unwrap_or(0),
                tokens_used_this_month: meta.as_ref().map(|m| m.tokens_this_month).unwrap_or(0),
                expired_at: meta.as_ref().and_then(|m| m.expired_at),
                model_limits: meta
                    .as_ref()
                    .map(|m| m.model_limits.clone())
                    .unwrap_or_default(),
                remain_quota: meta.as_ref().map(|m| m.remain_quota).unwrap_or(-1),
                unlimited_quota: meta.as_ref().map(|m| m.unlimited_quota).unwrap_or(true),
                max_concurrent: spec.max_concurrent,
                inflight: spec.inflight,
            }
        })
        .collect();

    Ok(Json(keys))
}

fn normalize_optional_project_id(
    project_id: &Option<String>,
) -> Result<Option<String>, (StatusCode, String)> {
    match project_id
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        None => Ok(None),
        Some(raw) => crab_proxy::sanitize_user_id(raw)
            .map(Some)
            .map_err(|e| (StatusCode::BAD_REQUEST, e)),
    }
}

async fn create_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<ApiKey>, StatusCode> {
    let project_id = match normalize_optional_project_id(&req.project_id) {
        Ok(v) => v,
        Err((code, _)) => return Err(code),
    };

    let created = state
        .gateway
        .create_key(&CreateGatewayKeyRequest {
            name: req.name.clone(),
            enabled: true,
            token: None,
            domain: req.domain.clone(),
            project_id: project_id.clone(),
            pipeline: req.pipeline.clone(),
            upstream_profile: req.upstream_profile.clone(),
            max_concurrent: req.max_concurrent,
            rpm_limit: Some(req.rpm_limit),
        })
        .await
        .map_err(|e| gateway_status_code(&e))?;

    let model_limits = req.model_limits.clone().unwrap_or_default();
    let remain_quota = req.remain_quota.unwrap_or(-1);
    let unlimited_quota = req.unlimited_quota.unwrap_or(true);

    let max_concurrent = req.max_concurrent.unwrap_or(created.max_concurrent);
    let meta = KeyMetadata {
        id: created.id.clone(),
        name: created.name.clone(),
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
        max_concurrent,
        usage_month: String::new(),
    };
    state.keys_meta.insert(created.id.clone(), meta);
    state.flush_persist();

    audit_log(
        &state,
        "create_key",
        Some(&created.id),
        Some(serde_json::json!({ "name": &created.name })),
    )
    .await;

    Ok(Json(ApiKey {
        id: created.id,
        name: created.name,
        key_preview: created.key_preview,
        key_full: Some(created.key_full),
        active: created.enabled,
        domain: created.domain,
        project_id: created.project_id,
        pipeline: created.pipeline,
        upstream_profile: created.upstream_profile,
        rpm_limit: req.rpm_limit,
        monthly_token_budget: req.monthly_token_budget,
        tokens_used_this_month: 0,
        expired_at: req.expired_at,
        model_limits,
        remain_quota,
        unlimited_quota,
        max_concurrent: created.max_concurrent,
        inflight: 0,
    }))
}

async fn revoke_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .gateway
        .revoke_key_by_id(&id)
        .await
        .map_err(|e| gateway_status_code(&e))?;

    state.keys_meta.remove(&id);
    state.flush_persist();

    audit_log(&state, "revoke_key", Some(&id), None).await;

    Ok(StatusCode::NO_CONTENT)
}

async fn patch_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchKeyRequest>,
) -> Result<Json<ApiKey>, (StatusCode, String)> {
    let updated = state
        .gateway
        .patch_key_by_id(
            &id,
            &crab_control::PatchGatewayKeyRequest {
                name: req.name.clone(),
                enabled: req.enabled,
                domain: req.domain.clone(),
                project_id: req.project_id.clone(),
                pipeline: req.pipeline.clone(),
                upstream_profile: req.upstream_profile.clone(),
                max_concurrent: req.max_concurrent,
                rpm_limit: req.rpm_limit,
            },
        )
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    if let Some(mut meta) = state.keys_meta.get_mut(&id) {
        if let Some(name) = req.name.as_ref() {
            meta.name = name.clone();
        }
        if req.enabled == Some(false) {
            meta.token = String::new();
        }
        meta.max_concurrent = updated.max_concurrent;
        meta.rpm_limit = updated.rpm_limit as u64;
        if let Some(monthly_token_budget) = req.monthly_token_budget {
            meta.monthly_token_limit = monthly_token_budget;
        }
        if let Some(remain_quota) = req.remain_quota {
            meta.remain_quota = remain_quota;
        }
        if let Some(unlimited_quota) = req.unlimited_quota {
            meta.unlimited_quota = unlimited_quota;
        }
    } else {
        state.keys_meta.insert(
            id.clone(),
            KeyMetadata {
                id: id.clone(),
                name: updated.name.clone(),
                token: String::new(),
                rpm_limit: updated.rpm_limit as u64,
                monthly_token_limit: req.monthly_token_budget.unwrap_or(0),
                current_rpm: 0,
                tokens_this_month: 0,
                input_tokens: 0,
                output_tokens: 0,
                expired_at: None,
                model_limits: Vec::new(),
                remain_quota: req.remain_quota.unwrap_or(-1),
                unlimited_quota: req.unlimited_quota.unwrap_or(true),
                max_concurrent: updated.max_concurrent,
                usage_month: String::new(),
            },
        );
    }
    state.flush_persist();

    let meta = state.keys_meta.get(&id);
    Ok(Json(ApiKey {
        id: updated.id.clone(),
        name: updated.name,
        key_preview: updated.key_preview,
        key_full: meta.as_ref().map(|m| m.token.clone()),
        active: updated.enabled,
        rpm_limit: updated.rpm_limit,
        monthly_token_budget: meta.as_ref().map(|m| m.monthly_token_limit).unwrap_or(0),
        tokens_used_this_month: meta.as_ref().map(|m| m.tokens_this_month).unwrap_or(0),
        expired_at: meta.as_ref().and_then(|m| m.expired_at),
        model_limits: meta
            .as_ref()
            .map(|m| m.model_limits.clone())
            .unwrap_or_default(),
        remain_quota: meta.as_ref().map(|m| m.remain_quota).unwrap_or(-1),
        unlimited_quota: meta.as_ref().map(|m| m.unlimited_quota).unwrap_or(true),
        domain: updated.domain,
        project_id: updated.project_id,
        pipeline: updated.pipeline,
        upstream_profile: updated.upstream_profile,
        max_concurrent: updated.max_concurrent,
        inflight: updated.inflight,
    }))
}

#[derive(serde::Deserialize)]
struct BatchRevokeBody {
    ids: Vec<String>,
}

async fn batch_revoke_keys(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchRevokeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut revoked = Vec::new();
    let mut errors = Vec::new();
    for id in &body.ids {
        match state.gateway.revoke_key_by_id(id).await {
            Ok(_) => {
                state.keys_meta.remove(id);
                revoked.push(id.clone());
            }
            Err(e) => {
                errors.push((id.clone(), e.to_string()));
            }
        }
    }
    state.flush_persist();
    Ok(Json(serde_json::json!({
        "revoked": revoked,
        "errors": errors,
    })))
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
            audit_log(
                &state,
                "cache_invalidate",
                None,
                Some(serde_json::json!({ "scope": &resp.scope })),
            )
            .await;
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

async fn get_pipeline_runtime(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PipelineRuntimeConfig>, (StatusCode, String)> {
    let cfg = state
        .gateway
        .get_pipeline_runtime()
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    Ok(Json(PipelineRuntimeConfig {
        pipeline_mode: cfg.pipeline_mode,
        default_upstream_profile: cfg.default_upstream_profile,
        profiles: cfg
            .profiles
            .into_iter()
            .map(|p| PipelineProfileView {
                id: p.id,
                provider: p.provider,
                base_url: p.base_url,
                fallback_model: p.fallback_model,
            })
            .collect(),
    }))
}

async fn put_pipeline_runtime(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PipelineRuntimeConfig>,
) -> Result<Json<PipelineRuntimeConfig>, (StatusCode, String)> {
    let cfg = state
        .gateway
        .put_pipeline_runtime(&crab_control::PipelineRuntimeConfigView {
            pipeline_mode: req.pipeline_mode,
            default_upstream_profile: req.default_upstream_profile,
            profiles: req
                .profiles
                .into_iter()
                .map(|p| crab_control::PipelineProfileView {
                    id: p.id,
                    provider: p.provider,
                    base_url: p.base_url,
                    fallback_model: p.fallback_model,
                })
                .collect(),
        })
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    Ok(Json(PipelineRuntimeConfig {
        pipeline_mode: cfg.pipeline_mode,
        default_upstream_profile: cfg.default_upstream_profile,
        profiles: cfg
            .profiles
            .into_iter()
            .map(|p| PipelineProfileView {
                id: p.id,
                provider: p.provider,
                base_url: p.base_url,
                fallback_model: p.fallback_model,
            })
            .collect(),
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
        model_overrides: config.model_overrides.clone(),
        consumer_overrides: config.consumer_overrides.clone(),
        consumer_model_overrides: config.consumer_model_overrides.clone(),
    }))
}

async fn update_cache_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateCacheConfigRequest>,
) -> Result<Json<CacheConfig>, StatusCode> {
    {
        let mut config = state.cache_config.write();
        config.l0_ttl_secs = req.l0_ttl_secs;
        config.l1_ttl_secs = req.l1_ttl_secs;
        config.model_overrides = req.model_overrides.clone();
        config.consumer_overrides = req.consumer_overrides.clone();
        config.consumer_model_overrides = req.consumer_model_overrides.clone();
    }
    let put_req = {
        let config = state.cache_config.read();
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
            consumer_model_overrides: config
                .consumer_model_overrides
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
            stale_while_revalidate_ttl_secs: 0,
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

    audit_log(
        &state,
        "update_cache_config",
        None,
        Some(serde_json::json!({ "default_ttl_secs": config.default_ttl_secs })),
    )
    .await;

    Ok(Json(CacheConfig {
        l0_ttl_secs: config.l0_ttl_secs,
        l1_ttl_secs: config.l1_ttl_secs,
        default_ttl_secs: config.default_ttl_secs,
        model_overrides: config.model_overrides.clone(),
        consumer_overrides: config.consumer_overrides.clone(),
        consumer_model_overrides: config.consumer_model_overrides.clone(),
    }))
}

async fn get_semantic_config(State(state): State<Arc<AppState>>) -> Json<SemanticConfig> {
    let config = state.semantic_config.read().clone();
    Json(SemanticConfig {
        enabled: config.enabled,
        similarity_threshold: config.similarity_threshold as f64,
    })
}

async fn update_semantic_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateSemanticConfigRequest>,
) -> Json<SemanticConfig> {
    let mut config = state.semantic_config.write();
    if let Some(enabled) = req.enabled {
        config.enabled = enabled;
    }
    config.similarity_threshold = req.similarity_threshold as f32;

    Json(SemanticConfig {
        enabled: config.enabled,
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
            addr: b.addr,
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

async fn put_routing_backends(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PutBackendsRequest>,
) -> Result<Json<RoutingStatus>, (StatusCode, String)> {
    let endpoints: Vec<String> = req.backends.iter().map(|b| b.addr.clone()).collect();
    let default_weight = req.backends.first().map(|b| b.weight).unwrap_or(1);
    let view = state
        .gateway
        .put_backends(&crab_control::PutBackendsRequest {
            endpoints,
            default_weight,
            tls_sni: "api.deepseek.com".to_string(),
        })
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    let backends: Vec<BackendStatus> = view
        .backends
        .into_iter()
        .map(|b| BackendStatus {
            name: b.name,
            request_count: 0,
            healthy: b.healthy,
            addr: b.addr,
        })
        .collect();

    Ok(Json(RoutingStatus {
        total_backends: backends.len(),
        active_backends: backends.len(),
        total_requests: 0,
        backends,
    }))
}

async fn get_cursor_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CursorModelsConfig>, (StatusCode, String)> {
    let view = state
        .gateway
        .get_cursor_models()
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    let aliases: Vec<CursorModelAlias> = view
        .aliases
        .into_iter()
        .map(|(model, alias)| CursorModelAlias {
            model: model.clone(),
            alias: alias.upstream.clone(),
        })
        .collect();

    Ok(Json(CursorModelsConfig { aliases }))
}

async fn put_cursor_models(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CursorModelsConfig>,
) -> Result<Json<CursorModelsConfig>, (StatusCode, String)> {
    let aliases: std::collections::HashMap<String, crab_control::CursorModelAliasView> = req
        .aliases
        .into_iter()
        .map(|a| {
            (
                a.model.clone(),
                crab_control::CursorModelAliasView {
                    upstream: a.alias.clone(),
                    pipeline: "cursor_deepseek_v4".to_string(),
                },
            )
        })
        .collect();

    let gateway_req = CursorModelsConfigView {
        force_deepseek_profile_for_aliases: false,
        synthetic_models_enabled: true,
        aliases,
    };

    let view = state
        .gateway
        .put_cursor_models(&gateway_req)
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;

    let aliases: Vec<CursorModelAlias> = view
        .aliases
        .into_iter()
        .map(|(model, alias)| CursorModelAlias {
            model: model.clone(),
            alias: alias.upstream.clone(),
        })
        .collect();

    Ok(Json(CursorModelsConfig { aliases }))
}

async fn get_logs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<crate::types::LogsQuery>,
) -> Json<crate::types::LogsPageResponse> {
    let limit = query.limit.unwrap_or(100).min(500);

    let memory_logs = state.request_logs.read().clone();
    let trace_path = crate::trace_log::trace_log_path();

    // If we have memory logs and no archive/filter query, use memory.
    let uses_trace_archive = query.cursor.is_some()
        || query.from_ms.is_some()
        || query.to_ms.is_some()
        || query.consumer.as_ref().is_some_and(|s| !s.is_empty())
        || query.model.as_ref().is_some_and(|s| !s.is_empty())
        || query.cache_tier.as_ref().is_some_and(|s| !s.is_empty())
        || query.request_hash.as_ref().is_some_and(|s| !s.is_empty())
        || query.latency_min.is_some()
        || query.latency_max.is_some()
        || query.token_min.is_some()
        || query.token_max.is_some();
    if !memory_logs.is_empty() && !uses_trace_archive {
        let items: Vec<RequestLog> = memory_logs
            .into_iter()
            .map(|log| RequestLog {
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
                input_tokens: Some(log.input_tokens),
                output_tokens: Some(log.output_tokens),
                ttft_ms: None,
                content_length: None,
                request_hash: None,
                project_id: None,
                upstream_user_id: None,
                user_id_audit: None,
                upstream_key_id: None,
            })
            .collect();
        return Json(crate::types::LogsPageResponse {
            items: items.into_iter().take(limit).collect(),
            next_cursor: None,
            has_more: false,
            total_in_window: 0,
        });
    }

    // ── PG primary path ─────────────────────────────────────────────
    // When PG is available, use it as the primary query source for all
    // filtered/cursor queries, with JSONL as the fallback.
    let pg_ref = state.pg_store.read().clone();
    if let Some(ref pg) = pg_ref {
        if let Some(result) = try_query_pg_logs(pg, &query, limit).await {
            return Json(result);
        }
    }

    // ── JSONL fallback path ─────────────────────────────────────────
    // Use archive-aware loading with pagination.
    let opts = crate::trace_log::TraceLoadOpts {
        from_ms: query.from_ms,
        to_ms: query.to_ms,
        consumer: query.consumer,
        model: query.model,
        cache_tier: query.cache_tier,
        request_hash: query.request_hash,
        latency_min: query.latency_min,
        latency_max: query.latency_max,
        token_min: query.token_min,
        token_max: query.token_max,
        limit: limit + 1, // fetch +1 to determine has_more
        cursor: query.cursor,
    };

    let entries = crate::trace_log::load_trace_with_opts(&trace_path, &opts);

    let has_more = entries.len() > limit;

    // Build cursor from raw trace entry data BEFORE consuming entries.
    let next_cursor = if has_more {
        let tail = &entries[limit - 1];
        Some(format!("{}:{}", tail.timestamp_ms, tail.request_hash))
    } else {
        None
    };

    let items: Vec<RequestLog> = entries
        .into_iter()
        .take(limit)
        .map(|e| crate::trace_log::trace_entry_to_request_log(&e))
        .collect();

    Json(crate::types::LogsPageResponse {
        items,
        next_cursor,
        has_more,
        total_in_window: 0,
    })
}

/// Try to query logs from PG. Returns `Some(LogsPageResponse)` on success, `None` to fall back to JSONL.
async fn try_query_pg_logs(
    pg: &crate::pg::PgStore,
    query: &crate::types::LogsQuery,
    limit: usize,
) -> Option<crate::types::LogsPageResponse> {
    let (rows, next_cursor) = pg
        .query_trace_logs_paginated(
            query.cursor.as_deref(),
            query.from_ms,
            query.to_ms,
            query.consumer.as_deref(),
            query.model.as_deref(),
            query.cache_tier.as_deref(),
            query.request_hash.as_deref(),
            query.latency_min,
            query.latency_max,
            query.token_min,
            query.token_max,
            limit + 1,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "PG query failed, falling back to JSONL");
            e
        })
        .ok()?;

    let has_more = rows.len() > limit;
    let items: Vec<RequestLog> = rows
        .into_iter()
        .take(limit)
        .map(|e| crate::trace_log::trace_entry_to_request_log(&e))
        .collect();

    Some(crate::types::LogsPageResponse {
        items,
        next_cursor,
        has_more,
        total_in_window: 0,
    })
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
            upstream_latency_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            request_hash: None,
            semantic_cluster: None,
            upstream_key_id: None,
            affinity_kind: None,
            backend_name: None,
            session_fingerprint: None,
            is_coalesced: false,
            client_key_id: None,
            pipeline: None,
            upstream_model: None,
            request_passthrough: false,
            request_passthrough_prefix_len: None,
        }));
    }

    let trace_path = crate::trace_log::trace_log_path();
    if let Some(entry) = state.find_trace_entry(&id, &trace_path).await {
        return Ok(Json(crate::trace_log::trace_entry_to_request_detail(&entry)));
    }

    Err(StatusCode::NOT_FOUND)
}

// ── Log Management Handlers ──────────────────────────────────────────

async fn get_log_disk_usage(
    State(_state): State<Arc<AppState>>,
) -> Json<crab_admin_types::LogDiskUsage> {
    Json(crate::log_management::compute_disk_usage())
}

async fn post_clear_logs(
    State(state): State<Arc<AppState>>,
    Json(req): Json<crab_admin_types::ClearLogsRequest>,
) -> Result<Json<crab_admin_types::ClearLogsResponse>, (StatusCode, String)> {
    use crab_admin_types::ClearTarget;

    // 1. Clear rotated JSONL files on disk (existing behavior).
    let mut result = crate::log_management::clear_logs(&req)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // 2. Clear in-memory request logs for All and Capture targets.
    if matches!(req.target, ClearTarget::All | ClearTarget::Capture) {
        state.request_logs.write().clear();
    }

    // 3. Truncate active JSONL trace files.
    let truncate_trace = matches!(req.target, ClearTarget::All | ClearTarget::TraceRotated);
    let truncate_debug = matches!(req.target, ClearTarget::All | ClearTarget::DebugRotated);
    if truncate_trace || truncate_debug {
        for (path, size) in
            crate::log_management::truncate_active_logs(truncate_trace, truncate_debug)
        {
            result.deleted_files.push(path);
            result.freed_bytes += size;
        }
    }

    // 4. Clear trace summary/analysis caches.
    if truncate_trace || truncate_debug {
        state.trace_entries.write().clear();
        *state.trace_summary_cache.write() = None;
        *state.trace_analysis_cache.write() = None;
    }

    // 5. Clear PG trace_logs / request_logs when configured.
    {
        let pg = state.pg_store.read().clone();
        if let Some(ref pg) = pg {
            const DELETE_ALL: u64 = u64::MAX;
            if matches!(req.target, ClearTarget::All | ClearTarget::TraceRotated) {
                if let Ok(count) = pg.prune_trace_logs(DELETE_ALL).await {
                    if count > 0 {
                        tracing::info!(deleted = count, "PG trace_logs cleared");
                    }
                }
            }
            if matches!(req.target, ClearTarget::All | ClearTarget::Capture) {
                if let Ok(count) = pg.prune_request_logs(DELETE_ALL).await {
                    if count > 0 {
                        tracing::info!(deleted = count, "PG request_logs cleared");
                    }
                }
            }
        }
    }

    audit_log(
        &state,
        "clear_logs",
        None,
        Some(serde_json::json!({ "target": format!("{:?}", req.target), "freed_bytes": result.freed_bytes })),
    )
    .await;

    Ok(Json(result))
}

async fn get_retention_policy(
    State(state): State<Arc<AppState>>,
) -> Json<crab_admin_types::RetentionPolicy> {
    Json(state.log_retention.read().clone())
}

async fn put_retention_policy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<crab_admin_types::RetentionPolicy>,
) -> Result<Json<crab_admin_types::RetentionPolicy>, (StatusCode, String)> {
    if req.max_age_hours > 0 && req.max_age_hours < 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "max_age_hours must be >= 1 or 0 (disabled)".to_string(),
        ));
    }
    if req.max_disk_mb > 0 && req.max_disk_mb < 10 {
        return Err((
            StatusCode::BAD_REQUEST,
            "max_disk_mb must be >= 10 or 0 (disabled)".to_string(),
        ));
    }
    *state.log_retention.write() = req.clone();
    tracing::info!(
        max_age_hours = req.max_age_hours,
        max_disk_mb = req.max_disk_mb,
        max_trace_files = req.max_trace_files,
        max_capture_body_files = req.max_capture_body_files,
        "Log retention policy updated"
    );
    Ok(Json(req))
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

async fn get_models(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<crate::types::ModelsQuery>,
) -> Json<ModelListResponse> {
    let stored = state.models.read();
    let profile_filter = query.profile_id.as_deref();
    let models: Vec<ModelInfo> = stored
        .models
        .iter()
        .filter(|m| profile_filter.is_none_or(|p| m.profile_id == p))
        .map(|m| ModelInfo {
            profile_id: m.profile_id.clone(),
            id: m.id.clone(),
            owned_by: m.owned_by.clone(),
            context_length: m.context_length,
            input_price_per_mtok: m.input_price_per_mtok,
            output_price_per_mtok: m.output_price_per_mtok,
            available: m.available,
        })
        .collect();
    let total = models.len();
    let synced_at = profile_filter
        .and_then(|p| stored.synced_at_by_profile.get(p).cloned())
        .or_else(|| {
            if profile_filter.is_none() {
                stored.synced_at_by_profile.values().next().cloned()
            } else {
                None
            }
        });
    Json(ModelListResponse {
        models,
        total,
        profile_id: query.profile_id,
        synced_at,
    })
}

async fn sync_models(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<crate::types::ModelSyncQuery>,
) -> Result<Json<SyncResult>, (StatusCode, String)> {
    crate::upstream::sync_models_internal(&state, &query.profile_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn post_models_detect(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<crate::types::ModelSyncQuery>,
) -> Result<Json<crate::types::ModelDetectResponse>, (StatusCode, String)> {
    crate::upstream::detect_models_internal(&state, &query.profile_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn post_models_apply(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::types::ModelApplyBody>,
) -> Result<Json<SyncResult>, StatusCode> {
    state.refresh_profile_providers().await;
    let result =
        crate::upstream::apply_models_internal(&state, &body.profile_id, body.add, body.remove);
    Ok(Json(result))
}

async fn put_upstream_profile(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<crate::types::PutUpstreamProfileAdminRequest>,
) -> Result<Json<crate::types::UpstreamProfileAdminView>, (StatusCode, String)> {
    crate::upstream_profiles::put_profile(&state, &id, body)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn delete_upstream_profile(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    crate::upstream_profiles::delete_profile(&state, &id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn get_upstream_profile_keys(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<crate::types::UpstreamProfileKeysAdminView>, (StatusCode, String)> {
    crate::upstream_profiles::get_profile_keys(&state, &id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn put_upstream_profile_keys(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<PutUpstreamKeysRequest>,
) -> Result<Json<crate::types::UpstreamProfileKeysAdminView>, (StatusCode, String)> {
    let keys = body.keys.clone();
    let replace = matches!(body.mode, UpstreamKeysPutMode::Replace);
    crate::upstream_profiles::put_profile_keys(&state, &id, keys, replace)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn patch_upstream_profile_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((id, key_id)): axum::extract::Path<(String, String)>,
    Json(req): Json<PatchUpstreamKeyRequest>,
) -> Result<Json<crate::types::UpstreamKeyPoolEntry>, (StatusCode, String)> {
    crate::upstream_profiles::patch_profile_key(&state, &id, &key_id, req)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

async fn post_upstream_profile_test(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<UpstreamTestResult>, (StatusCode, String)> {
    let result = crate::upstream_profiles::test_profile(&state, &id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(result))
}

async fn post_upstream_profile_key_test(
    State(state): State<Arc<AppState>>,
    axum::extract::Path((id, key_id)): axum::extract::Path<(String, String)>,
) -> Result<Json<UpstreamTestResult>, (StatusCode, String)> {
    let result = crate::upstream_profiles::test_profile_key(&state, &id, &key_id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(result))
}

async fn get_upstream_profile_routing(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<crab_control::ProfileRoutingView>, (StatusCode, String)> {
    let result = state
        .gateway
        .get_profile_routing(&id)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(result))
}

async fn post_upstream_test(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::types::UpstreamTestBody>,
) -> Json<UpstreamTestResult> {
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

async fn get_reasoning_config(State(state): State<Arc<AppState>>) -> Json<ReasoningConfig> {
    Json(state.reasoning_config.read().clone())
}

async fn put_reasoning_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReasoningConfig>,
) -> Json<ReasoningConfig> {
    *state.reasoning_config.write() = req;
    state.flush_persist();
    Json(state.reasoning_config.read().clone())
}

async fn get_upstream_keys_pool(
    State(state): State<Arc<AppState>>,
) -> Result<Json<UpstreamKeysView>, (StatusCode, String)> {
    state
        .gateway
        .get_upstream_keys()
        .await
        .map(crate::types::upstream_keys_view_from_control)
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

    let ctrl_keys: Vec<crab_control::UpstreamKeyInput> = req
        .keys
        .iter()
        .map(crate::types::upstream_key_input_to_control)
        .collect();

    if req.mode == UpstreamKeysPutMode::Replace {
        state.replace_upstream_pool_secrets(&ctrl_keys);
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
                account_id: String::new(),
            })
            .collect();
        let append_ctrl: Vec<crab_control::UpstreamKeyInput> = inputs
            .iter()
            .map(crate::types::upstream_key_input_to_control)
            .collect();
        state.replace_upstream_pool_secrets(&append_ctrl);
    }

    let view = state
        .gateway
        .put_upstream_keys(&crate::types::put_upstream_keys_to_control(&req))
        .await
        .map_err(|e| (gateway_status_code(&e), gateway_error_message(&e)))?;
    state.flush_persist();
    Ok(Json(crate::types::upstream_keys_view_from_control(view)))
}

async fn patch_upstream_key_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PatchUpstreamKeyRequest>,
) -> Result<Json<UpstreamKeyView>, (StatusCode, String)> {
    state
        .gateway
        .patch_upstream_key(&id, &crate::types::patch_upstream_key_to_control(&req))
        .await
        .map(crate::types::upstream_key_view_from_control)
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
    state.reconcile_upstream_if_stale(false).await;
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
        if let Some(key) = &req.api_key
            && !key.is_empty()
            && !key.contains("****")
        {
            if let Err(e) = validate_deepseek_key(key) {
                return Err((StatusCode::BAD_REQUEST, e));
            }
            config.api_key = key.clone();
        }
    }

    let mut keys_to_push: Vec<String> = req.keys_to_append.clone();
    if let Some(key) = &req.api_key
        && !key.is_empty()
        && !key.contains("****")
    {
        keys_to_push.push(key.clone());
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
                account_id: String::new(),
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
                account_id: String::new(),
            })
            .collect();
        let merged_ctrl: Vec<crab_control::UpstreamKeyInput> = merged_inputs
            .iter()
            .map(crate::types::upstream_key_input_to_control)
            .collect();
        state.replace_upstream_pool_secrets(&merged_ctrl);
        let _ = state
            .gateway
            .put_upstream_keys(&crate::types::put_upstream_keys_to_control(&put_req))
            .await;
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

    let default_profile = state.default_profile_id();
    let sync = if state.pick_sync_api_key(&default_profile).is_some() {
        match crate::upstream::sync_models_internal(&state, &default_profile).await {
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

#[derive(Debug, serde::Deserialize)]
struct TraceAnalysisQuery {
    #[serde(default = "default_trace_hours")]
    hours: u32,
}

fn default_trace_hours() -> u32 {
    24
}

fn empty_trace_analysis() -> TraceAnalysis {
    TraceAnalysis {
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
        deepseek_user_id: None,
        zipf_log_points: vec![],
        zipf_regression_slope: 0.0,
        zipf_regression_intercept: 0.0,
    }
}

#[derive(Debug, Deserialize)]
pub struct ConsumersQuery {
    #[serde(default = "default_live_window_secs")]
    pub window_secs: u32,
}

/// GET /api/admin/live-metrics/consumers?window_secs=300
async fn get_live_consumers(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ConsumersQuery>,
) -> Json<serde_json::Value> {
    let path = crate::trace_log::trace_log_path();
    let pg = state.pg_store.read().clone();
    let window_secs = query.window_secs.clamp(60, 30 * 24 * 3600);

    let _ = crate::trace_log::load_live_trace_entries_auto(&state, window_secs).await;

    let trace_available = crate::trace_log::trace_source_available(&path, pg.as_ref());

    let consumer_names = crate::trace_log::live_distinct_consumers(&state.live_trace_cache);
    Json(serde_json::json!({
        "trace_available": trace_available,
        "available_consumers": consumer_names,
    }))
}

// ── P1: Per-key concurrency ─────────────────────────────────────────

#[derive(serde::Deserialize)]
struct KeyWindowQuery {
    #[serde(default = "default_key_window")]
    window_secs: u32,
}

fn default_key_window() -> u32 {
    300
}

async fn get_key_concurrency(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(key_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<KeyWindowQuery>,
) -> Result<Json<KeyConcurrencyResponse>, StatusCode> {
    let window_secs = query.window_secs;
    let entries =
        crate::trace_log::load_live_trace_entries_auto(&state, window_secs).await;

    let filtered: Vec<KeyConcurrencyEntry> = entries
        .iter()
        .filter(|e| {
            e.upstream_key_id.as_deref() == Some(&key_id)
                || e.client_key_id.as_deref() == Some(&key_id)
                || e.consumer.as_deref() == Some(&key_id)
        })
        .map(|e| KeyConcurrencyEntry {
            request_hash: e.request_hash.clone(),
            timestamp_ms: e.timestamp_ms,
            model: e.model.clone(),
            consumer: e.consumer.clone(),
            affinity_key: e.affinity_key.clone(),
            affinity_kind: e.affinity_kind.clone(),
            backend_name: e.backend_name.clone(),
            session_fingerprint: e.session_fingerprint.clone(),
            is_coalesced: e.is_coalesced,
            latency_ms: e.latency_ms,
            cache_hit: e.cache_hit,
            cache_tier: e.cache_tier.clone(),
        })
        .collect();

    let total = filtered.len();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let active_threshold_ms = 30_000;
    let active_now = filtered
        .iter()
        .filter(|e| now_ms.saturating_sub(e.timestamp_ms) < active_threshold_ms)
        .count();

    // Compute peak concurrency: max overlapping requests within any 1-second window.
    let concurrent_peak = compute_peak_concurrency(&filtered);

    Ok(Json(KeyConcurrencyResponse {
        key_id,
        window_secs,
        total_requests: total,
        concurrent_peak,
        active_now,
        entries: filtered,
    }))
}

fn compute_peak_concurrency(entries: &[KeyConcurrencyEntry]) -> u32 {
    if entries.is_empty() {
        return 0;
    }
    let mut events: Vec<(u64, i32)> = Vec::with_capacity(entries.len() * 2);
    for e in entries {
        let duration_ms = (e.latency_ms.max(1.0)) as u64;
        let end_ms = e.timestamp_ms.saturating_add(duration_ms);
        events.push((e.timestamp_ms, 1));
        events.push((end_ms, -1));
    }
    events.sort_by_key(|(ts, _)| *ts);
    let mut current: u32 = 0;
    let mut peak: u32 = 0;
    for (_, delta) in events {
        if delta > 0 {
            current += 1;
            peak = peak.max(current);
        } else {
            current = current.saturating_sub(1);
        }
    }
    peak
}

// ── P1: Per-key routing distribution ────────────────────────────────

async fn get_key_routing(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(key_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<KeyWindowQuery>,
) -> Result<Json<KeyRoutingResponse>, StatusCode> {
    let window_secs = query.window_secs;
    let entries =
        crate::trace_log::load_live_trace_entries_auto(&state, window_secs).await;

    let key_entries: Vec<&crate::trace_log::TraceLogEntry> = entries
        .iter()
        .filter(|e| {
            e.upstream_key_id.as_deref() == Some(&key_id)
                || e.client_key_id.as_deref() == Some(&key_id)
                || e.consumer.as_deref() == Some(&key_id)
        })
        .collect();

    // Backend distribution by affinity kind.
    let mut backend_map: HashMap<(String, Option<String>), (u64, f64, u32)> = HashMap::new();
    for e in &key_entries {
        let name = e.backend_name.clone().unwrap_or_else(|| "unknown".into());
        let affinity_kind = e.affinity_kind.clone();
        let slot = backend_map
            .entry((name, affinity_kind))
            .or_insert((0, 0.0, 0));
        slot.0 += 1;
        slot.1 += e.latency_ms;
        if e.cache_hit {
            slot.2 += 1;
        }
    }
    let backends: Vec<KeyRoutingBackend> = backend_map
        .into_iter()
        .map(
            |((name, affinity_kind), (count, lat_sum, hits))| KeyRoutingBackend {
                backend_name: name,
                request_count: count,
                affinity_kind,
                avg_latency_ms: if count > 0 {
                    lat_sum / count as f64
                } else {
                    0.0
                },
                cache_hit_rate: if count > 0 {
                    hits as f64 / count as f64
                } else {
                    0.0
                },
            },
        )
        .collect();

    // Detect affinity migrations: same session_fingerprint switching backends.
    let mut migrations: Vec<AffinityMigration> = Vec::new();
    let mut session_backend: HashMap<String, (String, u64)> = HashMap::new();
    let mut sorted = key_entries.clone();
    sorted.sort_by_key(|e| e.timestamp_ms);
    for e in &sorted {
        if let (Some(fp), Some(backend)) =
            (e.session_fingerprint.as_deref(), e.backend_name.as_deref())
        {
            if let Some((prev_backend, _)) = session_backend.get(fp) {
                if prev_backend != backend {
                    migrations.push(AffinityMigration {
                        session_fingerprint: fp.to_string(),
                        from_backend: prev_backend.clone(),
                        to_backend: backend.to_string(),
                        timestamp_ms: e.timestamp_ms,
                    });
                }
            }
            session_backend.insert(fp.to_string(), (backend.to_string(), e.timestamp_ms));
        }
    }

    // Count prefix breaks: requests where affinity_kind changed from a prefix-based source.
    let mut prefix_break_count: u64 = 0;
    let mut session_affinity: HashMap<String, &str> = HashMap::new();
    for e in &sorted {
        if let (Some(fp), Some(kind)) =
            (e.session_fingerprint.as_deref(), e.affinity_kind.as_deref())
        {
            if let Some(prev_kind) = session_affinity.get(fp) {
                if *prev_kind != kind && (*prev_kind == "conv" || *prev_kind == "pck") {
                    prefix_break_count += 1;
                }
            }
            session_affinity.insert(fp.to_string(), kind);
        }
    }

    Ok(Json(KeyRoutingResponse {
        key_id,
        window_secs,
        backends,
        migrations,
        prefix_break_count,
    }))
}

// ── P1: Session timeline ────────────────────────────────────────────

async fn get_session_timeline(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(fingerprint): axum::extract::Path<String>,
) -> Result<Json<SessionTimelineResponse>, StatusCode> {
    let window_secs: u32 = 900;
    let entries =
        crate::trace_log::load_live_trace_entries_auto(&state, window_secs).await;

    let mut session_entries: Vec<&crate::trace_log::TraceLogEntry> = entries
        .iter()
        .filter(|e| e.session_fingerprint.as_deref() == Some(&fingerprint))
        .collect();
    session_entries.sort_by_key(|e| e.timestamp_ms);

    let unique_keys: Vec<String> = session_entries
        .iter()
        .filter_map(|e| {
            e.upstream_key_id
                .as_deref()
                .or(e.client_key_id.as_deref())
                .map(|s| s.to_string())
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let events: Vec<SessionEvent> = session_entries
        .iter()
        .map(|e| SessionEvent {
            timestamp_ms: e.timestamp_ms,
            request_hash: e.request_hash.clone(),
            model: e.model.clone(),
            consumer: e.consumer.clone(),
            affinity_key: e.affinity_key.clone(),
            backend_name: e.backend_name.clone(),
            is_coalesced: e.is_coalesced,
            cache_hit: e.cache_hit,
            cache_tier: e.cache_tier.clone(),
            latency_ms: e.latency_ms,
            input_tokens: e.input_tokens,
            output_tokens: e.output_tokens,
        })
        .collect();

    let total = events.len();
    Ok(Json(SessionTimelineResponse {
        session_fingerprint: fingerprint,
        window_secs,
        total_events: total,
        unique_keys,
        events,
    }))
}

async fn get_live_metrics(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LiveMetricsQuery>,
) -> Result<Json<LiveMetricsResponse>, StatusCode> {
    let consumer = query.consumer.trim();
    if consumer.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = crate::trace_log::trace_log_path();
    let pg = state.pg_store.read().clone();
    let (window_secs, bucket_secs) =
        crate::live_metrics::clamp_live_params(query.window_secs, query.bucket_secs);

    let consumer = consumer.to_string();
    let key_id = query.key_id.clone();
    let session_fingerprint = query.session_fingerprint.clone();
    let group_by = query.group_by.clone();
    let entries =
        crate::trace_log::load_live_trace_entries_auto(&state, window_secs).await;
    let trace_available = crate::trace_log::trace_source_available(&path, pg.as_ref());
    // Use the cache's consumer HashSet instead of scanning the full entries list.
    let available_consumers = crate::trace_log::live_distinct_consumers(&state.live_trace_cache);
    let resp = crate::live_metrics::aggregate_live_metrics(
        entries.as_ref(),
        &consumer,
        &key_id,
        &session_fingerprint,
        window_secs,
        bucket_secs,
        trace_available,
        available_consumers,
        &group_by,
    );
    Ok(Json(resp))
}

async fn get_trace_analysis(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<TraceAnalysisQuery>,
) -> Result<Response, StatusCode> {
    use std::collections::HashMap;
    use std::time::Duration;

    // Check cache (60s TTL) — return (etag, analysis) tuple
    {
        let cached = state.trace_analysis_cache.read();
        if let Some((instant, ref analysis)) = *cached {
            if instant.elapsed() < Duration::from_secs(60) {
                let etag_val = format!("\"ta-{}-{}\"", analysis.total_requests, query.hours);
                if let Some(if_none_match) = headers
                    .get(header::IF_NONE_MATCH)
                    .and_then(|v| v.to_str().ok())
                    && if_none_match == etag_val.as_str()
                {
                    let mut resp = Response::new(axum::body::Body::empty());
                    *resp.status_mut() = StatusCode::NOT_MODIFIED;
                    if let Ok(hv) = HeaderValue::from_str(&etag_val) {
                        resp.headers_mut().insert(header::ETAG, hv);
                    }
                    return Ok(resp);
                }
                let mut resp = Json(analysis.clone()).into_response();
                if let Ok(hv) = HeaderValue::from_str(&etag_val) {
                    resp.headers_mut().insert(header::ETAG, hv);
                }
                return Ok(resp);
            }
        }
    }

    let trace_path = crate::trace_log::trace_log_path();
    let entries = state.load_trace_entries(&trace_path, query.hours).await;

    if entries.is_empty() {
        return Ok(Json(empty_trace_analysis()).into_response());
    }

    let summary = crate::trace_summary::compute_trace_summary(&entries, query.hours);
    let total_requests = summary.total_requests;
    let mut hash_counts: HashMap<String, usize> = HashMap::new();
    let mut cluster_counts: HashMap<u32, usize> = HashMap::new();
    let mut model_counts: HashMap<String, usize> = HashMap::new();
    let mut total_latency = 0.0;
    let mut total_prompt_tokens = 0;
    for entry in &entries {
        *hash_counts.entry(entry.request_hash.clone()).or_insert(0) += 1;
        *cluster_counts.entry(entry.semantic_cluster).or_insert(0) += 1;
        *model_counts.entry(entry.model.clone()).or_insert(0) += 1;
        total_latency += entry.latency_ms;
        total_prompt_tokens += entry.prompt_tokens;
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
    let (estimated_zipf_alpha, zipf_log_points, zipf_regression_slope, zipf_regression_intercept) =
        if freqs.len() >= 2 {
            let alpha = compute_zipf_alpha(&freqs);
            let (points, slope, intercept) = compute_zipf_regression_data(&freqs);
            (alpha, points, slope, intercept)
        } else {
            (0.0, Vec::new(), 0.0, 0.0)
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

    let cache_hit_ratio = summary.cache_hit_ratio;

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
            cluster_id: cluster_id as usize,
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

    let deepseek_user_id = Some(crate::trace_user_id_audit::compute_deepseek_user_id_audit(
        &entries,
    ));

    let analysis = TraceAnalysis {
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
        deepseek_user_id,
        zipf_log_points,
        zipf_regression_slope,
        zipf_regression_intercept,
    };

    // Store in cache
    *state.trace_analysis_cache.write() = Some((std::time::Instant::now(), analysis.clone()));

    let etag_val = format!("\"ta-{}-{}\"", analysis.total_requests, query.hours);
    if let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && if_none_match == etag_val.as_str()
    {
        let mut resp = Response::new(axum::body::Body::empty());
        *resp.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(hv) = HeaderValue::from_str(&etag_val) {
            resp.headers_mut().insert(header::ETAG, hv);
        }
        return Ok(resp);
    }
    let mut resp = Json(analysis).into_response();
    if let Ok(hv) = HeaderValue::from_str(&etag_val) {
        resp.headers_mut().insert(header::ETAG, hv);
    }
    Ok(resp)
}

fn compute_zipf_alpha(freqs: &[usize]) -> f64 {
    if freqs.len() < 2 {
        return 0.0;
    }

    let mut n_valid = 0usize;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;

    for (rank, &freq) in freqs.iter().enumerate() {
        if freq == 0 {
            continue;
        }
        n_valid += 1;
        let r = (rank + 1) as f64;
        let f = freq as f64;
        let log_r = r.ln();
        let log_f = f.ln();

        sum_x += log_r;
        sum_y += log_f;
        sum_xy += log_r * log_f;
        sum_xx += log_r * log_r;
    }

    if n_valid < 2 {
        return 0.0;
    }

    let denominator = n_valid as f64 * sum_xx - sum_x * sum_x;
    if denominator == 0.0 {
        return 0.0;
    }

    let alpha = (n_valid as f64 * sum_xy - sum_x * sum_y) / denominator;
    -alpha
}

/// Compute log-log data points and OLS regression line for Zipf visualization.
/// Returns (points, slope, intercept) where slope ≈ -alpha.
fn compute_zipf_regression_data(freqs: &[usize]) -> (Vec<crate::types::ZipfLogPoint>, f64, f64) {
    use crate::types::ZipfLogPoint;

    let mut points = Vec::new();
    let mut n_valid = 0usize;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_xx = 0.0;

    for (rank, &freq) in freqs.iter().enumerate() {
        if freq == 0 {
            continue;
        }
        n_valid += 1;
        let log_r = (rank + 1) as f64;
        let log_f = freq as f64;
        let lr = log_r.ln();
        let lf = log_f.ln();
        points.push(ZipfLogPoint {
            log_rank: lr,
            log_freq: lf,
        });
        sum_x += lr;
        sum_y += lf;
        sum_xy += lr * lf;
        sum_xx += lr * lr;
    }

    if n_valid < 2 {
        return (points, 0.0, 0.0);
    }

    let denominator = n_valid as f64 * sum_xx - sum_x * sum_x;
    if denominator == 0.0 {
        return (points, 0.0, 0.0);
    }

    let slope = (n_valid as f64 * sum_xy - sum_x * sum_y) / denominator;
    let intercept = (sum_y - slope * sum_x) / n_valid as f64;
    (points, slope, intercept)
}

// ── Infra handlers ──────────────────────────────────────────────

async fn get_infra_snapshot(
    State(state): State<Arc<AppState>>,
) -> Json<crate::infra::types::InfraSnapshot> {
    if let Some(cached) = state.infra_cache.get_latest() {
        return Json(cached);
    }
    Json(crate::infra::collector::empty_snapshot(
        state.infra_docker.is_some(),
        Some("collector initializing".into()),
    ))
}

async fn get_infra_status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::infra::types::InfraStatus> {
    Json(crate::infra::types::InfraStatus {
        docker_connected: state.infra_docker.is_some(),
        compose_project: crate::infra::resolve_compose_project(),
        poll_hint_secs: 10,
        history_sample_count: state.infra_history.read().sample_count(),
        last_collected_at: state.infra_cache.latest_collected_at(),
    })
}

#[derive(Debug, Deserialize)]
struct InfraTimeseriesQuery {
    #[serde(default = "default_infra_timeseries_window")]
    window: String,
    #[serde(default)]
    container_id: String,
}

fn default_infra_timeseries_window() -> String {
    "1h".to_string()
}

fn infra_window_secs(window: &str) -> u64 {
    match window {
        "24h" => 24 * 3600,
        _ => 3600,
    }
}

async fn get_infra_timeseries(
    State(state): State<Arc<AppState>>,
    Query(q): Query<InfraTimeseriesQuery>,
) -> Json<crate::infra::types::InfraTimeseriesResponse> {
    let window_secs = infra_window_secs(&q.window);
    let points: Vec<_> = state
        .infra_history
        .read()
        .points_since(window_secs)
        .into_iter()
        .cloned()
        .collect();

    let container_id = if q.container_id.is_empty() {
        state
            .infra_cache
            .get_latest()
            .and_then(|s| s.containers.first().map(|c| c.container_id.clone()))
            .unwrap_or_default()
    } else {
        q.container_id.clone()
    };

    let (cpu, mem, rx, tx) = crate::infra::history::to_timeseries(&points, &container_id);

    let map_pts = |pts: Vec<crate::infra::history::ChartPoint>| {
        pts.into_iter()
            .map(|p| crate::infra::types::InfraChartPoint {
                timestamp: p.timestamp,
                value: p.value,
            })
            .collect()
    };

    Json(crate::infra::types::InfraTimeseriesResponse {
        window: q.window,
        container_id,
        cpu: map_pts(cpu),
        memory: map_pts(mem),
        net_rx: map_pts(rx),
        net_tx: map_pts(tx),
        sample_count: points.len(),
    })
}

async fn post_infra_speed_test(
    State(state): State<Arc<AppState>>,
    Json(body): Json<crate::infra::types::SpeedTestRequest>,
) -> Result<(StatusCode, Json<crate::infra::types::SpeedTestAccepted>), StatusCode> {
    let direction = match body.direction.to_lowercase().as_str() {
        "download" => crate::infra::speed_test::SpeedTestDirection::Download,
        "upload" => crate::infra::speed_test::SpeedTestDirection::Upload,
        "both" => crate::infra::speed_test::SpeedTestDirection::Both,
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let (job_id, upload_token) = state
        .infra_speed_jobs
        .try_start(direction)
        .map_err(|_| StatusCode::CONFLICT)?;

    let jobs = Arc::clone(&state.infra_speed_jobs);
    match direction {
        crate::infra::speed_test::SpeedTestDirection::Download => {
            let jid = job_id.clone();
            tokio::spawn(async move {
                crate::infra::speed_test::run_download_test(jid, jobs).await;
            });
        }
        crate::infra::speed_test::SpeedTestDirection::Both => {
            let jid = job_id.clone();
            tokio::spawn(async move {
                crate::infra::speed_test::run_both_test(jid, jobs).await;
            });
        }
        crate::infra::speed_test::SpeedTestDirection::Upload => {
            state.infra_speed_jobs.set_running(&job_id);
        }
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(crate::infra::types::SpeedTestAccepted {
            job_id,
            upload_token,
        }),
    ))
}

async fn get_infra_speed_test_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<crate::infra::speed_test::SpeedTestJobView>, StatusCode> {
    state
        .infra_speed_jobs
        .get(&job_id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Debug, Deserialize)]
struct SpeedTestUploadQuery {
    job_id: String,
    token: String,
}

async fn post_infra_speed_test_upload(
    State(state): State<Arc<AppState>>,
    Query(q): Query<SpeedTestUploadQuery>,
    request: Request,
) -> Result<StatusCode, StatusCode> {
    if !state
        .infra_speed_jobs
        .validate_upload_token(&q.job_id, &q.token)
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let max_bytes = crate::infra::speed_test::max_upload_bytes();
    let start = std::time::Instant::now();
    let body = request.into_body();
    let bytes = axum::body::to_bytes(body, max_bytes)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let elapsed = start.elapsed().as_secs_f64();
    state
        .infra_speed_jobs
        .try_record_upload(&q.job_id, bytes.len() as u64, elapsed)
        .map_err(|code| match code {
            "job_expired" => StatusCode::GONE,
            "too_many_upload_attempts" => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::BAD_REQUEST,
        })?;
    Ok(StatusCode::OK)
}

// ---------------------------------------------------------------------------
// Audit Log
// ---------------------------------------------------------------------------

/// Helper: insert an audit log entry if PG is available. Fire-and-forget.
async fn audit_log(
    state: &AppState,
    action: &str,
    target: Option<&str>,
    detail: Option<serde_json::Value>,
) {
    let pg = state.pg_store.read().clone();
    if let Some(ref pg) = pg {
        let _ = pg
            .insert_audit_log(action, "admin", target, detail.as_ref(), None)
            .await;
    }
}

#[derive(Deserialize)]
struct AuditLogQuery {
    #[serde(default = "default_audit_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    action: Option<String>,
}

fn default_audit_limit() -> i64 {
    50
}

async fn get_audit_log(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<AuditLogQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pg = state.pg_store.read().clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "PG not available".to_string(),
    ))?;

    let rows = pg
        .load_audit_logs(query.limit, query.offset, query.action.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let entries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, timestamp, action, actor, target, detail, ip)| {
            serde_json::json!({
                "id": id,
                "timestamp": timestamp,
                "action": action,
                "actor": actor,
                "target": target,
                "detail": detail.and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok()),
                "ip_address": ip,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "entries": entries })))
}

#[cfg(test)]
mod metrics_tests {
    use super::{ensure_current_period_stats, generate_hourly_stats_mock, sum_prometheus_counter};

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
            hit_rate: 0.0,
            ..Default::default()
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
