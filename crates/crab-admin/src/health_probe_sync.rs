//! Background sync for health probes.
//!
//! Periodically fetches Gateway health status and persists it to PG
//! `health_probes` table for availability SLA tracking.

use crate::state::AppState;
use std::sync::Arc;
use tracing::{debug, info};

/// Interval between health probes (env override, default 30s).
const PROBE_INTERVAL_ENV: &str = "CRABCACHE_HEALTH_PROBE_INTERVAL_SECS";

pub fn probe_interval_secs() -> u64 {
    std::env::var(PROBE_INTERVAL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Spawn the background health probe loop. Called once at startup.
pub fn spawn(state: Arc<AppState>) {
    let interval_secs = probe_interval_secs();
    if interval_secs == 0 {
        info!("Health probe sync disabled (interval = 0)");
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            if let Err(e) = probe_once(&state).await {
                debug!(error = %e, "Health probe failed");
            }
        }
    });
    info!(interval_secs, "Health probe sync started");
}

async fn probe_once(state: &Arc<AppState>) -> Result<(), String> {
    let pg = state.pg_store.read().clone().ok_or("PG not available")?;

    // Fetch health from Gateway.
    let ready_resp = state
        .gateway
        .ready_detail()
        .await
        .map_err(|e| format!("ready_detail: {e}"))?;

    let status_resp = state
        .gateway
        .status()
        .await
        .map_err(|e| format!("status: {e}"))?;

    let gateway_ready = ready_resp.ready;
    let redis_status = Some(ready_resp.redis.as_str());
    let l2_status = Some(ready_resp.l2.as_str());
    let uptime_secs = Some(status_resp.uptime_secs as i64);
    let active_keys = Some(status_resp.active_keys as i32);

    pg.insert_health_probe(
        gateway_ready,
        redis_status,
        l2_status,
        uptime_secs,
        active_keys,
    )
    .await
    .map_err(|e| format!("insert_health_probe: {e}"))?;

    // Prune old probes (keep 30 days).
    let cutoff_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        - 30 * 24 * 3600 * 1000;
    if let Ok(count) = pg.prune_health_probes(cutoff_ms).await {
        if count > 0 {
            debug!(deleted = count, "Pruned old health probes");
        }
    }

    Ok(())
}
