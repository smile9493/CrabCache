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
use crate::state::{StoredRequestLog, StoredUpstreamConfig};
use crate::trace_log::TraceLogEntry;
use crate::types::UpstreamTestResult;
use anyhow::{Context, Result};
use deadpool_postgres::{Config as PoolConfig, Pool, Runtime};
use std::collections::HashMap;
use std::time::Duration;
use tokio_postgres::NoTls;
use tracing::info;

/// Aggregated trace analysis results from SQL queries.
#[derive(Debug, Clone)]
pub struct TraceAnalysisResult {
    pub total_requests: i64,
    pub cache_hits: i64,
    pub avg_latency_ms: f64,
    pub total_input_tokens: u64,
    pub avg_tokens: f64,
    pub unique_requests: i64,
    pub model_distribution: Vec<(String, i64)>,
}

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
// Helpers
// ---------------------------------------------------------------------------

/// Saturating cast: `u64` → `i64` (clamps at `i64::MAX` instead of wrapping).
#[inline]
fn to_pg_bigint(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Safe cast: `i64` → `u64` (negative values map to 0).
#[inline]
fn from_pg_bigint(v: i64) -> u64 {
    v.max(0) as u64
}

const TRACE_LOGS_SELECT: &str = "SELECT request_hash, timestamp_ms, content_length, semantic_cluster,
                    model, prompt_tokens, latency_ms, cache_hit,
                    conversation_id, consumer, domain, project_id,
                    upstream_latency_ms, ttft_ms, input_tokens, output_tokens,
                    cache_tier, composition,
                    request_messages_snapshot, response_preview,
                    retired_prefix_messages, reasoning_strategy,
                    prompt_cache_hit_ratio, upstream_profile_id, pipeline,
                    upstream_model, client_body_user_id, upstream_user_id,
                    user_id_audit, upstream_key_id,
                    streaming_defer, streaming_defer_reject_reason,
                    session_store, stable_session_kind, upstream_outbound_bytes,
                    prefill_ms, pre_header_ms,
                    affinity_key, affinity_kind, backend_name,
                    session_fingerprint, is_coalesced, client_key_id";

fn trace_log_entry_from_row(row: &tokio_postgres::Row) -> TraceLogEntry {
    let composition_raw: Option<String> = row.get(17);
    let composition = composition_raw.and_then(|s| serde_json::from_str(&s).ok());
    TraceLogEntry {
        request_hash: row.get(0),
        timestamp_ms: from_pg_bigint(row.get(1)),
        content_length: row.get::<_, i32>(2) as usize,
        semantic_cluster: row.get::<_, i32>(3) as u32,
        model: row.get(4),
        prompt_tokens: row.get::<_, i32>(5) as usize,
        latency_ms: row.get(6),
        cache_hit: row.get(7),
        conversation_id: row.get(8),
        consumer: row.get(9),
        domain: row.get(10),
        project_id: row.get(11),
        upstream_latency_ms: row.get(12),
        prefill_ms: row.get(35),
        pre_header_ms: row.get(36),
        ttft_ms: row.get(13),
        input_tokens: row.get::<_, Option<i64>>(14).map(from_pg_bigint),
        output_tokens: row.get::<_, Option<i64>>(15).map(from_pg_bigint),
        cache_tier: row.get(16),
        composition,
        request_messages_snapshot: row.get(18),
        response_preview: row.get(19),
        retired_prefix_messages: row.get::<_, Option<i32>>(20).map(|v| v as usize),
        reasoning_strategy: row.get(21),
        prompt_cache_hit_ratio: row.get(22),
        upstream_profile_id: row.get(23),
        pipeline: row.get(24),
        upstream_model: row.get(25),
        client_body_user_id: row.get(26),
        upstream_user_id: row.get(27),
        user_id_audit: row.get(28),
        upstream_key_id: row.get(29),
        affinity_key: row.get(37),
        affinity_kind: row.get(38),
        backend_name: row.get(39),
        session_fingerprint: row.get(40),
        is_coalesced: row.get(41),
        client_key_id: row.get(42),
        streaming_defer: row.get(30),
        streaming_defer_reject_reason: row.get(31),
        session_store: row.get(32),
        stable_session_kind: row.get(33),
        upstream_outbound_bytes: row.get::<_, Option<i32>>(34).map(|v| v as usize),
    }
}

fn append_trace_numeric_filters(
    sql: &mut String,
    params: &mut Vec<Box<dyn tokio_postgres::types::ToSql + Send + Sync>>,
    idx: &mut usize,
    latency_min: Option<f64>,
    latency_max: Option<f64>,
    token_min: Option<u64>,
    token_max: Option<u64>,
) {
    if let Some(v) = latency_min {
        sql.push_str(&format!(" AND latency_ms >= ${idx}"));
        params.push(Box::new(v));
        *idx += 1;
    }
    if let Some(v) = latency_max {
        sql.push_str(&format!(" AND latency_ms <= ${idx}"));
        params.push(Box::new(v));
        *idx += 1;
    }
    if let Some(v) = token_min {
        sql.push_str(&format!(
            " AND (COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) >= ${idx}"
        ));
        params.push(Box::new(to_pg_bigint(v)));
        *idx += 1;
    }
    if let Some(v) = token_max {
        sql.push_str(&format!(
            " AND (COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) <= ${idx}"
        ));
        params.push(Box::new(to_pg_bigint(v)));
        *idx += 1;
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
        cfg.connect_timeout = Some(Duration::from_secs(5));
        cfg.pool = Some(deadpool_postgres::PoolConfig {
            max_size: max_pool_size,
            timeouts: deadpool_postgres::Timeouts {
                wait: Some(Duration::from_secs(10)),
                create: Some(Duration::from_secs(5)),
                recycle: Some(Duration::from_secs(5)),
            },
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

    /// Return the row count for a given table (used by metrics gauges).
    /// Table name is validated against a whitelist to prevent SQL injection.
    pub async fn table_row_count(&self, table: &str) -> Result<i64> {
        const ALLOWED: &[&str] = &["trace_logs", "request_logs", "audit_log"];
        if !ALLOWED.contains(&table) {
            anyhow::bail!("invalid table name: {}", table);
        }
        let client = self.pool.get().await?;
        let q = format!("SELECT count(*) FROM {}", table);
        let row = client.query_one(q.as_str(), &[]).await?;
        Ok(row.get(0))
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

        // Note: sampled_at is PRIMARY KEY so an implicit index already exists.

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS trace_logs (
                    request_hash    TEXT NOT NULL,
                    timestamp_ms    BIGINT NOT NULL,
                    content_length  INTEGER NOT NULL,
                    semantic_cluster INTEGER NOT NULL,
                    model           TEXT NOT NULL,
                    prompt_tokens   INTEGER NOT NULL,
                    latency_ms      DOUBLE PRECISION NOT NULL,
                    cache_hit       BOOLEAN NOT NULL,
                    conversation_id TEXT,
                    consumer        TEXT,
                    domain          TEXT,
                    project_id      TEXT,
                    upstream_latency_ms DOUBLE PRECISION,
                    ttft_ms         DOUBLE PRECISION,
                    input_tokens    BIGINT,
                    output_tokens   BIGINT,
                    cache_tier      TEXT,
                    composition     JSONB,
                    request_messages_snapshot TEXT,
                    response_preview TEXT,
                    retired_prefix_messages INTEGER,
                    reasoning_strategy TEXT,
                    prompt_cache_hit_ratio DOUBLE PRECISION,
                    upstream_profile_id TEXT,
                    pipeline        TEXT,
                    upstream_model  TEXT,
                    client_body_user_id TEXT,
                    upstream_user_id TEXT,
                    user_id_audit   TEXT,
                    upstream_key_id TEXT,
                    streaming_defer BOOLEAN NOT NULL DEFAULT false,
                    streaming_defer_reject_reason TEXT,
                    session_store   TEXT,
                    stable_session_kind TEXT,
                    upstream_outbound_bytes INTEGER,
                    prefill_ms      DOUBLE PRECISION,
                    pre_header_ms   DOUBLE PRECISION,
                    affinity_key    TEXT,
                    affinity_kind   TEXT,
                    backend_name    TEXT,
                    session_fingerprint TEXT,
                    is_coalesced    BOOLEAN NOT NULL DEFAULT false,
                    client_key_id   TEXT,
                    PRIMARY KEY (request_hash, timestamp_ms)
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_trace_ts
                 ON trace_logs (timestamp_ms DESC)",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_trace_consumer_ts
                 ON trace_logs (consumer, timestamp_ms DESC)
                 WHERE consumer IS NOT NULL",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_trace_model_ts
                 ON trace_logs (model, timestamp_ms DESC)",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_trace_cache_tier
                 ON trace_logs (cache_tier, timestamp_ms DESC)
                 WHERE cache_tier IS NOT NULL",
                &[],
            )
            .await?;

        for stmt in [
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS streaming_defer BOOLEAN NOT NULL DEFAULT false",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS streaming_defer_reject_reason TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS session_store TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS stable_session_kind TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS upstream_outbound_bytes INTEGER",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS prefill_ms DOUBLE PRECISION",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS pre_header_ms DOUBLE PRECISION",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS affinity_key TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS affinity_kind TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS backend_name TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS session_fingerprint TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS is_coalesced BOOLEAN NOT NULL DEFAULT false",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS client_key_id TEXT",
        ] {
            client.execute(stmt, &[]).await?;
        }

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

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS request_logs (
                    id              TEXT PRIMARY KEY,
                    timestamp_ms    BIGINT NOT NULL,
                    model           TEXT NOT NULL DEFAULT '',
                    consumer        TEXT NOT NULL DEFAULT '',
                    duration_ms     DOUBLE PRECISION NOT NULL DEFAULT 0,
                    input_tokens    BIGINT NOT NULL DEFAULT 0,
                    output_tokens   BIGINT NOT NULL DEFAULT 0,
                    cache_status    TEXT NOT NULL DEFAULT '',
                    cache_tier      TEXT NOT NULL DEFAULT '',
                    status_code     INTEGER NOT NULL DEFAULT 200,
                    conversation_id TEXT NOT NULL DEFAULT '',
                    route_backend   TEXT NOT NULL DEFAULT '',
                    request_payload JSONB,
                    response_body   TEXT,
                    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_req_logs_ts
                 ON request_logs (timestamp_ms DESC)",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_req_logs_consumer
                 ON request_logs (consumer, timestamp_ms DESC)
                 WHERE consumer != ''",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS schema_migration (
                    version     INTEGER PRIMARY KEY,
                    migrated_at TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        // Phase 1.3: domain_usage persistence table.
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS domain_usage (
                    domain      TEXT NOT NULL,
                    month       TEXT NOT NULL,
                    tokens      BIGINT NOT NULL DEFAULT 0,
                    spend_usd   DOUBLE PRECISION NOT NULL DEFAULT 0,
                    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
                    PRIMARY KEY (domain, month)
                )",
                &[],
            )
            .await?;

        // Phase 2.1: consumer_usage_monthly pre-aggregation table.
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS consumer_usage_monthly (
                    consumer        TEXT NOT NULL,
                    month           TEXT NOT NULL,
                    input_tokens    BIGINT NOT NULL DEFAULT 0,
                    output_tokens   BIGINT NOT NULL DEFAULT 0,
                    total_tokens    BIGINT NOT NULL DEFAULT 0,
                    cost_usd        DOUBLE PRECISION NOT NULL DEFAULT 0,
                    request_count   BIGINT NOT NULL DEFAULT 0,
                    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
                    PRIMARY KEY (consumer, month)
                )",
                &[],
            )
            .await?;

        // Phase 2.2: audit_log table.
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS audit_log (
                    id          BIGSERIAL PRIMARY KEY,
                    timestamp   TIMESTAMPTZ NOT NULL DEFAULT now(),
                    action      TEXT NOT NULL,
                    actor       TEXT NOT NULL,
                    target      TEXT,
                    detail      JSONB,
                    ip_address  TEXT
                )",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_ts ON audit_log (timestamp DESC)",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log (action, timestamp DESC)",
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

        let model_limits_json =
            serde_json::to_string(&meta.model_limits).context("serialize model_limits")?;
        client
            .execute(
                &stmt,
                &[
                    &meta.id,
                    &meta.token,
                    &meta.name,
                    &to_pg_bigint(meta.rpm_limit),
                    &to_pg_bigint(meta.monthly_token_limit),
                    &meta.expired_at.map(to_pg_bigint),
                    &model_limits_json,
                    &meta.remain_quota,
                    &meta.unlimited_quota,
                    &(meta.max_concurrent as i32),
                    &meta.usage_month,
                    &to_pg_bigint(meta.tokens_this_month),
                    &to_pg_bigint(meta.input_tokens),
                    &to_pg_bigint(meta.output_tokens),
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
                rpm_limit: from_pg_bigint(row.get(3)),
                monthly_token_limit: from_pg_bigint(row.get(4)),
                expired_at: row.get::<_, Option<i64>>(5).map(from_pg_bigint),
                model_limits,
                remain_quota: row.get(7),
                unlimited_quota: row.get(8),
                max_concurrent: row.get::<_, i32>(9) as u32,
                usage_month: row.get(10),
                tokens_this_month: from_pg_bigint(row.get(11)),
                input_tokens: from_pg_bigint(row.get(12)),
                output_tokens: from_pg_bigint(row.get(13)),
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
                    &m.context_length.map(to_pg_bigint),
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
                context_length: row.get::<_, Option<i64>>(3).map(from_pg_bigint),
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
                    &to_pg_bigint(p.monthly_token_budget),
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
                monthly_token_budget: from_pg_bigint(row.get(1)),
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
        let endpoints_json = serde_json::to_string(endpoints).context("serialize endpoints")?;
        let last_test_json = last_test
            .map(serde_json::to_string)
            .transpose()
            .context("serialize last_test")?;

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
        let payload = serde_json::to_string(snapshot).context("serialize metrics snapshot")?;
        client
            .execute(
                "INSERT INTO metrics_snapshots (sampled_at, gateway_uptime_secs, payload)
                 VALUES ($1, $2, $3::jsonb)
                 ON CONFLICT (sampled_at) DO NOTHING",
                &[
                    &to_pg_bigint(snapshot.sampled_at),
                    &to_pg_bigint(gateway_uptime_secs),
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
                &[&(to_pg_bigint(cutoff_ts))],
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

        Ok(rows.first().map(|row| from_pg_bigint(row.get(0))))
    }

    pub async fn prune_metric_snapshots(&self, cutoff_ts: u64) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "DELETE FROM metrics_snapshots WHERE sampled_at < $1",
                &[&(to_pg_bigint(cutoff_ts))],
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
    /// Uses a `schema_migration` table to track whether the import has been done.
    /// Returns `Ok(true)` if import succeeded, `Ok(false)` if skipped (already done).
    pub async fn maybe_import_from_json(&self, json: &AdminStateFile) -> Result<bool> {
        let client = self.pool.get().await?;

        // Check if migration already completed.
        let migrated: bool = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM schema_migration WHERE version = 1)",
                &[],
            )
            .await?
            .get(0);

        if migrated {
            info!("JSON → PG migration already completed; skipping");
            return Ok(false);
        }

        info!("Importing admin state from JSON into PostgreSQL...");
        let mut failures: u32 = 0;

        // Import keys_meta
        for key in &json.keys_meta {
            if let Err(e) = self.upsert_key(key).await {
                tracing::error!(error = %e, key_id = %key.id, "Failed to import key to PG");
                failures += 1;
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
                tracing::error!(error = %e, profile_id = %profile_id, "Failed to import models to PG");
                failures += 1;
            }
        }

        // Import domain policies
        if let Err(e) = self.replace_policies(&json.domain_policies).await {
            tracing::error!(error = %e, "Failed to import domain policies to PG");
            failures += 1;
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
                tracing::error!(error = %e, "Failed to import upstream config to PG");
                failures += 1;
            }
        }

        // Import pool secrets
        if let Err(e) = self.replace_pool_secrets(&json.upstream_pool_secrets).await {
            tracing::error!(error = %e, "Failed to import pool secrets to PG");
            failures += 1;
        }

        // Import profile secrets
        for (profile_id, secrets) in &json.upstream_profile_secrets.by_profile {
            if let Err(e) = self.replace_profile_secrets(profile_id, secrets).await {
                tracing::error!(
                    error = %e,
                    profile_id = %profile_id,
                    "Failed to import profile secrets to PG"
                );
                failures += 1;
            }
        }

        if failures > 0 {
            tracing::error!(
                failures,
                "JSON → PG migration completed with errors; PG state may be incomplete"
            );
            // Still mark as migrated to avoid infinite retry on partial data.
            // The failed entities can be re-synced via the dual-write path.
        }

        // Mark migration as complete.
        client
            .execute(
                "INSERT INTO schema_migration (version) VALUES (1) ON CONFLICT DO NOTHING",
                &[],
            )
            .await?;

        info!(failures, "JSON → PG migration complete");
        Ok(failures == 0)
    }

    /// Return the inner pool (for advanced usage / health checks).
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    // -----------------------------------------------------------------------
    // trace_logs CRUD
    // -----------------------------------------------------------------------

    /// Batch-insert trace log entries. Uses a single transaction for atomicity.
    pub async fn insert_trace_logs(&self, entries: &[TraceLogEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;
        let stmt = tx
            .prepare_cached(
                "INSERT INTO trace_logs
                    (request_hash, timestamp_ms, content_length, semantic_cluster,
                     model, prompt_tokens, latency_ms, cache_hit,
                     conversation_id, consumer, domain, project_id,
                     upstream_latency_ms, ttft_ms, input_tokens, output_tokens,
                     cache_tier, composition,
                     request_messages_snapshot, response_preview,
                     retired_prefix_messages, reasoning_strategy,
                     prompt_cache_hit_ratio, upstream_profile_id, pipeline,
                     upstream_model, client_body_user_id, upstream_user_id,
                     user_id_audit, upstream_key_id,
                     streaming_defer, streaming_defer_reject_reason,
                     session_store, stable_session_kind, upstream_outbound_bytes,
                     prefill_ms, pre_header_ms,
                     affinity_key, affinity_kind, backend_name,
                     session_fingerprint, is_coalesced, client_key_id)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,
                         $15,$16,$17,$18::jsonb,$19,$20,$21,$22,$23,$24,$25,
                         $26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,
                         $39,$40,$41,$42,$43)
                 ON CONFLICT (request_hash, timestamp_ms) DO NOTHING",
            )
            .await?;

        for e in entries {
            let composition_json = e
                .composition
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .context("serialize composition")?;
            tx.execute(
                &stmt,
                &[
                    &e.request_hash,
                    &to_pg_bigint(e.timestamp_ms),
                    &(e.content_length as i32),
                    &(e.semantic_cluster as i32),
                    &e.model,
                    &(e.prompt_tokens as i32),
                    &e.latency_ms,
                    &e.cache_hit,
                    &e.conversation_id,
                    &e.consumer,
                    &e.domain,
                    &e.project_id,
                    &e.upstream_latency_ms,
                    &e.ttft_ms,
                    &e.input_tokens.map(to_pg_bigint),
                    &e.output_tokens.map(to_pg_bigint),
                    &e.cache_tier,
                    &composition_json,
                    &e.request_messages_snapshot,
                    &e.response_preview,
                    &e.retired_prefix_messages.map(|v| v as i32),
                    &e.reasoning_strategy,
                    &e.prompt_cache_hit_ratio,
                    &e.upstream_profile_id,
                    &e.pipeline,
                    &e.upstream_model,
                    &e.client_body_user_id,
                    &e.upstream_user_id,
                    &e.user_id_audit,
                    &e.upstream_key_id,
                    &e.streaming_defer,
                    &e.streaming_defer_reject_reason,
                    &e.session_store,
                    &e.stable_session_kind,
                    &e.upstream_outbound_bytes.map(|v| v as i32),
                    &e.prefill_ms,
                    &e.pre_header_ms,
                    &e.affinity_key,
                    &e.affinity_kind,
                    &e.backend_name,
                    &e.session_fingerprint,
                    &e.is_coalesced,
                    &e.client_key_id,
                ],
            )
            .await?;
        }

        tx.commit().await?;
        crab_metrics::global_metrics().inc_admin_log_write("trace");
        Ok(())
    }

    /// Load trace log entries with filters, sort, and limit.
    pub async fn load_trace_logs(
        &self,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        consumer: Option<&str>,
        model: Option<&str>,
        cache_tier: Option<&str>,
        request_hash: Option<&str>,
        limit: usize,
    ) -> Result<Vec<TraceLogEntry>> {
        let client = self.pool.get().await?;
        let mut sql = String::from(TRACE_LOGS_SELECT);
        sql.push_str(" FROM trace_logs WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Send + Sync>> = Vec::new();
        let mut idx = 1;

        if let Some(from) = from_ms {
            sql.push_str(&format!(" AND timestamp_ms >= ${idx}"));
            params.push(Box::new(to_pg_bigint(from)));
            idx += 1;
        }
        if let Some(to) = to_ms {
            sql.push_str(&format!(" AND timestamp_ms <= ${idx}"));
            params.push(Box::new(to_pg_bigint(to)));
            idx += 1;
        }
        if let Some(c) = consumer {
            sql.push_str(&format!(" AND consumer = ${idx}"));
            params.push(Box::new(c.to_string()));
            idx += 1;
        }
        if let Some(m) = model {
            sql.push_str(&format!(" AND model = ${idx}"));
            params.push(Box::new(m.to_string()));
            idx += 1;
        }
        if let Some(ct) = cache_tier {
            sql.push_str(&format!(" AND cache_tier = ${idx}"));
            params.push(Box::new(ct.to_string()));
            idx += 1;
        }
        if let Some(rh) = request_hash {
            sql.push_str(&format!(" AND request_hash = ${idx}"));
            params.push(Box::new(rh.to_string()));
            idx += 1;
        }

        sql.push_str(" ORDER BY timestamp_ms DESC");
        sql.push_str(&format!(" LIMIT ${idx}"));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
            .iter()
            .map(|p| p.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect();
        let rows = client.query(&sql, &param_refs[..]).await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(trace_log_entry_from_row(&row));
        }
        Ok(out)
    }

    /// Paginated trace log query with cursor support.
    /// Returns (entries, next_cursor) where next_cursor is `None` if no more pages.
    pub async fn query_trace_logs_paginated(
        &self,
        cursor: Option<&str>,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        consumer: Option<&str>,
        model: Option<&str>,
        cache_tier: Option<&str>,
        request_hash: Option<&str>,
        latency_min: Option<f64>,
        latency_max: Option<f64>,
        token_min: Option<u64>,
        token_max: Option<u64>,
        limit: usize,
    ) -> Result<(Vec<TraceLogEntry>, Option<String>)> {
        // Parse cursor: "timestamp_ms:request_hash"
        let (cursor_ts, cursor_hash) = if let Some(c) = cursor {
            let parts: Vec<&str> = c.splitn(2, ':').collect();
            if parts.len() == 2 {
                let ts: Option<u64> = parts[0].parse().ok();
                if ts.is_none() {
                    tracing::warn!(cursor = %c, "malformed cursor timestamp, ignoring cursor");
                }
                (ts, Some(parts[1].to_string()))
            } else {
                tracing::warn!(cursor = %c, "malformed cursor format, ignoring cursor");
                (None, None)
            }
        } else {
            (None, None)
        };

        let client = self.pool.get().await?;
        let mut sql = String::from(TRACE_LOGS_SELECT);
        sql.push_str(" FROM trace_logs WHERE 1=1");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Send + Sync>> = Vec::new();
        let mut idx = 1;

        // Cursor-based keyset pagination: (timestamp_ms, request_hash) < cursor
        if let Some(ts) = cursor_ts {
            if let Some(ref rh) = cursor_hash {
                let next_idx = idx + 1;
                sql.push_str(&format!(
                    " AND (timestamp_ms < ${idx} OR (timestamp_ms = ${idx} AND request_hash < ${next_idx}))",
                ));
                params.push(Box::new(to_pg_bigint(ts)));
                params.push(Box::new(rh.clone()));
                idx += 2;
            }
        }

        if let Some(from) = from_ms {
            sql.push_str(&format!(" AND timestamp_ms >= ${idx}"));
            params.push(Box::new(to_pg_bigint(from)));
            idx += 1;
        }
        if let Some(to) = to_ms {
            sql.push_str(&format!(" AND timestamp_ms <= ${idx}"));
            params.push(Box::new(to_pg_bigint(to)));
            idx += 1;
        }
        if let Some(c) = consumer {
            sql.push_str(&format!(" AND consumer = ${idx}"));
            params.push(Box::new(c.to_string()));
            idx += 1;
        }
        if let Some(m) = model {
            sql.push_str(&format!(" AND model = ${idx}"));
            params.push(Box::new(m.to_string()));
            idx += 1;
        }
        if let Some(ct) = cache_tier {
            sql.push_str(&format!(" AND cache_tier = ${idx}"));
            params.push(Box::new(ct.to_string()));
            idx += 1;
        }
        if let Some(rh) = request_hash {
            sql.push_str(&format!(" AND request_hash = ${idx}"));
            params.push(Box::new(rh.to_string()));
            idx += 1;
        }

        append_trace_numeric_filters(
            &mut sql,
            &mut params,
            &mut idx,
            latency_min,
            latency_max,
            token_min,
            token_max,
        );

        sql.push_str(" ORDER BY timestamp_ms DESC, request_hash DESC");
        // Fetch limit+1 to detect has_more.
        sql.push_str(&format!(" LIMIT ${idx}"));
        params.push(Box::new((limit + 1) as i64));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
            .iter()
            .map(|p| p.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect();
        let rows = client.query(&sql, &param_refs[..]).await?;

        let has_more = rows.len() > limit;

        // Build cursor from the (limit+1)-th row BEFORE consuming rows.
        let next_cursor = if has_more {
            let tail = &rows[limit];
            Some(format!(
                "{}:{}",
                from_pg_bigint(tail.get(1)),
                tail.get::<_, String>(0),
            ))
        } else {
            None
        };

        let entries: Vec<TraceLogEntry> = rows
            .into_iter()
            .take(limit)
            .map(|row| trace_log_entry_from_row(&row))
            .collect();

        Ok((entries, next_cursor))
    }

    /// Find a single trace log entry by composite key (request_hash, timestamp_ms).
    pub async fn find_trace_log(
        &self,
        request_hash: &str,
        timestamp_ms: u64,
    ) -> Result<Option<TraceLogEntry>> {
        let mut results = self
            .load_trace_logs(
                Some(timestamp_ms),
                Some(timestamp_ms),
                None,
                None,
                None,
                Some(request_hash),
                1,
            )
            .await?;
        Ok(results.pop())
    }

    /// Find the most recent trace log entry for a bare request_hash (legacy list IDs).
    pub async fn find_trace_log_by_hash(&self, request_hash: &str) -> Result<Option<TraceLogEntry>> {
        let client = self.pool.get().await?;
        let sql = format!(
            "{TRACE_LOGS_SELECT} FROM trace_logs WHERE request_hash = $1 \
             ORDER BY timestamp_ms DESC LIMIT 1"
        );
        let rows = client.query(&sql, &[&request_hash]).await?;
        Ok(rows.first().map(trace_log_entry_from_row))
    }

    /// Delete trace logs older than the given timestamp.
    pub async fn prune_trace_logs(&self, cutoff_ms: u64) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM trace_logs WHERE timestamp_ms < $1",
                &[&to_pg_bigint(cutoff_ms)],
            )
            .await?;
        Ok(count)
    }

    /// Count total requests and cache hits in a time window (for trace summary).
    pub async fn trace_log_summary(&self, from_ms: u64) -> Result<(i64, i64)> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*), COUNT(*) FILTER (WHERE cache_hit)
                 FROM trace_logs WHERE timestamp_ms >= $1",
                &[&to_pg_bigint(from_ms)],
            )
            .await?;
        Ok((row.get(0), row.get(1)))
    }

    /// Aggregate trace stats for analysis (within a time window).
    pub async fn trace_log_analysis(&self, from_ms: u64) -> Result<TraceAnalysisResult> {
        let client = self.pool.get().await?;

        let row = client
            .query_one(
                "SELECT
                    COUNT(*),
                    COUNT(*) FILTER (WHERE cache_hit),
                    COALESCE(SUM(latency_ms), 0),
                    COALESCE(SUM(COALESCE(input_tokens, prompt_tokens::bigint)), 0)
                 FROM trace_logs WHERE timestamp_ms >= $1",
                &[&to_pg_bigint(from_ms)],
            )
            .await?;

        let total: i64 = row.get(0);
        let cache_hits: i64 = row.get(1);
        let total_latency: f64 = row.get(2);
        let total_tokens: i64 = row.get(3);

        // Model distribution (top 5)
        let model_rows = client
            .query(
                "SELECT model, COUNT(*) FROM trace_logs
                 WHERE timestamp_ms >= $1
                 GROUP BY model ORDER BY COUNT(*) DESC LIMIT 5",
                &[&to_pg_bigint(from_ms)],
            )
            .await?;
        let model_distribution: Vec<(String, i64)> = model_rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();

        // Unique requests (for repeat ratio)
        let unique_row = client
            .query_one(
                "SELECT COUNT(DISTINCT request_hash) FROM trace_logs
                 WHERE timestamp_ms >= $1",
                &[&to_pg_bigint(from_ms)],
            )
            .await?;
        let unique_requests: i64 = unique_row.get(0);

        Ok(TraceAnalysisResult {
            total_requests: total,
            cache_hits,
            avg_latency_ms: if total > 0 {
                total_latency / total as f64
            } else {
                0.0
            },
            total_input_tokens: total_tokens as u64,
            avg_tokens: if total > 0 {
                total_tokens as f64 / total as f64
            } else {
                0.0
            },
            unique_requests,
            model_distribution,
        })
    }

    /// List distinct consumers in a time window (for live-metrics consumer list).
    pub async fn trace_log_consumers(&self, from_ms: u64) -> Result<Vec<String>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT DISTINCT consumer FROM trace_logs
                 WHERE timestamp_ms >= $1
                   AND consumer IS NOT NULL AND consumer != ''
                 ORDER BY consumer",
                &[&to_pg_bigint(from_ms)],
            )
            .await?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    // -----------------------------------------------------------------------
    // request_logs CRUD
    // -----------------------------------------------------------------------

    /// Batch-insert request log entries.
    pub async fn insert_request_logs(&self, logs: &[StoredRequestLog]) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;
        let stmt = tx
            .prepare_cached(
                "INSERT INTO request_logs
                    (id, timestamp_ms, model, consumer, duration_ms,
                     input_tokens, output_tokens, cache_status, cache_tier,
                     status_code, conversation_id, route_backend,
                     request_payload, response_body)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::jsonb,$14)
                 ON CONFLICT (id) DO NOTHING",
            )
            .await?;

        for log in logs {
            let payload_json =
                serde_json::to_string(&log.request_payload).context("serialize request_payload")?;
            tx.execute(
                &stmt,
                &[
                    &log.id,
                    &to_pg_bigint(log.timestamp),
                    &log.model,
                    &log.consumer,
                    &log.duration_ms,
                    &to_pg_bigint(log.input_tokens),
                    &to_pg_bigint(log.output_tokens),
                    &log.cache_status,
                    &log.cache_tier,
                    &(log.status_code as i32),
                    &log.conversation_id,
                    &log.route_backend,
                    &payload_json,
                    &log.response_body,
                ],
            )
            .await?;
        }

        tx.commit().await?;
        crab_metrics::global_metrics().inc_admin_log_write("request");
        Ok(())
    }

    /// Load request logs with optional filters.
    pub async fn load_request_logs(
        &self,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
        consumer: Option<&str>,
        model: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredRequestLog>> {
        let client = self.pool.get().await?;
        let mut sql = String::from(
            "SELECT id, timestamp_ms, model, consumer, duration_ms,
                    input_tokens, output_tokens, cache_status, cache_tier,
                    status_code, conversation_id, route_backend,
                    request_payload, response_body
             FROM request_logs WHERE 1=1",
        );
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Send + Sync>> = Vec::new();
        let mut idx = 1;

        if let Some(from) = from_ms {
            sql.push_str(&format!(" AND timestamp_ms >= ${idx}"));
            params.push(Box::new(to_pg_bigint(from)));
            idx += 1;
        }
        if let Some(to) = to_ms {
            sql.push_str(&format!(" AND timestamp_ms <= ${idx}"));
            params.push(Box::new(to_pg_bigint(to)));
            idx += 1;
        }
        if let Some(c) = consumer {
            sql.push_str(&format!(" AND consumer = ${idx}"));
            params.push(Box::new(c.to_string()));
            idx += 1;
        }
        if let Some(m) = model {
            sql.push_str(&format!(" AND model = ${idx}"));
            params.push(Box::new(m.to_string()));
            idx += 1;
        }

        sql.push_str(" ORDER BY timestamp_ms DESC");
        sql.push_str(&format!(" LIMIT ${idx}"));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = params
            .iter()
            .map(|p| p.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect();
        let rows = client.query(&sql, &param_refs[..]).await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let payload_raw: Option<String> = row.get(12);
            let request_payload: serde_json::Value = payload_raw
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(serde_json::Value::Null);
            out.push(StoredRequestLog {
                id: row.get(0),
                timestamp: from_pg_bigint(row.get(1)),
                model: row.get(2),
                consumer: row.get(3),
                duration_ms: row.get(4),
                input_tokens: from_pg_bigint(row.get(5)),
                output_tokens: from_pg_bigint(row.get(6)),
                cache_status: row.get(7),
                cache_tier: row.get(8),
                status_code: row.get::<_, i32>(9) as u16,
                conversation_id: row.get(10),
                request_payload,
                response_body: row.get::<_, Option<String>>(13).unwrap_or_default(),
                cache_path: Vec::new(),
                route_backend: row.get(11),
            });
        }
        Ok(out)
    }

    /// Prune request logs older than the given timestamp.
    pub async fn prune_request_logs(&self, cutoff_ms: u64) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM request_logs WHERE timestamp_ms < $1",
                &[&to_pg_bigint(cutoff_ms)],
            )
            .await?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // domain_usage CRUD
    // -----------------------------------------------------------------------

    /// Upsert domain monthly usage counters.
    pub async fn upsert_domain_usage(
        &self,
        domain: &str,
        month: &str,
        tokens: u64,
        spend_usd: f64,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO domain_usage (domain, month, tokens, spend_usd, updated_at)
                 VALUES ($1, $2, $3, $4, now())
                 ON CONFLICT (domain, month) DO UPDATE SET
                    tokens = EXCLUDED.tokens,
                    spend_usd = EXCLUDED.spend_usd,
                    updated_at = now()",
                &[&domain, &month, &to_pg_bigint(tokens), &spend_usd],
            )
            .await?;
        Ok(())
    }

    /// Load all domain usage for a given month.
    pub async fn load_domain_usage(&self, month: &str) -> Result<Vec<(String, u64, f64)>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT domain, tokens, spend_usd FROM domain_usage WHERE month = $1",
                &[&month],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<_, String>(0),
                    from_pg_bigint(r.get::<_, i64>(1)),
                    r.get::<_, f64>(2),
                )
            })
            .collect())
    }

    /// Aggregate consumer usage from trace_logs for a given month and upsert into consumer_usage_monthly.
    pub async fn aggregate_consumer_usage(&self, month: &str) -> Result<u64> {
        let client = self.pool.get().await?;
        // Compute month boundaries in milliseconds.
        let month_start = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
            .and_then(|d| {
                let dt = d.and_hms_opt(0, 0, 0).unwrap();
                Ok(dt.and_utc().timestamp_millis() as u64)
            })
            .map_err(|e| anyhow::anyhow!("parse month: {e}"))?;
        let next_month = {
            let parts: Vec<&str> = month.split('-').collect();
            let y: i32 = parts[0].parse().unwrap_or(2026);
            let m: u32 = parts[1].parse().unwrap_or(1);
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            format!("{ny:04}-{nm:02}")
        };
        let month_end = chrono::NaiveDate::parse_from_str(&format!("{next_month}-01"), "%Y-%m-%d")
            .and_then(|d| {
                let dt = d.and_hms_opt(0, 0, 0).unwrap();
                Ok(dt.and_utc().timestamp_millis() as u64)
            })
            .map_err(|e| anyhow::anyhow!("parse next month: {e}"))?;

        let count = client
            .execute(
                "INSERT INTO consumer_usage_monthly
                    (consumer, month, input_tokens, output_tokens, total_tokens, cost_usd, request_count, updated_at)
                 SELECT
                    COALESCE(consumer, 'unclassified') AS consumer,
                    $3 AS month,
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(input_tokens) + SUM(output_tokens), 0),
                    0.0,
                    COUNT(*),
                    now()
                 FROM trace_logs
                 WHERE timestamp_ms >= $1 AND timestamp_ms < $2
                   AND consumer IS NOT NULL AND consumer != ''
                 GROUP BY consumer
                 ON CONFLICT (consumer, month) DO UPDATE SET
                    input_tokens = EXCLUDED.input_tokens,
                    output_tokens = EXCLUDED.output_tokens,
                    total_tokens = EXCLUDED.total_tokens,
                    request_count = EXCLUDED.request_count,
                    updated_at = now()",
                &[
                    &(month_start as i64),
                    &(month_end as i64),
                    &month,
                ],
            )
            .await?;
        Ok(count)
    }

    /// Delete domain usage older than the given month.
    pub async fn prune_domain_usage(&self, older_than_month: &str) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM domain_usage WHERE month < $1",
                &[&older_than_month],
            )
            .await?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // consumer_usage_monthly CRUD
    // -----------------------------------------------------------------------

    /// Upsert consumer monthly usage summary.
    pub async fn upsert_consumer_usage(
        &self,
        consumer: &str,
        month: &str,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        cost_usd: f64,
        request_count: u64,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO consumer_usage_monthly
                    (consumer, month, input_tokens, output_tokens, total_tokens, cost_usd, request_count, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, now())
                 ON CONFLICT (consumer, month) DO UPDATE SET
                    input_tokens = EXCLUDED.input_tokens,
                    output_tokens = EXCLUDED.output_tokens,
                    total_tokens = EXCLUDED.total_tokens,
                    cost_usd = EXCLUDED.cost_usd,
                    request_count = EXCLUDED.request_count,
                    updated_at = now()",
                &[
                    &consumer,
                    &month,
                    &to_pg_bigint(input_tokens),
                    &to_pg_bigint(output_tokens),
                    &to_pg_bigint(total_tokens),
                    &cost_usd,
                    &to_pg_bigint(request_count),
                ],
            )
            .await?;
        Ok(())
    }

    /// Load all consumer usage for a given month.
    pub async fn load_consumer_usage(
        &self,
        month: &str,
    ) -> Result<Vec<(String, u64, u64, u64, f64, u64)>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT consumer, input_tokens, output_tokens, total_tokens, cost_usd, request_count
                 FROM consumer_usage_monthly WHERE month = $1",
                &[&month],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<_, String>(0),
                    from_pg_bigint(r.get::<_, i64>(1)),
                    from_pg_bigint(r.get::<_, i64>(2)),
                    from_pg_bigint(r.get::<_, i64>(3)),
                    r.get::<_, f64>(4),
                    from_pg_bigint(r.get::<_, i64>(5)),
                )
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // audit_log CRUD
    // -----------------------------------------------------------------------

    /// Insert an audit log entry.
    pub async fn insert_audit_log(
        &self,
        action: &str,
        actor: &str,
        target: Option<&str>,
        detail: Option<&serde_json::Value>,
        ip_address: Option<&str>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        let detail_json = detail.map(|v| serde_json::to_string(v).unwrap_or_default());
        client
            .execute(
                "INSERT INTO audit_log (action, actor, target, detail, ip_address)
                 VALUES ($1, $2, $3, $4::jsonb, $5)",
                &[
                    &action,
                    &actor,
                    &target,
                    &detail_json.as_deref(),
                    &ip_address,
                ],
            )
            .await?;
        crab_metrics::global_metrics().inc_admin_log_write("audit");
        Ok(())
    }

    /// Load audit log entries with pagination and optional action filter.
    /// Returns (id, timestamp_iso, action, actor, target, detail_text, ip_address).
    pub async fn load_audit_logs(
        &self,
        limit: i64,
        offset: i64,
        action_filter: Option<&str>,
    ) -> Result<
        Vec<(
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        )>,
    > {
        let client = self.pool.get().await?;
        let rows = if let Some(action) = action_filter {
            client
                .query(
                    "SELECT id, timestamp::text, action, actor, target, detail::text, ip_address
                     FROM audit_log WHERE action = $1
                     ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                    &[&action, &limit, &offset],
                )
                .await?
        } else {
            client
                .query(
                    "SELECT id, timestamp::text, action, actor, target, detail::text, ip_address
                     FROM audit_log
                     ORDER BY timestamp DESC LIMIT $1 OFFSET $2",
                    &[&limit, &offset],
                )
                .await?
        };
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<_, i64>(0),
                    r.get::<_, String>(1),
                    r.get::<_, String>(2),
                    r.get::<_, String>(3),
                    r.get::<_, Option<String>>(4),
                    r.get::<_, Option<String>>(5),
                    r.get::<_, Option<String>>(6),
                )
            })
            .collect())
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
    fn append_trace_numeric_filters_builds_latency_and_token_clauses() {
        let mut sql = String::from("SELECT 1 WHERE true");
        let mut params: Vec<Box<dyn tokio_postgres::types::ToSql + Send + Sync>> = Vec::new();
        let mut idx = 1;
        append_trace_numeric_filters(
            &mut sql,
            &mut params,
            &mut idx,
            Some(10.0),
            Some(100.0),
            Some(50),
            Some(500),
        );
        assert!(sql.contains("latency_ms >= $1"));
        assert!(sql.contains("latency_ms <= $2"));
        assert!(sql.contains(
            "COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) >= $3"
        ));
        assert!(sql.contains(
            "COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) <= $4"
        ));
        assert_eq!(params.len(), 4);
        assert_eq!(idx, 5);
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
