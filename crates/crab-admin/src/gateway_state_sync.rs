//! Background sync for gateway control-plane snapshots.
//!
//! Periodically fetches the Gateway's current control-plane state via
//! `GET /v1/state/snapshot` and persists it to PG `gateway_state_snapshots`.
//! This provides a secondary PG backup of the Redis control-plane state,
//! enabling disaster recovery when Redis is unavailable.

use crate::client_keys_reconcile_sync::needs_client_key_reconcile;
use crate::control_plane_restore::restore_control_plane_from_pg;
use crate::gateway_uptime::{current_gateway_uptime_secs, detect_gateway_restart};
use crate::state::AppState;
use std::sync::Arc;
use tracing::{debug, info, warn};

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
        let mut prev_uptime: Option<u64> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            if let Err(e) = sync_once(&state, &mut prev_uptime).await {
                debug!(error = %e, "Gateway state sync failed");
            }
        }
    });
    info!(interval_secs, "Gateway state sync started");
}

fn snapshot_keys_len(keys_json: Option<&serde_json::Value>) -> usize {
    keys_json
        .and_then(|v| v.as_object())
        .map(|m| m.len())
        .unwrap_or(0)
}

async fn sync_once(state: &Arc<AppState>, prev_uptime: &mut Option<u64>) -> Result<(), String> {
    let pg = state.pg_store.read().clone().ok_or("PG not available")?;

    if state.gateway.ready().await.is_err() {
        return Err("gateway not ready".into());
    }

    let curr_uptime = current_gateway_uptime_secs(state);
    let restarted = detect_gateway_restart(*prev_uptime, curr_uptime);
    *prev_uptime = curr_uptime;

    let gateway_count = state
        .gateway
        .list_keys()
        .await
        .map_err(|e| format!("list_keys: {e}"))?
        .len();
    let local_token_keys = state
        .keys_meta
        .iter()
        .filter(|e| !e.value().token.is_empty())
        .count();
    let pg_token_keys = pg
        .count_keys_meta_with_token()
        .await
        .map_err(|e| format!("count_keys_meta_with_token: {e}"))? as usize;
    let authoritative = local_token_keys.max(pg_token_keys);

    if needs_client_key_reconcile(authoritative, gateway_count, restarted) {
        restore_control_plane_from_pg(state).await;
    }

    let body = state
        .gateway
        .get_state_snapshot()
        .await
        .map_err(|e| format!("get_state_snapshot: {e}"))?;

    let version = body.get("version").and_then(|v| v.as_i64()).unwrap_or(0);

    let keys_json = body.get("keys").cloned();
    let runtime_json = body.get("runtime").cloned();
    let profiles_json = body.get("profiles").cloned();
    let key_states_json = body.get("key_states").cloned();
    let domain_policies_json = body.get("domain_policies").cloned();

    let keys_len = snapshot_keys_len(keys_json.as_ref());
    if keys_len == 0 && pg_token_keys > 0 {
        warn!(
            pg_token_keys,
            gateway_keys = gateway_count,
            "Skipping gateway state snapshot persist: gateway keys empty but PG has client keys"
        );
        return Ok(());
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_keys_len_counts_object_entries() {
        let v = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(snapshot_keys_len(Some(&v)), 2);
        assert_eq!(snapshot_keys_len(None), 0);
        assert_eq!(snapshot_keys_len(Some(&serde_json::json!([]))), 0);
    }
}
