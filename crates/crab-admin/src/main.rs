mod composition;
mod credential_persist;
mod dataplane;
mod domain_usage_sync;
mod infra;
mod key_usage_sync;
mod live_metrics;
mod log_management;
mod metrics_history;
mod metrics_store;
mod network;
mod oauth_codex;
mod overview;
mod peak_hours_aggregator;
mod persist;
mod pg;
mod raw_capture;
mod routes;
mod sse;
mod state;
mod static_cache;
mod suggestions;
mod trace_log;
mod trace_summary;
mod trace_user_id_audit;
mod types;
mod update;
mod upstream;
mod upstream_profiles;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header::HeaderValue};
use axum::middleware::Next;
use axum::{Json, Router, middleware, response::IntoResponse};
use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

#[derive(Debug, Clone)]
struct ServerConfig {
    listen_addr: String,
    cert_path: Option<PathBuf>,
    key_path: Option<PathBuf>,
}

async fn api_no_cache_headers(req: Request<Body>, next: Next) -> Response<Body> {
    let is_api = req.uri().path().starts_with("/api/admin/");
    let mut res = next.run(req).await;
    if is_api {
        res.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
        );
        res.headers_mut().insert(
            axum::http::header::PRAGMA,
            HeaderValue::from_static("no-cache"),
        );
    }
    res
}

impl ServerConfig {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();

        let mut listen_addr = "0.0.0.0:3000".to_string();
        let mut cert_path = None;
        let mut key_path = None;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--listen" | "-l" if i + 1 < args.len() => {
                    listen_addr = args[i + 1].clone();
                    i += 1;
                }
                "--cert" | "-c" if i + 1 < args.len() => {
                    cert_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
                "--key" | "-k" if i + 1 < args.len() => {
                    key_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
                "--https" => {
                    cert_path = Some(PathBuf::from("certs/cert.pem"));
                    key_path = Some(PathBuf::from("certs/key.pem"));
                }
                _ => {}
            }
            i += 1;
        }

        ServerConfig {
            listen_addr,
            cert_path,
            key_path,
        }
    }

    fn is_https(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }
}

