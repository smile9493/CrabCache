use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tracing::{debug, warn};

use crate::redis_store::RedisStateStore;
use crate::snapshot::{apply_snapshot_to_runtime, build_snapshot_from_runtime};
use crab_metrics::global_metrics;
use crab_proxy::RuntimeConfig;

const PERSIST_MAX_ATTEMPTS: u32 = 3;

async fn refresh_from_store(
    store: &RedisStateStore,
    runtime: &RuntimeConfig,
    upstream_cooldown_secs: u64,
    last_version: &mut u64,
) {
    let version = match store.current_version().await {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Failed to read state version from Redis");
            return;
        }
    };
    if version == *last_version {
        return;
    }
    match store.load_all().await {
        Ok((v, snap)) => {
            if let Err(e) = apply_snapshot_to_runtime(runtime, &snap, upstream_cooldown_secs) {
                warn!(error = %e, "Failed to apply control plane snapshot");
            } else {
                debug!(
                    version = v,
                    keys = snap.keys.len(),
                    domains = snap.domain_policies.len(),
                    "Refreshed control plane from Redis"
                );
                *last_version = v;
            }
        }
        Err(e) => warn!(error = %e, "Failed to load control plane from Redis"),
    }
}

/// Poll Redis revision and subscribe to pub/sub; refresh local `RuntimeConfig` on change.
pub fn spawn_state_refresh_task(
    store: Arc<RedisStateStore>,
    runtime: Arc<RuntimeConfig>,
    upstream_cooldown_secs: u64,
    interval_secs: u64,
) {
    let redis_url = store.redis_url().to_string();
    let rev_channel = format!("{}:rev", store.key_prefix());

    let runtime_sub = runtime.clone();
    let runtime_poll = runtime.clone();
    let poll_secs = interval_secs.max(1);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("state refresh runtime");
        rt.block_on(async move {
            let store = store;
            let store_poll = store.clone();
            tokio::spawn(async move {
                let mut last_version = store_poll.current_version().await.unwrap_or(0);
                let mut interval = tokio::time::interval(Duration::from_secs(poll_secs));
                loop {
                    interval.tick().await;
                    refresh_from_store(
                        &store_poll,
                        &runtime_poll,
                        upstream_cooldown_secs,
                        &mut last_version,
                    )
                    .await;
                }
            });

            let store_sub = store.clone();
            let mut last_version = store_sub.current_version().await.unwrap_or(0);
            loop {
                let client = match redis::Client::open(redis_url.as_str()) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(error = %e, "State pub/sub: redis client open failed, retrying");
                        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
                        continue;
                    }
                };
                let conn = match client.get_async_connection().await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(error = %e, "State pub/sub: connection failed, retrying");
                        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
                        continue;
                    }
                };
                let mut pubsub = conn.into_pubsub();
                if let Err(e) = pubsub.subscribe(&rev_channel).await {
                    warn!(error = %e, channel = %rev_channel, "State pub/sub subscribe failed");
                    tokio::time::sleep(Duration::from_secs(poll_secs)).await;
                    continue;
                }
                debug!(channel = %rev_channel, "State pub/sub subscribed");
                let mut stream = pubsub.on_message();
                while let Some(msg) = stream.next().await {
                    let _payload: String = match msg.get_payload() {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(error = %e, "State pub/sub invalid payload");
                            continue;
                        }
                    };
                    refresh_from_store(
                        &store_sub,
                        &runtime_sub,
                        upstream_cooldown_secs,
                        &mut last_version,
                    )
                    .await;
                }
                warn!(channel = %rev_channel, "State pub/sub stream ended, reconnecting");
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

/// Persist with exponential backoff; records Prometheus success/error counters.
pub async fn persist_runtime_state_with_retry(
    store: &RedisStateStore,
    runtime: &RuntimeConfig,
) -> anyhow::Result<u64> {
    let mut delay = Duration::from_millis(50);
    let mut last_err = None;
    for attempt in 1..=PERSIST_MAX_ATTEMPTS {
        match persist_runtime_state(store, runtime).await {
            Ok(v) => {
                global_metrics().record_state_persist_success();
                return Ok(v);
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < PERSIST_MAX_ATTEMPTS {
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                }
            }
        }
    }
    global_metrics().record_state_persist_error();
    Err(last_err.unwrap())
}
