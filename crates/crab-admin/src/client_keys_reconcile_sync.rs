//! Background reconciliation: PG `keys_meta` → Gateway Redis when drift or restart is detected.

use crate::control_plane_restore::restore_control_plane_from_pg;
use crate::gateway_uptime::{current_gateway_uptime_secs, detect_gateway_restart};
use crate::state::AppState;
use std::sync::Arc;
use tracing::{debug, info};

const INTERVAL_ENV: &str = "CRABCACHE_CLIENT_KEYS_RECONCILE_INTERVAL_SECS";

pub fn reconcile_interval_secs() -> u64 {
    std::env::var(INTERVAL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// `true` when Gateway restart was detected or PG/local metadata has more keys than Gateway.
pub fn needs_client_key_reconcile(
    local_token_keys: usize,
    gateway_key_count: usize,
    gateway_restarted: bool,
) -> bool {
    gateway_restarted || (local_token_keys > 0 && gateway_key_count < local_token_keys)
}

pub fn spawn(state: Arc<AppState>) {
    let interval_secs = reconcile_interval_secs();
    if interval_secs == 0 {
        info!("Client keys reconcile sync disabled (interval = 0)");
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let mut prev_uptime: Option<u64> = None;
        loop {
            if let Err(e) = reconcile_once(&state, &mut prev_uptime).await {
                debug!(error = %e, "Client keys reconcile tick failed");
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        }
    });
    info!(interval_secs, "Client keys reconcile sync started");
}

async fn reconcile_once(state: &Arc<AppState>, prev_uptime: &mut Option<u64>) -> Result<(), String> {
    if state.pg_store.read().is_none() {
        return Err("PG not available".into());
    }
    if state.gateway.ready().await.is_err() {
        return Err("gateway not ready".into());
    }

    let curr_uptime = current_gateway_uptime_secs(state);
    let restarted = detect_gateway_restart(*prev_uptime, curr_uptime);
    *prev_uptime = curr_uptime;

    let gateway_specs = state
        .gateway
        .list_keys()
        .await
        .map_err(|e| e.to_string())?;
    let gateway_count = gateway_specs.len();

    let local_token_keys = state
        .keys_meta
        .iter()
        .filter(|e| !e.value().token.is_empty())
        .count();

    let pg = state.pg_store.read().clone();
    let pg_token_keys = if let Some(pg) = pg {
        pg.count_keys_meta_with_token()
            .await
            .map_err(|e| e.to_string())? as usize
    } else {
        0
    };

    let authoritative = local_token_keys.max(pg_token_keys);

    if !needs_client_key_reconcile(authoritative, gateway_count, restarted) {
        return Ok(());
    }

    info!(
        gateway_keys = gateway_count,
        local_token_keys,
        pg_token_keys,
        restarted,
        "Client key drift detected; restoring control plane on gateway"
    );
    restore_control_plane_from_pg(state).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_reconcile_on_restart() {
        assert!(needs_client_key_reconcile(5, 5, true));
    }

    #[test]
    fn needs_reconcile_on_drift() {
        assert!(needs_client_key_reconcile(3, 1, false));
        assert!(!needs_client_key_reconcile(0, 0, false));
        assert!(!needs_client_key_reconcile(2, 2, false));
    }
}
