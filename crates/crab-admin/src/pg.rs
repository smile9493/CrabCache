//! PostgreSQL persistence backend for Admin Dashboard state.
//!
//! Provides `PgStore` — an async wrapper around `deadpool_postgres::Pool` that
//! replaces the JSON file (`admin-state.json`) and SQLite (`metrics.sqlite`)
//! with relational tables.  Used in dual-write mode alongside the legacy
//! backends until PG is proven stable.

use crate::metrics_history::MetricsCounterSnapshot;
use crate::persist::{
    AdminStateFile, PersistedDomainPolicy, PersistedKeyMetadata, PersistedModel, PersistedModels,
    PersistedUpstreamPoolSecret, PersistedUpstreamSnapshot,
};
use crate::state::StoredUpstreamConfig;
use crate::types::UpstreamTestResult;
use anyhow::{Context, Result};
use deadpool_postgres::{Config as PoolConfig, Pool, Runtime};
use std::collections::HashMap;
use tokio_postgres::NoTls;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Environment variable holding the PostgreSQL connection URL.
const PG_URL_ENV: &str = "CRADMIN_PG_URL";

/// Environment variable for max pool size.
const PG_POOL_SIZE_ENV: &str = "CRADMIN_PG_MAX_POOL_SIZE";

/// Whether to auto-import JSON state on first PG start.
const PG_MIGRATE_ENV: &str = "CRADMIN_PG_MIGRATE_FROM_JSON";

const DEFAULT_POOL_SIZE: usize = 16;

/// Parsed PG configuration.  `None` URL means PG is disabled.
pub struct PgConfig {
    pub url: Option<String>,
    pub max_pool_size: usize,
    pub migrate_from_json: bool,
}

impl PgConfig {
    pub fn from_env() -> Self {
        let url = std::env::var(PG_URL_ENV).ok().filter(|u| !u.is_empty());
        let max_pool_size = std::env::var(PG_POOL_SIZE_ENV)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_POOL_SIZE);
        let migrate_from_json = std::env::var(PG_MIGRATE_ENV)
            .ok()
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        Self {
            url,
            max_pool_size,
            migrate_from_json,
        }
    }

    pub fn enabled(&self) -> bool {
        self.url.is_some()
    }
}

// ---------------------------------------------------------------------------
// PgStore
// ---------------------------------------------------------------------------

/// Async PostgreSQL store backed by a `deadpool_postgres` connection pool.
#[derive(Clone)]
pub struct PgStore {
    pool: Pool,
}

impl PgStore {
    /// Create a new store and run schema migrations.
    pub async fn new(url: &str, max_pool_size: usize) -> Result<Self> {
        let mut cfg = PoolConfig::new();
        cfg.url = Some(url.to_string());
        cfg.pool = Some(deadpool_postgres::PoolConfig {
            max_size: max_pool_size,
            ..Default::default()
        });

        let pool = cfg
            .create_pool(Some(Runtime::Tokio1), NoTls)
            .context("failed to create PG pool")?;

        // Verify connectivity.
        let _ = pool.get().await.context("PG pool: cannot get connection")?;

        let store = Self { pool };
        store.run_migrations().await?;
        info!(url = %redact_url(url), "PostgreSQL store connected and migrated");
        Ok(store)
    }

    // -----------------------------------------------------------------------
    // Schema migrations
    // -----------------------------------------------------------------------