/// Spawn a background task that flushes admin state on SIGTERM/SIGINT.
/// Unlike `with_graceful_shutdown`, this does NOT stop the server from
/// accepting connections — it only persists state so a subsequent restart
/// (e.g. docker restart during hot-update) never loses key-pool edits.
fn spawn_shutdown_flusher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => info!("Received SIGINT (Ctrl-C), flushing state…"),
                _ = sigterm.recv() => info!("Received SIGTERM, flushing state…"),
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.ok();
            info!("Received Ctrl-C, flushing state…");
        }
        state.flush_persist();
        info!("Admin state persisted, shutting down");
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Register a global panic hook that logs and aborts to prevent
    // inconsistent shared state from a panicked thread.
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        tracing::error!(
            location = %location,
            message = %message,
            "Panic occurred in Admin Dashboard — aborting"
        );
        std::process::abort();
    }));

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let _ = rustls::crypto::ring::default_provider().install_default();

    let admin_key = std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            tracing::warn!("CRABCACHE_ADMIN_KEY is unset; using 'admin' (debug mode only).");
            "admin".to_string()
        } else {
            eprintln!(
                "FATAL: CRABCACHE_ADMIN_KEY is not set. Refusing to start with default 'admin' key."
            );
            std::process::exit(1);
        }
    });
    if admin_key == "admin" && !cfg!(debug_assertions) {
        tracing::warn!(
            "CRABCACHE_ADMIN_KEY is unset or uses the default 'admin'; set a strong key for production. \
             Dashboard must use the same value in the Admin API Key sign-in screen."
        );
    }

    let config = ServerConfig::from_args();
    let state = Arc::new(AppState::new());

    // PG connect + hydrate run in background so HTTP bind is never blocked on Postgres.
    {
        let init_state = Arc::clone(&state);
        let pg_configured = state.pg_pending_config.read().is_some();
        tokio::spawn(async move {
            loop {
                let pending = init_state.pg_pending_config.read().clone();
                let Some((url, pool_size, migrate)) = pending else {
                    break;
                };
                if AppState::try_connect_pg(&init_state, &url, pool_size, migrate).await {
                    info!("PostgreSQL connected at startup");
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }

            if init_state.pg_store.read().is_some() {
                if init_state.hydrate_profile_secrets_from_pg().await {
                    info!("Profile key pools hydrated from PostgreSQL");
                }
                if crate::credential_persist::hydrate_credentials_from_pg(&init_state).await {
                    info!("OAuth credentials hydrated from PostgreSQL");
                }
                let configs_restored = init_state.hydrate_system_configs_from_pg().await;
                if configs_restored > 0 {
                    info!(configs = configs_restored, "System configs hydrated from PostgreSQL");
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let cutoff = now.saturating_sub(crate::metrics_history::MAX_RETENTION_SECS);
                let pg = init_state.pg_store.read().clone();
                if let Some(pg) = pg {
                    if let Ok(pg_snapshots) = pg.load_metric_snapshots_since(cutoff).await {
                        let sqlite_count = init_state.metrics_history.read().sample_count();
                        if pg_snapshots.len() > sqlite_count {
                            let mut hist = init_state.metrics_history.write();
                            *hist = crate::metrics_history::MetricsHistory::new();
                            for s in &pg_snapshots {
                                hist.append(s.clone());
                            }
                            info!(
                                hydrated = hist.sample_count(),
                                "Metrics history restored from PostgreSQL"
                            );
                        }
                    }
                }
            }
            init_state.reconcile_upstream_from_gateway().await;
            init_state.sync_profile_secrets_from_gateway().await;
            crate::oauth_codex::prepare_auth_dir(&init_state).await;
            init_state.push_all_profile_pools_to_gateway().await;
        });
        if pg_configured {
            info!("Background PG init started (non-blocking + 30s retry)");
        }
    }

    {
        let metrics_state = Arc::clone(&state);
        let interval_secs = crate::metrics_history::sample_interval_secs();
        tokio::spawn(async move {
            loop {
                // Read current gateway uptime for uptime-based metrics persistence.
                let uptime = metrics_state
                    .gateway_probe_cache
                    .read()
                    .as_ref()
                    .and_then(|(_, p)| p.status.as_ref().map(|s| s.uptime_secs))
                    .unwrap_or(0);
                let pg_ref = metrics_state.pg_store.read().clone();
                if let Err(e) = crate::metrics_history::sample_metrics_history(
                    &metrics_state.gateway_metrics_cache,
                    &metrics_state.metrics_history,
                    uptime,
                    pg_ref.as_ref(),
                    metrics_state.metrics_store.as_ref(),
                )
                .await
                {
                    tracing::debug!(error = %e, "Metrics history sample failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            }
        });
        info!(interval_secs, "Metrics history sampler started");
    }

    // Hydrate metrics history from SQLite if PG did not provide data.
    if state.pg_store.read().is_none() {
        if let Some(ref store) = state.metrics_store {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cutoff = now.saturating_sub(crate::metrics_history::MAX_RETENTION_SECS);
            let snaps = store.load_snapshots_since(cutoff);
            if !snaps.is_empty() {
                let mut hist = state.metrics_history.write();
                if hist.sample_count() == 0 {
                    for s in &snaps {
                        hist.append(s.clone());
                    }
                    info!(
                        hydrated = hist.sample_count(),
                        "Metrics history restored from SQLite"
                    );
                }
            }
        }
    }

    {
        crate::infra::collector::spawn_background_collector(Arc::clone(&state));
        info!(
            collect_secs = crate::infra::collector::collect_interval_secs(),
            history_secs = crate::infra::history::sample_interval_secs(),
            "Infra background collector started"
        );
    }

    // Peak hours aggregator: waits for PG then aggregates trace_logs every 5 min.
    {
        let agg_state = Arc::clone(&state);
        tokio::spawn(crate::peak_hours_aggregator::run(agg_state));
        info!("Model peak hours aggregator started");
    }

    if std::env::var("CRABCACHE_MODEL_SYNC_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&s| s > 0)
        .is_some()
    {
        let bg = Arc::clone(&state);
        tokio::spawn(async move {
            let interval_secs = std::env::var("CRABCACHE_MODEL_SYNC_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(3600);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
                let profile_id = bg.default_profile_id();
                match crate::upstream::detect_models_internal(&bg, &profile_id).await {
                    Ok(diff) if !diff.to_add.is_empty() || !diff.to_remove.is_empty() => {
                        tracing::info!(
                            add = diff.to_add.len(),
                            remove = diff.to_remove.len(),
                            "Upstream model drift detected (apply via dashboard)"
                        );
                    }
                    Err(e) => tracing::warn!(error = %e, "Periodic model detect failed"),
                    _ => {}
                }
            }
        });
    }

    state.sync_domain_policies_to_gateway().await;

    // Start key usage sync (reads trace, accumulates keys_meta monthly counters).
    crate::key_usage_sync::spawn(Arc::clone(&state));

    // Start domain_usage sync (fetches from Gateway, persists to PG, restores on restart).
    crate::domain_usage_sync::spawn(Arc::clone(&state));

    {
        let prefetch = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = prefetch.fetch_gateway_metrics().await {
                tracing::warn!(error = %e, "Initial gateway metrics prefetch failed");
            }
            if let Err(e) = crate::overview::refresh_overview_core_cache(&prefetch).await {
                tracing::debug!(error = %e, "Initial overview core cache warm failed");
            }
        });
    }

    {
        let bg = Arc::clone(&state);
        let interval = crate::overview::overview_core_background_interval();
        tokio::spawn(async move {
            let mut last_broadcast_etag = String::new();
            loop {
                if let Err(e) = crate::overview::refresh_overview_core_cache(&bg).await {
                    tracing::debug!(error = %e, "Overview core background refresh failed");
                } else {
                    // Only broadcast when the ETag changes (data actually differs).
                    let cache = bg.overview_core_cache.read();
                    if let Some((_, ref core, ref etag)) = *cache {
                        if etag != &last_broadcast_etag {
                            if let Ok(json) = serde_json::to_value(core) {
                                let _ = bg.sse_broadcast.send(crate::sse::SseEvent::Metrics(json));
                            }
                            last_broadcast_etag = etag.clone();
                        }
                    }
                }
                tokio::time::sleep(interval).await;
            }
        });
        info!(
            interval_secs = interval.as_secs(),
            "Overview core cache refresh started"
        );
    }

    // Spawn log retention enforcement task (runs every 10 minutes).
    {
        let bg = Arc::clone(&state);
        tokio::spawn(crate::log_management::log_retention_loop(bg));
        info!("Log retention enforcement task started");
    }

    match state.gateway.list_keys().await {
        Ok(specs) => {
            for spec in specs {
                use dashmap::mapref::entry::Entry;
                match state.keys_meta.entry(spec.id.clone()) {
                    Entry::Vacant(v) => {
                        v.insert(crate::state::KeyMetadata {
                            id: spec.id.clone(),
                            name: spec.name.clone(),
                            token: spec.key_full.clone().unwrap_or_default(),
                            rpm_limit: spec.rpm_limit as u64,
                            monthly_token_limit: 0,
                            current_rpm: 0,
                            tokens_this_month: 0,
                            input_tokens: 0,
                            output_tokens: 0,
                            expired_at: None,
                            model_limits: Vec::new(),
                            remain_quota: -1,
                            unlimited_quota: true,
                            max_concurrent: spec.max_concurrent,
                            usage_month: String::new(),
                        });
                    }
                    Entry::Occupied(mut o) => {
                        let m = o.get_mut();
                        m.name = spec.name.clone();
                        m.max_concurrent = spec.max_concurrent;
                        m.rpm_limit = spec.rpm_limit as u64;
                        if let Some(full) = spec.key_full.filter(|t| !t.is_empty()) {
                            m.token = full;
                        }
                    }
                }
            }
            info!(
                count = state.keys_meta.len(),
                "Synced API keys from gateway"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Gateway management API unreachable; key operations may fail until gateway is up"
            );
        }
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let dashboard_static = Router::new()
        .fallback_service(
            ServeDir::new("crates/crab-dashboard/dist")
                .fallback(ServeFile::new("crates/crab-dashboard/dist/index.html")),
        )
        .layer(middleware::from_fn(static_cache::static_cache_headers));

    // Important: avoid falling through to SPA `index.html` for API paths.
    // If an `/api/admin/*` route is missing, return a JSON 404 so the dashboard
    // shows a clear error instead of "Parse error: expected value".
    let dashboard_fallback = {
        let dashboard_static = dashboard_static.clone();
        tower::service_fn(move |req: Request<axum::body::Body>| {
            let dashboard_static = dashboard_static.clone();
            async move {
                if req.uri().path().starts_with("/api/admin/") {
                    Ok::<_, std::convert::Infallible>(
                        (
                            StatusCode::NOT_FOUND,
                            Json(serde_json::json!({
                                "error": format!("not_found: {}", req.uri().path())
                            })),
                        )
                            .into_response(),
                    )
                } else {
                    // `Router` is an infallible service; `.oneshot` returns `Result<Response, Infallible>`.
                    Ok::<_, std::convert::Infallible>(
                        dashboard_static
                            .oneshot(req)
                            .await
                            .unwrap_or_else(|e| match e {}),
                    )
                }
            }
        })
    };

    let app = routes::router(Arc::clone(&state))
        .layer(middleware::from_fn(api_no_cache_headers))
        .layer(cors)
        .layer(CompressionLayer::new())
        .fallback_service(dashboard_fallback);

    let protocol = if config.is_https() { "https" } else { "http" };

    info!(
        addr = config.listen_addr,
        protocol = protocol,
        "CrabCache Admin Dashboard starting"
    );

    // Spawn background SIGTERM/SIGINT handler to persist state before exit.
    spawn_shutdown_flusher(Arc::clone(&state));

    if config.is_https() {
        let cert_path = config.cert_path.unwrap();
        let key_path = config.key_path.unwrap();

        info!(
            cert = %cert_path.display(),
            key = %key_path.display(),
            "Using HTTPS with self-signed certificate"
        );

        let tls_config =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;

        let addr: std::net::SocketAddr = config.listen_addr.parse()?;

        axum_server::bind_rustls(addr, tls_config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
        axum::serve(listener, app).await?;
    }

    Ok(())
}
