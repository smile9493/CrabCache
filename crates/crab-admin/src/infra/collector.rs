//! Single background task for infra snapshots (cache + history + rate baselines).

use crate::infra;
use crate::state::AppState;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// How often to poll Docker and refresh the snapshot cache (seconds).
pub fn collect_interval_secs() -> u64 {
    std::env::var("CRABCACHE_INFRA_COLLECT_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| infra::resolve_cache_ttl_secs().max(5))
}

/// Spawn the sole infra collector loop (HTTP handlers read cache only).
pub fn spawn_background_collector(state: Arc<AppState>) {
    tokio::spawn(async move {
        let collect_secs = collect_interval_secs();
        let history_secs = infra::history::sample_interval_secs();
        let mut last_history = Instant::now()
            .checked_sub(std::time::Duration::from_secs(history_secs))
            .unwrap_or_else(Instant::now);

        loop {
            let compose = infra::resolve_compose_project();
            let snapshot =
                infra::collect_snapshot(&state.infra_docker, &compose, &state.infra_prev).await;
            state.infra_cache.store(snapshot.clone());

            if last_history.elapsed().as_secs() >= history_secs {
                state.infra_history.write().append(&snapshot);
                last_history = Instant::now();
            }

            state.infra_speed_jobs.purge_expired();

            tokio::time::sleep(std::time::Duration::from_secs(collect_secs)).await;
        }
    });
}

impl crate::infra::InfraCache {
    /// Latest snapshot regardless of TTL (for API reads).
    pub fn get_latest(&self) -> Option<crate::infra::types::InfraSnapshot> {
        self.snapshot.read().as_ref().map(|(s, _)| s.clone())
    }

    pub fn latest_collected_at(&self) -> Option<u64> {
        self.snapshot.read().as_ref().map(|(s, _)| s.collected_at)
    }
}

/// Empty placeholder before the first collector tick.
pub fn empty_snapshot(
    docker_connected: bool,
    error: Option<String>,
) -> crate::infra::types::InfraSnapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    crate::infra::types::InfraSnapshot {
        containers: vec![],
        host_disks: infra::host::collect_host_disks(),
        volumes: vec![],
        collected_at: now,
        compose_project: infra::resolve_compose_project(),
        docker_connected,
        collection_error: error,
    }
}
