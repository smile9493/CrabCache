use crate::redis_store::RedisStateStore;
use crate::snapshot::{apply_snapshot_to_runtime, build_snapshot_from_runtime};
use crab_proxy::RuntimeConfig;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Poll Redis revision and refresh local `RuntimeConfig` when another instance writes.
pub fn spawn_state_refresh_task(
    store: Arc<RedisStateStore>,
    runtime: Arc<RuntimeConfig>,
    upstream_cooldown_secs: u64,
    interval_secs: u64,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("state refresh runtime");
        rt.block_on(async move {
            let mut last_version = store.current_version().await.unwrap_or(0);
            let mut interval =
                tokio::time::interval(Duration::from_secs(interval_secs.max(1)));
            loop {
                interval.tick().await;
                let version = match store.current_version().await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(error = %e, "Failed to read state version from Redis");
                        continue;
                    }
                };
                if version == last_version {
                    continue;
                }
                match store.load_all().await {
                    Ok((v, snap)) => {
                        if let Err(e) =
                            apply_snapshot_to_runtime(&runtime, &snap, upstream_cooldown_secs)
                        {
                            warn!(error = %e, "Failed to apply control plane snapshot");
                        } else {
                            debug!(version = v, keys = snap.keys.len(), "Refreshed control plane from Redis");
                            last_version = v;
                        }
                    }
                    Err(e) => warn!(error = %e, "Failed to load control plane from Redis"),
                }
            }
        });
    });
}

/// Persist current runtime snapshot to Redis (after local mutation).
pub async fn persist_runtime_state(
    store: &RedisStateStore,
    runtime: &RuntimeConfig,
) -> anyhow::Result<u64> {
    let snap = build_snapshot_from_runtime(runtime);
    store.save_all(&snap).await
}
