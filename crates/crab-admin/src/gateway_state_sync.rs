//! Background sync for gateway control-plane snapshots.
//!
//! Periodically fetches the Gateway's current control-plane state via
//! `GET /v1/state/snapshot` and persists it to PG `gateway_state_snapshots`.
//! This provides a secondary PG backup of the Redis control-plane state,
//! enabling disaster recovery when Redis is unavailable.

use crate::state::AppState;
use std::sync::Arc;
use tracing::{debug, info};

/// Interval between sync ticks (env override, default 60s).
const SYNC_INTERVAL_ENV: &str = "CRABCACHE_GATEWAY_STATE_SYNC_INTERVAL_SECS";

pub fn sync_interval_secs() -> u64 {
    std::env::var(SYNC_INTERVAL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

/// Spawn the background gateway state sync loop. Called once at startup.
pub fn spawn(state: Arc<AppState>) {
    let interval_secs = sync_interval_secs();
    if interval_secs == 0 {
        info!("Gateway state sync disabled (interval = 0)");
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            if let Err(e) = sync_once(&state).await {
                debug!(error = %e, "Gateway state sync failed");
            }
        }
    });
    info!(interval_secs, "Gateway state sync started");
}

async fn sync_once(state: &Arc<AppState>) -> Result<(), String> {
    let pg = state.pg_store.read().clone().ok_or("PG not available")?;

    // Fetch current snapshot from Gateway Management API.
    let body = state
        .gateway
        .get_state_snapshot()
        .await
        .map_err(|e| format!("get_state_snapshot: {e}"))?;

    // Extract version if present (currently 0 from gateway_direct writes).
    let version = body.get("version").and_then(|v| v.as_i64()).unwrap_or(0);

    let keys_json = body.get("keys").cloned();
    let runtime_json = body.get("runtime").cloned();
    let profiles_json = body.get("profiles").cloned();
    let key_states_json = body.get("key_states").cloned();
    let domain_policies_json = body.get("domain_policies").cloned();

    pg.upsert_gateway_snapshot(
        "full",
        keys_json.as_ref(),
        runtime_json.as_ref(),
        profiles_json.as_ref(),
        key_states_json.as_ref(),
        domain_policies_json.as_ref(),
        version,
        "admin_pull",
    )
    .await
    .map_err(|e| format!("upsert_gateway_snapshot: {e}"))?;

    debug!("Gateway state snapshot persisted to PG (admin_pull)");

    // Prune old snapshots (keep last 7 days = 168 hourly snapshots).
    let cutoff_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        - 7 * 24 * 3600 * 1000;
    if let Ok(count) = pg.prune_gateway_snapshots(cutoff_ms).await {
        if count > 0 {
            debug!(deleted = count, "Pruned old gateway snapshots");
        }
    }

    Ok(())
}