    pub async fn run_migrations(&self) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS keys_meta (
                    id              TEXT PRIMARY KEY,
                    token           TEXT NOT NULL,
                    name            TEXT NOT NULL DEFAULT '',
                    rpm_limit       BIGINT NOT NULL DEFAULT 0,
                    monthly_token_limit BIGINT NOT NULL DEFAULT 0,
                    expired_at      BIGINT,
                    model_limits    JSONB NOT NULL DEFAULT '[]',
                    remain_quota    BIGINT NOT NULL DEFAULT 0,
                    unlimited_quota BOOLEAN NOT NULL DEFAULT false,
                    max_concurrent  INTEGER NOT NULL DEFAULT 0,
                    usage_month     TEXT NOT NULL DEFAULT '',
                    tokens_this_month BIGINT NOT NULL DEFAULT 0,
                    input_tokens    BIGINT NOT NULL DEFAULT 0,
                    output_tokens   BIGINT NOT NULL DEFAULT 0,
                    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS models (
                    profile_id          TEXT NOT NULL DEFAULT 'deepseek',
                    model_id            TEXT NOT NULL,
                    owned_by            TEXT NOT NULL DEFAULT '',
                    context_length      BIGINT,
                    input_price_per_mtok  DOUBLE PRECISION,
                    output_price_per_mtok DOUBLE PRECISION,
                    available           BOOLEAN NOT NULL DEFAULT true,
                    PRIMARY KEY (profile_id, model_id)
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS model_sync_state (
                    profile_id TEXT PRIMARY KEY,
                    synced_at  TEXT NOT NULL DEFAULT ''
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS domain_policies (
                    domain                TEXT PRIMARY KEY,
                    monthly_token_budget  BIGINT NOT NULL DEFAULT 0,
                    monthly_cost_budget_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
                    min_hit_rate          DOUBLE PRECISION NOT NULL DEFAULT 0,
                    enabled               BOOLEAN NOT NULL DEFAULT true,
                    pipeline              TEXT,
                    upstream_profile      TEXT
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS upstream_config (
                    singleton   BOOLEAN PRIMARY KEY DEFAULT true,
                    base_url    TEXT NOT NULL DEFAULT 'https://api.deepseek.com',
                    model       TEXT NOT NULL DEFAULT 'deepseek-v4-pro',
                    endpoints   JSONB NOT NULL DEFAULT '[]',
                    notes       TEXT,
                    last_test   JSONB,
                    CHECK (singleton = true)
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS upstream_pool_secrets (
                    id      TEXT NOT NULL,
                    secret  TEXT NOT NULL,
                    enabled BOOLEAN NOT NULL DEFAULT true,
                    PRIMARY KEY (id, secret)
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS upstream_profile_secrets (
                    profile_id TEXT NOT NULL,
                    key_id     TEXT NOT NULL,
                    secret     TEXT NOT NULL,
                    enabled    BOOLEAN NOT NULL DEFAULT true,
                    PRIMARY KEY (profile_id, key_id)
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS metrics_snapshots (
                    sampled_at         BIGINT PRIMARY KEY,
                    gateway_uptime_secs BIGINT NOT NULL,
                    payload            JSONB NOT NULL
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_metrics_snapshots_at
                 ON metrics_snapshots(sampled_at)",
                &[],
            )
            .await?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // keys_meta CRUD
    // -----------------------------------------------------------------------

    pub async fn upsert_key(&self, meta: &PersistedKeyMetadata) -> Result<()> {
        let client = self.pool.get().await?;
        let stmt = client
            .prepare_cached(
                "INSERT INTO keys_meta
                    (id, token, name, rpm_limit, monthly_token_limit, expired_at,
                     model_limits, remain_quota, unlimited_quota, max_concurrent,
                     usage_month, tokens_this_month, input_tokens, output_tokens)
                 VALUES ($1,$2,$3,$4,$5,$6,$7::jsonb,$8,$9,$10,$11,$12,$13,$14)
                 ON CONFLICT (id) DO UPDATE SET
                    token = EXCLUDED.token,
                    name = EXCLUDED.name,
                    rpm_limit = EXCLUDED.rpm_limit,
                    monthly_token_limit = EXCLUDED.monthly_token_limit,
                    expired_at = EXCLUDED.expired_at,
                    model_limits = EXCLUDED.model_limits,
                    remain_quota = EXCLUDED.remain_quota,
                    unlimited_quota = EXCLUDED.unlimited_quota,
                    max_concurrent = EXCLUDED.max_concurrent,
                    usage_month = EXCLUDED.usage_month,
                    tokens_this_month = EXCLUDED.tokens_this_month,
                    input_tokens = EXCLUDED.input_tokens,
                    output_tokens = EXCLUDED.output_tokens,
                    updated_at = now()",
            )
            .await?;

        let model_limits_json = serde_json::to_string(&meta.model_limits).unwrap_or_default();
        client
            .execute(
                &stmt,
                &[
                    &meta.id,
                    &meta.token,
                    &meta.name,
                    &(meta.rpm_limit as i64),
                    &(meta.monthly_token_limit as i64),
                    &meta.expired_at.map(|v| v as i64),
                    &model_limits_json,
                    &(meta.remain_quota as i64),
                    &meta.unlimited_quota,
                    &(meta.max_concurrent as i32),
                    &meta.usage_month,
                    &(meta.tokens_this_month as i64),
                    &(meta.input_tokens as i64),
                    &(meta.output_tokens as i64),
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn delete_key(&self, id: &str) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute("DELETE FROM keys_meta WHERE id = $1", &[&id])
            .await?;
        Ok(())
    }

    pub async fn load_all_keys(&self) -> Result<Vec<PersistedKeyMetadata>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT id, token, name, rpm_limit, monthly_token_limit, expired_at,
                        model_limits, remain_quota, unlimited_quota, max_concurrent,
                        usage_month, tokens_this_month, input_tokens, output_tokens
                 FROM keys_meta ORDER BY id",
                &[],
            )
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let model_limits_raw: String = row.get(6);
            let model_limits: Vec<String> =
                serde_json::from_str(&model_limits_raw).unwrap_or_default();
            out.push(PersistedKeyMetadata {
                id: row.get(0),
                token: row.get(1),
                name: row.get(2),
                rpm_limit: row.get::<_, i64>(3) as u64,
                monthly_token_limit: row.get::<_, i64>(4) as u64,
                expired_at: row.get::<_, Option<i64>>(5).map(|v| v as u64),
                model_limits,
                remain_quota: row.get::<_, i64>(7) as i64,
                unlimited_quota: row.get(8),
                max_concurrent: row.get::<_, i32>(9) as u32,
                usage_month: row.get(10),
                tokens_this_month: row.get::<_, i64>(11) as u64,
                input_tokens: row.get::<_, i64>(12) as u64,
                output_tokens: row.get::<_, i64>(13) as u64,
            });
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------
    // models CRUD
    // -----------------------------------------------------------------------

    pub async fn replace_models(
        &self,
        profile_id: &str,
        models: &[PersistedModel],
        synced_at: &str,
    ) -> Result<()> {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

        tx.execute("DELETE FROM models WHERE profile_id = $1", &[&profile_id])
            .await?;

        let stmt = tx
            .prepare_cached(
                "INSERT INTO models
                    (profile_id, model_id, owned_by, context_length,
                     input_price_per_mtok, output_price_per_mtok, available)
                 VALUES ($1,$2,$3,$4,$5,$6,$7)",
            )
            .await?;

        for m in models {
            tx.execute(
                &stmt,
                &[
                    &m.profile_id,
                    &m.id,
                    &m.owned_by,
                    &m.context_length.map(|v| v as i64),
                    &m.input_price_per_mtok,
                    &m.output_price_per_mtok,
                    &m.available,
                ],
            )
            .await?;
        }

        // Upsert sync timestamp.
        tx.execute(
            "INSERT INTO model_sync_state (profile_id, synced_at)
             VALUES ($1, $2)
             ON CONFLICT (profile_id) DO UPDATE SET synced_at = EXCLUDED.synced_at",
            &[&profile_id, &synced_at],
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn load_all_models(&self) -> Result<(Vec<PersistedModel>, HashMap<String, String>)> {
        let client = self.pool.get().await?;

        let model_rows = client
            .query(
                "SELECT profile_id, model_id, owned_by, context_length,
                        input_price_per_mtok, output_price_per_mtok, available
                 FROM models ORDER BY profile_id, model_id",
                &[],
            )
            .await?;

        let mut models = Vec::with_capacity(model_rows.len());
        for row in model_rows {
            models.push(PersistedModel {
                profile_id: row.get(0),
                id: row.get(1),
                owned_by: row.get(2),
                context_length: row.get::<_, Option<i64>>(3).map(|v| v as u64),
                input_price_per_mtok: row.get(4),
                output_price_per_mtok: row.get(5),
                available: row.get(6),
            });
        }

        let sync_rows = client
            .query("SELECT profile_id, synced_at FROM model_sync_state", &[])
            .await?;
        let mut synced_at_by_profile = HashMap::new();
        for row in sync_rows {
            synced_at_by_profile.insert(row.get(0), row.get(1));
        }

        Ok((models, synced_at_by_profile))
    }

    // -----------------------------------------------------------------------
    // domain_policies CRUD
    // -----------------------------------------------------------------------

    pub async fn replace_policies(&self, policies: &[PersistedDomainPolicy]) -> Result<()> {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

        tx.execute("DELETE FROM domain_policies", &[]).await?;

        let stmt = tx
            .prepare_cached(
                "INSERT INTO domain_policies
                    (domain, monthly_token_budget, monthly_cost_budget_usd,
                     min_hit_rate, enabled, pipeline, upstream_profile)
                 VALUES ($1,$2,$3,$4,$5,$6,$7)",
            )
            .await?;

        for p in policies {
            tx.execute(
                &stmt,
                &[
                    &p.domain,
                    &(p.monthly_token_budget as i64),
                    &p.monthly_cost_budget_usd,
                    &p.min_hit_rate,
                    &p.enabled,
                    &p.pipeline,
                    &p.upstream_profile,
                ],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn load_policies(&self) -> Result<Vec<PersistedDomainPolicy>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT domain, monthly_token_budget, monthly_cost_budget_usd,
                        min_hit_rate, enabled, pipeline, upstream_profile
                 FROM domain_policies ORDER BY domain",
                &[],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| PersistedDomainPolicy {
                domain: row.get(0),
                monthly_token_budget: row.get::<_, i64>(1) as u64,
                monthly_cost_budget_usd: row.get(2),
                min_hit_rate: row.get(3),
                enabled: row.get(4),
                pipeline: row.get(5),
                upstream_profile: row.get(6),
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // upstream_config CRUD (singleton row)
    // -----------------------------------------------------------------------

    pub async fn save_upstream(
        &self,
        base_url: &str,
        model: &str,
        endpoints: &[String],
        notes: Option<&str>,
        last_test: Option<&UpstreamTestResult>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        let endpoints_json = serde_json::to_string(endpoints).unwrap_or_default();
        let last_test_json = last_test
            .map(serde_json::to_string)
            .transpose()
            .unwrap_or_default();

        client
            .execute(
                "INSERT INTO upstream_config (singleton, base_url, model, endpoints, notes, last_test)
                 VALUES (true, $1, $2, $3::jsonb, $4, $5::jsonb)
                 ON CONFLICT (singleton) DO UPDATE SET
                    base_url = EXCLUDED.base_url,
                    model = EXCLUDED.model,
                    endpoints = EXCLUDED.endpoints,
                    notes = EXCLUDED.notes,
                    last_test = EXCLUDED.last_test",
                &[&base_url, &model, &endpoints_json, &notes, &last_test_json],
            )
            .await?;
        Ok(())
    }

    pub async fn load_upstream(
        &self,
    ) -> Result<(
        StoredUpstreamConfig,
        Option<String>,
        Option<UpstreamTestResult>,
    )> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT base_url, model, endpoints, notes, last_test
                 FROM upstream_config WHERE singleton = true",
                &[],
            )
            .await?;

        if rows.is_empty() {
            return Ok((StoredUpstreamConfig::default(), None, None));
        }

        let row = &rows[0];
        let endpoints_raw: String = row.get(2);
        let endpoints: Vec<String> = serde_json::from_str(&endpoints_raw).unwrap_or_default();
        let notes: Option<String> = row.get(3);
        let last_test_raw: Option<String> = row.get(4);
        let last_test: Option<UpstreamTestResult> =
            last_test_raw.and_then(|s| serde_json::from_str(&s).ok());

        Ok((
            StoredUpstreamConfig {
                base_url: row.get(0),
                model: row.get(1),
                api_key: String::new(), // secrets stored separately
                endpoints,
            },
            notes,
            last_test,
        ))
    }

    // -----------------------------------------------------------------------
    // upstream_pool_secrets CRUD
    // -----------------------------------------------------------------------

    pub async fn replace_pool_secrets(
        &self,
        secrets: &[PersistedUpstreamPoolSecret],
    ) -> Result<()> {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

        tx.execute("DELETE FROM upstream_pool_secrets", &[]).await?;

        let stmt = tx
            .prepare_cached(
                "INSERT INTO upstream_pool_secrets (id, secret, enabled)
                 VALUES ($1, $2, $3)",
            )
            .await?;

        for s in secrets {
            tx.execute(&stmt, &[&s.id, &s.secret, &s.enabled]).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn load_pool_secrets(&self) -> Result<Vec<PersistedUpstreamPoolSecret>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT id, secret, enabled FROM upstream_pool_secrets ORDER BY id",
                &[],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| PersistedUpstreamPoolSecret {
                id: row.get(0),
                secret: row.get(1),
                enabled: row.get(2),
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // upstream_profile_secrets CRUD
    // -----------------------------------------------------------------------

    pub async fn replace_profile_secrets(
        &self,
        profile_id: &str,
        secrets: &[PersistedUpstreamPoolSecret],
    ) -> Result<()> {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

        tx.execute(
            "DELETE FROM upstream_profile_secrets WHERE profile_id = $1",
            &[&profile_id],
        )
        .await?;

        let stmt = tx
            .prepare_cached(
                "INSERT INTO upstream_profile_secrets (profile_id, key_id, secret, enabled)
                 VALUES ($1, $2, $3, $4)",
            )
            .await?;

        for s in secrets {
            tx.execute(&stmt, &[&profile_id, &s.id, &s.secret, &s.enabled])
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn load_profile_secrets(
        &self,
    ) -> Result<HashMap<String, Vec<PersistedUpstreamPoolSecret>>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT profile_id, key_id, secret, enabled
                 FROM upstream_profile_secrets ORDER BY profile_id, key_id",
                &[],
            )
            .await?;

        let mut map: HashMap<String, Vec<PersistedUpstreamPoolSecret>> = HashMap::new();
        for row in rows {
            let pid: String = row.get(0);
            map.entry(pid)
                .or_default()
                .push(PersistedUpstreamPoolSecret {
                    id: row.get(1),
                    secret: row.get(2),
                    enabled: row.get(3),
                });
        }
        Ok(map)
    }

    // -----------------------------------------------------------------------
    // metrics_snapshots CRUD (replaces SQLite MetricsStore)
    // -----------------------------------------------------------------------

    pub async fn insert_metric_snapshot(
        &self,
        snapshot: &MetricsCounterSnapshot,
        gateway_uptime_secs: u64,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        let payload = serde_json::to_string(snapshot).unwrap_or_default();
        client
            .execute(
                "INSERT INTO metrics_snapshots (sampled_at, gateway_uptime_secs, payload)
                 VALUES ($1, $2, $3::jsonb)
                 ON CONFLICT (sampled_at) DO NOTHING",
                &[
                    &(snapshot.sampled_at as i64),
                    &(gateway_uptime_secs as i64),
                    &payload,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn load_metric_snapshots_since(
        &self,
        cutoff_ts: u64,
    ) -> Result<Vec<MetricsCounterSnapshot>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT payload FROM metrics_snapshots
                 WHERE sampled_at >= $1 ORDER BY sampled_at ASC",
                &[&(cutoff_ts as i64)],
            )
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: String = row.get(0);
            if let Ok(snap) = serde_json::from_str::<MetricsCounterSnapshot>(&payload) {
                out.push(snap);
            }
        }
        Ok(out)
    }

    pub async fn last_metric_gateway_uptime(&self) -> Result<Option<u64>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT gateway_uptime_secs FROM metrics_snapshots
                 ORDER BY sampled_at DESC LIMIT 1",
                &[],
            )
            .await?;

        Ok(rows.first().map(|row| row.get::<_, i64>(0) as u64))
    }

    pub async fn prune_metric_snapshots(&self, cutoff_ts: u64) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "DELETE FROM metrics_snapshots WHERE sampled_at < $1",
                &[&(cutoff_ts as i64)],
            )
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Full-state load (for startup hydration)
    // -----------------------------------------------------------------------

    /// Load all persisted entities into an `AdminStateFile` equivalent.
    /// Returns `None` if any query fails (caller should fall back to JSON).
    pub async fn load_full_state(&self) -> Option<AdminStateFile> {
        let keys_meta = self.load_all_keys().await.ok()?;
        let (models, synced_at_by_profile) = self.load_all_models().await.ok()?;
        let domain_policies = self.load_policies().await.ok()?;
        let (upstream_cfg, notes, last_test) = self.load_upstream().await.ok()?;
        let pool_secrets = self.load_pool_secrets().await.ok()?;
        let profile_secrets_map = self.load_profile_secrets().await.ok()?;

        let profile_secrets = crate::persist::PersistedProfileSecrets {
            by_profile: profile_secrets_map,
        };

        Some(AdminStateFile {
            version: 4,
            models: PersistedModels {
                models,
                synced_at_by_profile,
                synced_at: None,
            },
            upstream_notes: notes,
            last_upstream_test: last_test,
            upstream_snapshot: Some(PersistedUpstreamSnapshot {
                base_url: upstream_cfg.base_url,
                model: upstream_cfg.model,
                endpoints: upstream_cfg.endpoints,
            }),
            keys_meta,
            domain_policies,
            upstream_profile_secrets: profile_secrets,
            upstream_pool_secrets: pool_secrets,
        })
    }

    // -----------------------------------------------------------------------
    // JSON → PG migration
    // -----------------------------------------------------------------------

    /// Import data from a loaded `AdminStateFile` into PG.
    /// Only runs when PG tables are empty (checked via `keys_meta` count).
    pub async fn maybe_import_from_json(&self, json: &AdminStateFile) -> Result<bool> {
        let client = self.pool.get().await?;
        let count: i64 = client
            .query_one("SELECT COUNT(*) FROM keys_meta", &[])
            .await?
            .get(0);

        if count > 0 {
            info!("PG tables already populated; skipping JSON import");
            return Ok(false);
        }

        info!("Importing admin state from JSON into PostgreSQL...");

        // Import keys_meta
        for key in &json.keys_meta {
            if let Err(e) = self.upsert_key(key).await {
                warn!(error = %e, key_id = %key.id, "Failed to import key to PG");
            }
        }

        // Import models
        for (profile_id, synced_at) in &json.models.synced_at_by_profile {
            let profile_models: Vec<PersistedModel> = json
                .models
                .models
                .iter()
                .filter(|m| &m.profile_id == profile_id)
                .cloned()
                .collect();
            if let Err(e) = self
                .replace_models(profile_id, &profile_models, synced_at)
                .await
            {
                warn!(error = %e, profile_id = %profile_id, "Failed to import models to PG");
            }
        }

        // Import domain policies
        if let Err(e) = self.replace_policies(&json.domain_policies).await {
            warn!(error = %e, "Failed to import domain policies to PG");
        }

        // Import upstream config
        if let Some(ref snap) = json.upstream_snapshot {
            if let Err(e) = self
                .save_upstream(
                    &snap.base_url,
                    &snap.model,
                    &snap.endpoints,
                    json.upstream_notes.as_deref(),
                    json.last_upstream_test.as_ref(),
                )
                .await
            {
                warn!(error = %e, "Failed to import upstream config to PG");
            }
        }

        // Import pool secrets
        if let Err(e) = self.replace_pool_secrets(&json.upstream_pool_secrets).await {
            warn!(error = %e, "Failed to import pool secrets to PG");
        }

        // Import profile secrets
        for (profile_id, secrets) in &json.upstream_profile_secrets.by_profile {
            if let Err(e) = self.replace_profile_secrets(profile_id, secrets).await {
                warn!(
                    error = %e,
                    profile_id = %profile_id,
                    "Failed to import profile secrets to PG"
                );
            }
        }

        info!("JSON → PG migration complete");
        Ok(true)
    }

    /// Return the inner pool (for advanced usage / health checks).
    pub fn pool(&self) -> &Pool {
        &self.pool
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Redact password from connection URL for logging.
fn redact_url(url: &str) -> String {
    if let Some(at) = url.find('@') {
        if let Some(slash) = url[..at].rfind('/') {
            let prefix = &url[..slash + 2]; // "postgresql://"
            let suffix = &url[at..];
            return format!("{}****:****{}", prefix, suffix);
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_url() {
        let url = "postgresql://user:secret@localhost:5432/crabcache";
        let redacted = redact_url(url);
        assert!(redacted.contains("****"));
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn test_pg_config_from_env_disabled() {
        let cfg = PgConfig::from_env();
        let _ = cfg.enabled();
    }

    /// Helper: connect to a test PG database (requires TEST_PG_URL env var).
    async fn test_pg() -> Option<PgStore> {
        let url = std::env::var("TEST_PG_URL").ok()?;
        PgStore::new(&url, 4).await.ok()
    }

    #[tokio::test]
    async fn test_keys_meta_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        // Clean table first.
        let client = pg.pool.get().await.unwrap();
        let _ = client.execute("DELETE FROM keys_meta", &[]).await;

        let meta = PersistedKeyMetadata {
            id: "test-key-1".to_string(),
            token: "sk-cc-test123".to_string(),
            name: "test-consumer".to_string(),
            rpm_limit: 100,
            monthly_token_limit: 1_000_000,
            expired_at: None,
            model_limits: vec!["deepseek-v4-pro".to_string()],
            remain_quota: 500_000,
            unlimited_quota: false,
            max_concurrent: 10,
            usage_month: "2026-05".to_string(),
            tokens_this_month: 42_000,
            input_tokens: 30_000,
            output_tokens: 12_000,
        };
        pg.upsert_key(&meta).await.unwrap();

        let loaded = pg.load_all_keys().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "test-key-1");
        assert_eq!(loaded[0].name, "test-consumer");
        assert_eq!(loaded[0].rpm_limit, 100);
        assert_eq!(loaded[0].tokens_this_month, 42_000);

        // Update
        let mut updated = meta.clone();
        updated.name = "updated-consumer".to_string();
        updated.tokens_this_month = 50_000;
        pg.upsert_key(&updated).await.unwrap();

        let loaded = pg.load_all_keys().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "updated-consumer");
        assert_eq!(loaded[0].tokens_this_month, 50_000);

        // Delete
        pg.delete_key("test-key-1").await.unwrap();
        let loaded = pg.load_all_keys().await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn test_models_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        let client = pg.pool.get().await.unwrap();
        let _ = client.execute("DELETE FROM models", &[]).await;
        let _ = client.execute("DELETE FROM model_sync_state", &[]).await;

        let models = vec![
            PersistedModel {
                profile_id: "deepseek".to_string(),
                id: "deepseek-v4-pro".to_string(),
                owned_by: "deepseek".to_string(),
                context_length: Some(128_000),
                input_price_per_mtok: Some(0.27),
                output_price_per_mtok: Some(1.10),
                available: true,
            },
            PersistedModel {
                profile_id: "deepseek".to_string(),
                id: "deepseek-v4-flash".to_string(),
                owned_by: "deepseek".to_string(),
                context_length: Some(64_000),
                input_price_per_mtok: Some(0.10),
                output_price_per_mtok: Some(0.40),
                available: true,
            },
        ];

        pg.replace_models("deepseek", &models, "2026-05-26T00:00:00Z")
            .await
            .unwrap();

        let (loaded, sync_map) = pg.load_all_models().await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(sync_map.get("deepseek").unwrap(), "2026-05-26T00:00:00Z");

        // Replace with fewer models.
        let models_v2 = vec![PersistedModel {
            profile_id: "deepseek".to_string(),
            id: "deepseek-v4-pro".to_string(),
            owned_by: "deepseek".to_string(),
            context_length: Some(128_000),
            input_price_per_mtok: None,
            output_price_per_mtok: None,
            available: true,
        }];
        pg.replace_models("deepseek", &models_v2, "2026-05-26T01:00:00Z")
            .await
            .unwrap();

        let (loaded, _) = pg.load_all_models().await.unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn test_domain_policies_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        let client = pg.pool.get().await.unwrap();
        let _ = client.execute("DELETE FROM domain_policies", &[]).await;

        let policies = vec![PersistedDomainPolicy {
            domain: "example.com".to_string(),
            monthly_token_budget: 10_000_000,
            monthly_cost_budget_usd: 50.0,
            min_hit_rate: 0.8,
            enabled: true,
            pipeline: Some("cursor_deepseek_v4".to_string()),
            upstream_profile: Some("deepseek".to_string()),
        }];

        pg.replace_policies(&policies).await.unwrap();
        let loaded = pg.load_policies().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].domain, "example.com");
        assert_eq!(loaded[0].monthly_token_budget, 10_000_000);
    }

    #[tokio::test]
    async fn test_upstream_config_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        let client = pg.pool.get().await.unwrap();
        let _ = client.execute("DELETE FROM upstream_config", &[]).await;

        let endpoints = vec!["api.deepseek.com:443".to_string()];
        pg.save_upstream(
            "https://api.deepseek.com",
            "deepseek-v4-pro",
            &endpoints,
            Some("test notes"),
            None,
        )
        .await
        .unwrap();

        let (cfg, notes, _test) = pg.load_upstream().await.unwrap();
        assert_eq!(cfg.base_url, "https://api.deepseek.com");
        assert_eq!(cfg.model, "deepseek-v4-pro");
        assert_eq!(notes.as_deref(), Some("test notes"));
    }

    #[tokio::test]
    async fn test_pool_secrets_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        let client = pg.pool.get().await.unwrap();
        let _ = client
            .execute("DELETE FROM upstream_pool_secrets", &[])
            .await;

        let secrets = vec![
            PersistedUpstreamPoolSecret {
                id: "key-1".to_string(),
                secret: "sk-ds-test123".to_string(),
                enabled: true,
            },
            PersistedUpstreamPoolSecret {
                id: "key-2".to_string(),
                secret: "sk-ds-test456".to_string(),
                enabled: false,
            },
        ];

        pg.replace_pool_secrets(&secrets).await.unwrap();
        let loaded = pg.load_pool_secrets().await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|s| s.id == "key-1" && s.enabled));
        assert!(loaded.iter().any(|s| s.id == "key-2" && !s.enabled));
    }

    #[tokio::test]
    async fn test_profile_secrets_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        let client = pg.pool.get().await.unwrap();
        let _ = client
            .execute("DELETE FROM upstream_profile_secrets", &[])
            .await;

        let secrets = vec![PersistedUpstreamPoolSecret {
            id: "pk-1".to_string(),
            secret: "sk-openai-test".to_string(),
            enabled: true,
        }];

        pg.replace_profile_secrets("openai", &secrets)
            .await
            .unwrap();
        let loaded = pg.load_profile_secrets().await.unwrap();
        assert!(loaded.contains_key("openai"));
        assert_eq!(loaded["openai"].len(), 1);
        assert_eq!(loaded["openai"][0].id, "pk-1");
    }

    #[tokio::test]
    async fn test_full_state_roundtrip() {
        let Some(pg) = test_pg().await else { return };
        // Clean all tables.
        let client = pg.pool.get().await.unwrap();
        for table in &[
            "keys_meta",
            "models",
            "model_sync_state",
            "domain_policies",
            "upstream_config",
            "upstream_pool_secrets",
            "upstream_profile_secrets",
        ] {
            let _ = client
                .execute(format!("DELETE FROM {}", table).as_str(), &[])
                .await;
        }

        // Build a test state.
        let state = AdminStateFile {
            version: 4,
            keys_meta: vec![PersistedKeyMetadata {
                id: "k1".to_string(),
                token: "sk-cc-k1".to_string(),
                name: "consumer-1".to_string(),
                rpm_limit: 50,
                monthly_token_limit: 500_000,
                expired_at: None,
                model_limits: vec![],
                remain_quota: 0,
                unlimited_quota: true,
                max_concurrent: 5,
                usage_month: "2026-05".to_string(),
                tokens_this_month: 1000,
                input_tokens: 800,
                output_tokens: 200,
            }],
            models: PersistedModels {
                models: vec![PersistedModel {
                    profile_id: "deepseek".to_string(),
                    id: "deepseek-v4-pro".to_string(),
                    owned_by: "deepseek".to_string(),
                    context_length: Some(128_000),
                    input_price_per_mtok: None,
                    output_price_per_mtok: None,
                    available: true,
                }],
                synced_at_by_profile: [("deepseek".to_string(), "2026-05-26".to_string())]
                    .into_iter()
                    .collect(),
                synced_at: None,
            },
            domain_policies: vec![PersistedDomainPolicy {
                domain: "test.local".to_string(),
                monthly_token_budget: 1_000_000,
                monthly_cost_budget_usd: 10.0,
                min_hit_rate: 0.5,
                enabled: true,
                pipeline: None,
                upstream_profile: None,
            }],
            upstream_snapshot: Some(PersistedUpstreamSnapshot {
                base_url: "https://api.deepseek.com".to_string(),
                model: "deepseek-v4-pro".to_string(),
                endpoints: vec!["api.deepseek.com:443".to_string()],
            }),
            upstream_notes: Some("test notes".to_string()),
            last_upstream_test: None,
            upstream_pool_secrets: vec![PersistedUpstreamPoolSecret {
                id: "ps-1".to_string(),
                secret: "sk-ds-pool1".to_string(),
                enabled: true,
            }],
            upstream_profile_secrets: crate::persist::PersistedProfileSecrets {
                by_profile: [(
                    "openai".to_string(),
                    vec![PersistedUpstreamPoolSecret {
                        id: "oai-1".to_string(),
                        secret: "sk-oai-1".to_string(),
                        enabled: true,
                    }],
                )]
                .into_iter()
                .collect(),
            },
        };

        // Import.
        let migrated = pg.maybe_import_from_json(&state).await.unwrap();
        assert!(migrated);

        // Load back.
        let loaded = pg.load_full_state().await.unwrap();
        assert_eq!(loaded.keys_meta.len(), 1);
        assert_eq!(loaded.keys_meta[0].id, "k1");
        assert_eq!(loaded.models.models.len(), 1);
        assert_eq!(loaded.domain_policies.len(), 1);
        assert_eq!(loaded.upstream_pool_secrets.len(), 1);
        assert!(
            loaded
                .upstream_profile_secrets
                .by_profile
                .contains_key("openai")
        );

        // Second import should skip (tables already populated).
        let migrated2 = pg.maybe_import_from_json(&state).await.unwrap();
        assert!(!migrated2);
    }
}
