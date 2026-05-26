mod composition;
mod infra;
mod key_usage_sync;
mod live_metrics;
mod log_management;
mod metrics_history;
mod metrics_store;
mod network;
mod openresty;
mod overview;
mod persist;
mod pg;
mod raw_capture;
mod routes;
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

use axum::{Router, middleware};
use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;
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

    let admin_key = std::env::var("CRABCACHE_ADMIN_KEY").unwrap_or_else(|_| "admin".to_string());
    if admin_key == "admin" {
        tracing::warn!(
            "CRABCACHE_ADMIN_KEY is unset or uses the default 'admin'; set a strong key for production. \
             Dashboard must use the same value in the Admin API Key sign-in screen."
        );
    }

    let config = ServerConfig::from_args();
    let state = Arc::new(AppState::new());
    state.reconcile_upstream_from_gateway().await;

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
                    metrics_state.metrics_store.as_ref(),
                    uptime,
                    pg_ref.as_ref(),
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

    // Background PG init retry: if PG was configured but unavailable at startup,
    // retry every 30s until connected, then run migration and hydrate metrics.
    if state.pg_pending_config.read().is_some() {
        let retry_state = Arc::clone(&state);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let pending = retry_state.pg_pending_config.read().clone();
                let Some((url, pool_size, migrate)) = pending else {
                    break; // PG connected or never configured
                };
                match crate::pg::PgStore::new(&url, pool_size).await {
                    Ok(pg) => {
                        info!("PostgreSQL connection established on retry");
                        if migrate {
                            if let Ok(true) =
                                pg.maybe_import_from_json(&retry_state.persist.load()).await
                            {
                                info!("JSON state imported into PostgreSQL (retry)");
                            }
                        }
                        // Hydrate metrics from PG.
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let cutoff = now.saturating_sub(crate::metrics_history::MAX_RETENTION_SECS);
                        if let Ok(snaps) = pg.load_metric_snapshots_since(cutoff).await {
                            let sqlite_count = retry_state.metrics_history.read().sample_count();
                            if snaps.len() > sqlite_count {
                                let mut hist = retry_state.metrics_history.write();
                                *hist = crate::metrics_history::MetricsHistory::new();
                                for s in &snaps {
                                    hist.append(s.clone());
                                }
                                info!(
                                    hydrated = hist.sample_count(),
                                    "Metrics history restored from PostgreSQL (retry)"
                                );
                            }
                        }
                        *retry_state.pg_store.write() = Some(pg);
                        *retry_state.pg_pending_config.write() = None;
                        info!("PostgreSQL fully initialized (retry successful)");
                        break;
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "PG retry failed; will try again in 30s");
                    }
                }
            }
        });
        info!("Background PG init retry started (every 30s)");
    }

    {
        crate::infra::collector::spawn_background_collector(Arc::clone(&state));
        info!(
            collect_secs = crate::infra::collector::collect_interval_secs(),
            history_secs = crate::infra::history::sample_interval_secs(),
            "Infra background collector started"
        );
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
            loop {
                if let Err(e) = crate::overview::refresh_overview_core_cache(&bg).await {
                    tracing::debug!(error = %e, "Overview core background refresh failed");
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

    let app = routes::router(state)
        .layer(cors)
        .layer(CompressionLayer::new())
        .fallback_service(dashboard_static);

    let protocol = if config.is_https() { "https" } else { "http" };

    info!(
        addr = config.listen_addr,
        protocol = protocol,
        "CrabCache Admin Dashboard starting"
    );

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
