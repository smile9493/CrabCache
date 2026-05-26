//! PostgreSQL-backed reasoning cache.
//!
//! Uses a single `tokio-postgres` connection (no pool) to minimize dependency
//! footprint.  The connection is wrapped in a `tokio::sync::Mutex` for
//! serialised access.

use serde_json::Value;
use tokio::sync::Mutex;
use tokio_postgres::Client;
use tracing::warn;

pub struct PgReasoningStore {
    client: Mutex<Client>,
    max_age_seconds: Option<u64>,
    max_rows: Option<usize>,
}

impl PgReasoningStore {
    /// Connect to PostgreSQL and create the table if needed.
    pub async fn connect(
        pg_url: &str,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
    ) -> anyhow::Result<Self> {
        let (client, connection) = tokio_postgres::connect(pg_url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                warn!("PG reasoning connection error: {}", e);
            }
        });

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS reasoning_cache (
                    key         TEXT PRIMARY KEY,
                    reasoning   TEXT NOT NULL,
                    message_json TEXT NOT NULL,
                    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_reasoning_created
                 ON reasoning_cache (created_at)",
                &[],
            )
            .await?;

        let store = Self {
            client: Mutex::new(client),
            max_age_seconds,
            max_rows,
        };
        store.prune().await?;
        Ok(store)
    }

    pub async fn put(&self, key: &str, reasoning: &str, message: &Value) {
        let message_json = match serde_json::to_string(message) {
            Ok(j) => j,
            Err(e) => {
                warn!("Failed to serialize reasoning message: {}", e);
                return;
            }
        };
        let client = self.client.lock().await;
        if let Err(e) = client
            .execute(
                "INSERT INTO reasoning_cache (key, reasoning, message_json)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (key) DO UPDATE SET
                    reasoning = EXCLUDED.reasoning,
                    message_json = EXCLUDED.message_json,
                    created_at = now()",
                &[&key, &reasoning, &message_json],
            )
            .await
        {
            warn!("PG reasoning put failed: {}", e);
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let client = self.client.lock().await;
        match client
            .query_opt(
                "SELECT reasoning FROM reasoning_cache WHERE key = $1",
                &[&key],
            )
            .await
        {
            Ok(Some(row)) => {
                crab_metrics::global_metrics().record_reasoning_store_lookup(true);
                Some(row.get(0))
            }
            Ok(None) => {
                crab_metrics::global_metrics().record_reasoning_store_lookup(false);
                None
            }
            Err(e) => {
                warn!("PG reasoning get failed: {}", e);
                crab_metrics::global_metrics().record_reasoning_store_lookup(false);
                None
            }
        }
    }

    pub async fn clear(&self) -> anyhow::Result<usize> {
        let client = self.client.lock().await;
        let count = client.execute("DELETE FROM reasoning_cache", &[]).await? as usize;
        Ok(count)
    }

    /// Remove expired entries and enforce max_rows.
    async fn prune(&self) -> anyhow::Result<()> {
        let client = self.client.lock().await;
        if let Some(max_age) = self.max_age_seconds {
            let _ = client
                .execute(
                    "DELETE FROM reasoning_cache
                     WHERE created_at < now() - make_interval(secs => $1)",
                    &[&(max_age as f64)],
                )
                .await?;
        }
        if let Some(max_rows) = self.max_rows {
            let _ = client
                .execute(
                    "DELETE FROM reasoning_cache
                     WHERE key NOT IN (
                         SELECT key FROM reasoning_cache
                         ORDER BY created_at DESC LIMIT $1
                     )",
                    &[&(max_rows as i64)],
                )
                .await?;
        }
        Ok(())
    }
}
