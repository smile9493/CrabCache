//! PG-backed control-plane snapshot store for the Gateway.
//!
//! Periodically serializes the current `ControlPlaneSnapshot` and UPSERTs it
//! into `gateway_state_snapshots` so that Admin can recover from PG when Redis
//! is unavailable.

use anyhow::Context;
use crab_state::ControlPlaneSnapshot;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Interval between snapshot writes (env override: `CRABCACHE_GW_PG_SNAPSHOT_SECS`).
pub fn snapshot_interval_secs() -> u64 {
    std::env::var("CRABCACHE_GW_PG_SNAPSHOT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}

/// Background writer that receives snapshots via mpsc and writes them to PG.
pub struct PgControlStore {
    client: tokio::sync::Mutex<tokio_postgres::Client>,
}

impl PgControlStore {
    pub async fn connect(pg_url: &str) -> anyhow::Result<Self> {
        let (client, connection) = tokio_postgres::connect(pg_url, tokio_postgres::NoTls)
            .await
            .context("pg control connect")?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                warn!("PG control connection error: {}", e);
            }
        });
        Ok(Self {
            client: tokio::sync::Mutex::new(client),
        })
    }

    /// Upsert a full snapshot into `gateway_state_snapshots`.
    pub async fn upsert_snapshot(&self, snap: &ControlPlaneSnapshot, version: i64) -> anyhow::Result<()> {
        let keys_json = serde_json::to_value(&snap.keys)
            .context("serialize keys")?;
        let runtime_json = serde_json::to_value(&snap.runtime)
            .context("serialize runtime")?;
        let profiles_json = serde_json::to_value(&snap.upstream_profiles)
            .context("serialize profiles")?;
        let key_states_json = serde_json::to_value(&snap.key_states)
            .context("serialize key_states")?;
        let domain_policies_json = serde_json::to_value(&snap.domain_policies)
            .context("serialize domain_policies")?;
        let keys_pg = tokio_postgres::types::Json(&keys_json);
        let runtime_pg = tokio_postgres::types::Json(&runtime_json);
        let profiles_pg = tokio_postgres::types::Json(&profiles_json);
        let key_states_pg = tokio_postgres::types::Json(&key_states_json);
        let domain_policies_pg = tokio_postgres::types::Json(&domain_policies_json);

        let client = self.client.lock().await;
        client
            .execute(
                "INSERT INTO gateway_state_snapshots
                    (snapshot_type, keys_json, runtime_json, profiles_json,
                     key_states_json, domain_policies_json, version, source)
                 VALUES ('full', $1::jsonb, $2::jsonb, $3::jsonb, $4::jsonb, $5::jsonb, $6, 'gateway_direct')
                 ON CONFLICT DO NOTHING",
                &[
                    &keys_pg,
                    &runtime_pg,
                    &profiles_pg,
                    &key_states_pg,
                    &domain_policies_pg,
                    &version,
                ],
            )
            .await
            .context("pg upsert_gateway_snapshot")?;

        debug!(version, "Gateway control-plane snapshot written to PG");
        Ok(())
    }
}

/// Spawn the background snapshot writer loop.
/// Reads from `rx` and writes to PG. Exits when `rx` is closed.
pub fn spawn_snapshot_writer(
    pg_url: String,
    mut rx: mpsc::Receiver<ControlPlaneSnapshot>,
    version_rx: Arc<std::sync::atomic::AtomicI64>,
) {
    tokio::spawn(async move {
        let store = match PgControlStore::connect(&pg_url).await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to connect PG control store: {}", e);
                return;
            }
        };
        while let Some(snap) = rx.recv().await {
            let ver = version_rx.load(std::sync::atomic::Ordering::Relaxed);
            if let Err(e) = store.upsert_snapshot(&snap, ver).await {
                warn!("PG control snapshot write failed: {:#}", e);
            }
        }
        debug!("PG control snapshot writer exited");
    });
}

/// Build a snapshot from the current runtime state.
pub fn build_snapshot(runtime: &crab_proxy::RuntimeConfig) -> ControlPlaneSnapshot {
    crab_state::build_snapshot_from_runtime(runtime)
}
