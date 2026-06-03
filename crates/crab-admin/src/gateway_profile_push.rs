//! Periodic push of upstream profile metadata + key pools from Admin PG to Gateway.
//!
//! Recovers hot-pipe state when Gateway/Redis restarts or Admin starts before Gateway is ready.

use crate::state::AppState;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info, warn};

const INTERVAL_ENV: &str = "CRABCACHE_GATEWAY_PROFILE_PUSH_INTERVAL_SECS";

pub fn push_interval_secs() -> u64 {
    std::env::var(INTERVAL_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Spawn background profile push loop (immediate first tick after PG hydrate path).
pub fn spawn(state: Arc<AppState>) {
    let interval_secs = push_interval_secs();
    if interval_secs == 0 {
        info!("Gateway profile push disabled (interval = 0)");
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        loop {
            if let Err(e) = push_once(&state).await {
                debug!(error = %e, "Gateway profile push tick failed");
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        }
    });
    info!(interval_secs, "Gateway profile push started");
}

async fn push_once(state: &Arc<AppState>) -> Result<(), String> {
    if state.gateway.ready().await.is_err() {
        return Err("gateway not ready".into());
    }

    state.bootstrap_profile_configs_from_gateway_if_empty().await;

    let gw_profiles = state
        .gateway
        .list_upstream_profiles()
        .await
        .map_err(|e| e.to_string())?;
    let gw_ids: HashSet<String> = gw_profiles.profiles.iter().map(|p| p.id.clone()).collect();

    let pg_configs = state.upstream_profile_configs.read().clone();
    let mut needs_full_push = false;

    for cfg in pg_configs.values() {
        if !gw_ids.contains(&cfg.profile_id) {
            warn!(
                profile_id = %cfg.profile_id,
                "Gateway missing profile from PG; pushing metadata"
            );
            if let Err(e) = crate::upstream_profiles::push_profile_config_to_gateway(state, cfg).await
            {
                warn!(profile_id = %cfg.profile_id, error = %e, "Profile metadata push failed");
            } else {
                needs_full_push = true;
            }
        }
    }

    for p in &gw_profiles.profiles {
        let pg_has_keys = state
            .upstream_profile_secrets
            .read()
            .get(&p.id)
            .is_some_and(|pool| !pool.is_empty());
        if pg_has_keys && p.keys_available == 0 {
            warn!(
                profile_id = %p.id,
                "Gateway keys_available=0 but PG has keys; pushing pool"
            );
            needs_full_push = true;
            if let Err(e) = crate::upstream_profiles::sync_profile_pool_to_gateway(
                state,
                &p.id,
                crate::types::UpstreamKeysPutMode::Replace,
            )
            .await
            {
                warn!(profile_id = %p.id, error = %e, "Profile key pool push failed");
            }
        }
    }

    if needs_full_push {
        debug!("Drift detected; completed targeted profile push");
    }

    Ok(())
}
