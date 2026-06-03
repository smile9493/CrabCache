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
use tokio_postgres::types::Json;
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

const TRACE_LOGS_SELECT: &str =
    "SELECT request_hash, timestamp_ms, content_length, semantic_cluster,
                    model, prompt_tokens, latency_ms, cache_hit,
                    conversation_id, consumer, domain, project_id,
                    upstream_latency_ms, ttft_ms, input_tokens, output_tokens,
                    cache_tier, composition,
                    request_messages_snapshot, response_preview,
                    retired_prefix_messages, reasoning_strategy,
                    prompt_cache_hit_ratio, upstream_profile_id, pipeline,
                    upstream_model, client_body_user_id, upstream_user_id,
                    user_id_audit, upstream_key_id,
                    session_store, stable_session_kind, upstream_outbound_bytes,
                    prefill_ms, pre_header_ms,
                    affinity_key, affinity_kind, backend_name,
                    session_fingerprint, is_coalesced, client_key_id,
                    request_passthrough, request_passthrough_prefix_len,
                    status_code, error_code, limit_source, cache_decision,
                    upstream_result, phase_durations_ms, client_ip,
                    client_kind";

fn trace_log_entry_from_row(row: &tokio_postgres::Row) -> TraceLogEntry {
    // composition is stored as jsonb in PG — read as serde_json::Value then deserialize.
    let composition: Option<crab_composition::RequestComposition> = row
        .get::<_, Option<serde_json::Value>>(17)
        .and_then(|v| serde_json::from_value(v).ok());
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
        prefill_ms: row.get(33),
        pre_header_ms: row.get(34),
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
        affinity_key: row.get(35),
        affinity_kind: row.get(36),
        backend_name: row.get(37),
        session_fingerprint: row.get(38),
        is_coalesced: row.get(39),
        client_key_id: row.get(40),
        session_store: row.get(30),
        stable_session_kind: row.get(31),
        upstream_outbound_bytes: row.get::<_, Option<i32>>(32).map(|v| v as usize),
        request_passthrough: row.get(41),
        request_passthrough_prefix_len: row.get::<_, Option<i32>>(42).map(|v| v as usize),
        status_code: row.get::<_, Option<i32>>(43).map(|v| v as u16),
        error_code: row.get(44),
        limit_source: row.get(45),
        cache_decision: row.get(46),
        upstream_result: row.get(47),
        phase_durations_ms: row
            .get::<_, Option<serde_json::Value>>(48)
            .and_then(|v| serde_json::from_value(v).ok()),
        client_ip: row.get(49),
        client_kind: row.get(50),
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
                    account_id TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (profile_id, key_id)
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "ALTER TABLE upstream_profile_secrets
                 ADD COLUMN IF NOT EXISTS account_id TEXT NOT NULL DEFAULT ''",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS upstream_profile_configs (
                    profile_id            TEXT PRIMARY KEY,
                    provider              TEXT NOT NULL,
                    base_url              TEXT NOT NULL,
                    fallback_model        TEXT NOT NULL,
                    endpoints             JSONB NOT NULL DEFAULT '[]',
                    tls_sni               TEXT,
                    proxy_url             TEXT,
                    fallback_profile_id   TEXT,
                    fallback_max_retries  INTEGER
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS oauth_credentials (
                    id          TEXT PRIMARY KEY,
                    provider    TEXT NOT NULL,
                    profile_id  TEXT NOT NULL DEFAULT 'codex',
                    payload     JSONB NOT NULL,
                    updated_at  BIGINT NOT NULL DEFAULT 0
                )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_oauth_credentials_profile
                 ON oauth_credentials (profile_id)",
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
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS request_passthrough BOOLEAN NOT NULL DEFAULT false",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS request_passthrough_prefix_len INTEGER",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS status_code INTEGER",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS error_code TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS limit_source TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS cache_decision TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS upstream_result TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS phase_durations_ms JSONB",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS client_ip TEXT",
            "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS client_kind TEXT",
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

        // Phase: model peak hours aggregation table.
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS model_peak_hours (
                    model         TEXT NOT NULL,
                    hour_bucket   BIGINT NOT NULL,
                    request_count BIGINT NOT NULL DEFAULT 0,
                    input_tokens  BIGINT NOT NULL DEFAULT 0,
                    output_tokens BIGINT NOT NULL DEFAULT 0,
                    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
                    PRIMARY KEY (model, hour_bucket)
                )",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_peak_hours_model ON model_peak_hours (model, hour_bucket DESC)",
                &[],
            )
            .await?;

        // Phase: system_config KV table for singleton configuration persistence.
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS system_config (
                    key         TEXT PRIMARY KEY,
                    value       JSONB NOT NULL,
                    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        // Gateway control-plane snapshots (P1: disaster recovery).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS gateway_state_snapshots (
                    snapshot_id     BIGSERIAL PRIMARY KEY,
                    snapshot_type   TEXT NOT NULL DEFAULT 'full',
                    keys_json       JSONB,
                    runtime_json    JSONB,
                    profiles_json   JSONB,
                    key_states_json JSONB,
                    domain_policies_json JSONB,
                    version         BIGINT NOT NULL DEFAULT 0,
                    source          TEXT NOT NULL DEFAULT 'admin_pull',
                    snapshot_at     TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_gw_snapshots_at
                 ON gateway_state_snapshots (snapshot_at DESC)",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_gw_snapshots_type
                 ON gateway_state_snapshots (snapshot_type, snapshot_at DESC)",
                &[],
            )
            .await?;

        // Codex OAuth sessions (P2: persist in-flight OAuth flows).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS codex_oauth_sessions (
                    session_id      UUID PRIMARY KEY,
                    session_type    TEXT NOT NULL,
                    profile_id      TEXT NOT NULL,
                    status          TEXT NOT NULL DEFAULT 'pending',
                    credential_id   TEXT,
                    email           TEXT,
                    account_id      TEXT,
                    session_json    JSONB NOT NULL,
                    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
                    expires_at      TIMESTAMPTZ NOT NULL
                )",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_codex_oauth_status
                 ON codex_oauth_sessions (status, created_at DESC)",
                &[],
            )
            .await?;

        // Health probes (P2: availability SLA tracking).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS health_probes (
                    probe_id        BIGSERIAL PRIMARY KEY,
                    gateway_ready   BOOLEAN NOT NULL,
                    redis_status    TEXT,
                    l2_status       TEXT,
                    uptime_secs     BIGINT,
                    active_keys     INTEGER,
                    probe_at        TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;
        client
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_health_probes_at
                 ON health_probes (probe_at DESC)",
                &[],
            )
            .await?;

        // Alert rules (P2: configurable alerting).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS alert_rules (
                    rule_id         TEXT PRIMARY KEY,
                    rule_type       TEXT NOT NULL,
                    condition_json  JSONB NOT NULL,
                    enabled         BOOLEAN NOT NULL DEFAULT true,
                    notify_channels JSONB,
                    last_triggered  TIMESTAMPTZ,
                    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
                    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        // Webhook subscriptions (P3: persistent webhooks).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS webhook_subscriptions (
                    webhook_id      TEXT PRIMARY KEY,
                    url             TEXT NOT NULL,
                    events          JSONB NOT NULL,
                    secret_hash     TEXT,
                    enabled         BOOLEAN NOT NULL DEFAULT true,
                    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
                    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        // Dashboard preferences (P3: multi-device sync).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS dashboard_preferences (
                    user_key        TEXT PRIMARY KEY,
                    preferences     JSONB NOT NULL,
                    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
                &[],
            )
            .await?;

        // Backup metadata (P3: backup tracking).
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS backup_metadata (
                    backup_id       BIGSERIAL PRIMARY KEY,
                    backup_type     TEXT NOT NULL,
                    file_path       TEXT,
                    file_size_bytes BIGINT,
                    started_at      TIMESTAMPTZ NOT NULL,
                    completed_at    TIMESTAMPTZ,
                    status          TEXT NOT NULL DEFAULT 'running'
                )",
                &[],
            )
            .await?;

        // Migrate trace_logs to range-partitioned table by timestamp_ms (daily partitions).
        drop(client); // release connection before calling self methods
        self.migrate_trace_logs_to_partitioned().await?;

        // Ensure enhanced columns exist on existing tables (idempotent ALTER ADD COLUMN).
        self.ensure_enhanced_columns().await?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // trace_logs partitioning
    // -----------------------------------------------------------------------

    /// Partition name for a given UTC date.
    fn partition_name(year: i32, month: u32, day: u32) -> String {
        format!("trace_logs_y{:04}m{:02}d{:02}", year, month, day)
    }

    /// Compute the `[start_ms, end_ms)` range for a UTC date.
    fn partition_range_ms(year: i32, month: u32, day: u32) -> (i64, i64) {
        use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
        let start = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(year, month, day).unwrap(),
            NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        );
        let end = start + chrono::Duration::days(1);
        (
            start.and_utc().timestamp_millis(),
            end.and_utc().timestamp_millis(),
        )
    }

    /// Check if `trace_logs` is already a partitioned table.
    async fn is_trace_logs_partitioned(&self) -> Result<bool> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                "SELECT relkind FROM pg_class
                 WHERE relname = 'trace_logs'
                   AND relkind = 'p'",
                &[],
            )
            .await?;
        Ok(row.is_some())
    }

    /// Create a daily partition for `trace_logs` covering the given UTC date.
    /// No-op if the partition already exists.
    pub async fn create_trace_partition(&self, year: i32, month: u32, day: u32) -> Result<String> {
        let name = Self::partition_name(year, month, day);
        let (start_ms, end_ms) = Self::partition_range_ms(year, month, day);
        let client = self.pool.get().await?;
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {name}
             PARTITION OF trace_logs
             FOR VALUES FROM ({start_ms}) TO ({end_ms})"
        );
        client.execute(sql.as_str(), &[]).await?;
        Ok(name)
    }

    /// Drop a daily partition. Returns `true` if it existed and was dropped.
    pub async fn drop_trace_partition(&self, year: i32, month: u32, day: u32) -> Result<bool> {
        let name = Self::partition_name(year, month, day);
        let client = self.pool.get().await?;
        let sql = format!("DROP TABLE IF EXISTS {name}");
        client.execute(sql.as_str(), &[]).await?;
        // IF EXISTS always succeeds; query pg_class to see if it was actually there.
        // Return true unconditionally — caller uses it for logging.
        Ok(true)
    }

    /// Migrate `trace_logs` from a regular table to a range-partitioned table.
    ///
    /// Steps (only if `trace_logs` is not already partitioned):
    /// 1. Rename existing table to `trace_logs_legacy`
    /// 2. Create the partitioned parent table
    /// 3. Create partitions for existing data range + future 3 days
    /// 4. Copy data from legacy table
    /// 5. Drop legacy table
    pub async fn migrate_trace_logs_to_partitioned(&self) -> Result<()> {
        if self.is_trace_logs_partitioned().await? {
            info!("trace_logs is already partitioned, skipping migration");
            return Ok(());
        }

        let client = self.pool.get().await?;

        // Check if trace_logs exists at all (it might be a fresh DB).
        let exists = client
            .query_opt(
                "SELECT 1 FROM pg_class WHERE relname = 'trace_logs'",
                &[],
            )
            .await?;
        if exists.is_none() {
            info!("trace_logs does not exist yet, creating partitioned table directly");
            drop(client);
            return self.create_partitioned_trace_logs_table().await;
        }

        info!("Migrating trace_logs to range-partitioned table...");

        // Step 1: Rename
        client
            .execute("ALTER TABLE trace_logs RENAME TO trace_logs_legacy", &[])
            .await?;

        // Step 2: Create partitioned parent (drop client first since we need self methods)
        drop(client);
        self.create_partitioned_trace_logs_table().await?;

        // Step 3: Discover data range from legacy table and create partitions
        let client = self.pool.get().await?;
        let range_row = client
            .query_opt(
                "SELECT MIN(timestamp_ms), MAX(timestamp_ms) FROM trace_logs_legacy",
                &[],
            )
            .await?;

        if let Some(row) = range_row {
            let min_ts: Option<i64> = row.get(0);
            let max_ts: Option<i64> = row.get(1);

            if let (Some(min_ts), Some(max_ts)) = (min_ts, max_ts) {
                use chrono::{Datelike, TimeZone, Utc};
                let min_date = Utc
                    .timestamp_millis_opt(min_ts)
                    .single()
                    .map(|dt| dt.date_naive());
                let max_date = Utc
                    .timestamp_millis_opt(max_ts)
                    .single()
                    .map(|dt| dt.date_naive());

                if let (Some(min_date), Some(max_date)) = (min_date, max_date) {
                    // Create partitions for each day in range + 3 days into the future
                    let end_date = max_date + chrono::Duration::days(3);
                    let mut current = min_date;
                    drop(client);
                    while current <= end_date {
                        self.create_trace_partition(
                            current.year(),
                            current.month(),
                            current.day(),
                        )
                        .await?;
                        current += chrono::Duration::days(1);
                    }
                    info!(
                        min = %min_date,
                        max = %max_date,
                        "Created daily partitions for existing data range"
                    );
                }
            }
        } else {
            drop(client);
        }

        // Also ensure a default partition for any data outside named partitions
        let client = self.pool.get().await?;
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS trace_logs_default
                 PARTITION OF trace_logs DEFAULT",
                &[],
            )
            .await?;

        // Step 4: Copy data
        let rows = client
            .execute(
                "INSERT INTO trace_logs SELECT * FROM trace_logs_legacy",
                &[],
            )
            .await?;
        info!(rows, "Copied data from trace_logs_legacy to partitioned table");

        // Step 5: Drop legacy
        client
            .execute("DROP TABLE trace_logs_legacy", &[])
            .await?;
        info!("Migration complete: trace_logs is now range-partitioned by timestamp_ms");

        Ok(())
    }

    /// Create the partitioned parent `trace_logs` table (must not exist yet).
    async fn create_partitioned_trace_logs_table(&self) -> Result<()> {
        let client = self.pool.get().await?;
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
                    request_passthrough BOOLEAN NOT NULL DEFAULT false,
                    request_passthrough_prefix_len INTEGER,
                    status_code     INTEGER,
                    error_code      TEXT,
                    limit_source    TEXT,
                    cache_decision  TEXT,
                    upstream_result TEXT,
                    phase_durations_ms JSONB,
                    client_ip       TEXT,
                    client_kind     TEXT,
                    PRIMARY KEY (request_hash, timestamp_ms)
                ) PARTITION BY RANGE (timestamp_ms)",
                &[],
            )
            .await?;

        // Create indexes (they propagate to partitions automatically)
        for stmt in [
            "CREATE INDEX IF NOT EXISTS idx_trace_ts ON trace_logs (timestamp_ms DESC)",
            "CREATE INDEX IF NOT EXISTS idx_trace_consumer_ts ON trace_logs (consumer, timestamp_ms DESC) WHERE consumer IS NOT NULL",
            "CREATE INDEX IF NOT EXISTS idx_trace_model_ts ON trace_logs (model, timestamp_ms DESC)",
            "CREATE INDEX IF NOT EXISTS idx_trace_cache_tier ON trace_logs (cache_tier, timestamp_ms DESC) WHERE cache_tier IS NOT NULL",
        ] {
            client.execute(stmt, &[]).await?;
        }

        Ok(())
    }

    /// Ensure partitions exist for today and the next N days.
    pub async fn ensure_future_trace_partitions(&self, days_ahead: u32) -> Result<()> {
        use chrono::{Datelike, Utc};
        let today = Utc::now().date_naive();
        for offset in 0..=days_ahead {
            let date = today + chrono::Duration::days(i64::from(offset));
            self.create_trace_partition(date.year(), date.month(), date.day())
                .await?;
        }
        Ok(())
    }

    /// Drop trace partitions older than the given number of days.
    /// Returns the number of partitions dropped.
    pub async fn drop_old_trace_partitions(&self, retention_days: u32) -> Result<u64> {
        use chrono::Utc;
        let cutoff = Utc::now().date_naive() - chrono::Duration::days(i64::from(retention_days));
        let client = self.pool.get().await?;

        // Find all partition names that match trace_logs_y* pattern
        let rows = client
            .query(
                "SELECT inhrelid::regclass::text
                 FROM pg_inherits
                 WHERE inhparent = 'trace_logs'::regclass
                   AND inhrelid::regclass::text ~ '^trace_logs_y\\d{4}m\\d{2}d\\d{2}$'",
                &[],
            )
            .await?;

        let mut dropped: u64 = 0;
        for row in rows {
            let part_name: String = row.get(0);
            // Parse date from name: trace_logs_y2026m06d01
            if let Some(date_str) = part_name.strip_prefix("trace_logs_y") {
                // date_str = "2026m06d01"
                if date_str.len() == 10 {
                    if let Ok(date) = chrono::NaiveDate::parse_from_str(
                        &format!("{}-{}-{}", &date_str[0..4], &date_str[5..7], &date_str[8..10]),
                        "%Y-%m-%d",
                    ) {
                        if date < cutoff {
                            let sql = format!("DROP TABLE IF EXISTS {part_name}");
                            client.execute(sql.as_str(), &[]).await?;
                            info!(partition = %part_name, "Dropped old trace partition");
                            dropped += 1;
                        }
                    }
                }
            }
        }

        Ok(dropped)
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
                     usage_month, tokens_this_month, input_tokens, output_tokens,
                     dashboard_created)
                 VALUES ($1,$2,$3,$4,$5,$6,$7::jsonb,$8,$9,$10,$11,$12,$13,$14,$15)
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
                    dashboard_created = EXCLUDED.dashboard_created,
                    updated_at = now()",
            )
            .await?;

        let model_limits_json =
            serde_json::to_value(&meta.model_limits).context("serialize model_limits")?;
        let model_limits_pg = Json(&model_limits_json);
        let expired_at: Option<i64> = meta.expired_at.map(to_pg_bigint);
        let rpm_limit = to_pg_bigint(meta.rpm_limit);
        let monthly_token_limit = to_pg_bigint(meta.monthly_token_limit);
        let max_concurrent = meta.max_concurrent as i32;
        let tokens_this_month = to_pg_bigint(meta.tokens_this_month);
        let input_tokens = to_pg_bigint(meta.input_tokens);
        let output_tokens = to_pg_bigint(meta.output_tokens);
        tracing::debug!(
            key_id = %meta.id,
            rpm_limit,
            monthly_token_limit,
            ?expired_at,
            model_limits_json = %model_limits_json,
            remain_quota = meta.remain_quota,
            max_concurrent,
            "upsert_key params"
        );
        client
            .execute(
                &stmt,
                &[
                    &meta.id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &meta.token as &(dyn tokio_postgres::types::ToSql + Sync),
                    &meta.name as &(dyn tokio_postgres::types::ToSql + Sync),
                    &rpm_limit as &(dyn tokio_postgres::types::ToSql + Sync),
                    &monthly_token_limit as &(dyn tokio_postgres::types::ToSql + Sync),
                    &expired_at as &(dyn tokio_postgres::types::ToSql + Sync),
                    &model_limits_pg as &(dyn tokio_postgres::types::ToSql + Sync),
                    &meta.remain_quota as &(dyn tokio_postgres::types::ToSql + Sync),
                    &meta.unlimited_quota as &(dyn tokio_postgres::types::ToSql + Sync),
                    &max_concurrent as &(dyn tokio_postgres::types::ToSql + Sync),
                    &meta.usage_month as &(dyn tokio_postgres::types::ToSql + Sync),
                    &tokens_this_month as &(dyn tokio_postgres::types::ToSql + Sync),
                    &input_tokens as &(dyn tokio_postgres::types::ToSql + Sync),
                    &output_tokens as &(dyn tokio_postgres::types::ToSql + Sync),
                    &meta.dashboard_created as &(dyn tokio_postgres::types::ToSql + Sync),
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

    /// Remove duplicate rows that share the same non-empty token, keeping the newest.
    pub async fn dedupe_keys_meta_by_token(&self) -> Result<usize> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "WITH ranked AS (
                    SELECT id,
                           ROW_NUMBER() OVER (
                               PARTITION BY token
                               ORDER BY updated_at DESC NULLS LAST, id DESC
                           ) AS rn
                    FROM keys_meta
                    WHERE token <> ''
                 )
                 DELETE FROM keys_meta k
                 USING ranked r
                 WHERE k.id = r.id AND r.rn > 1
                 RETURNING k.id",
                &[],
            )
            .await?;
        Ok(rows.len())
    }

    /// Keys with a non-empty token (authoritative for reconcile drift checks).
    pub async fn count_keys_meta_with_token(&self) -> Result<i64> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "SELECT COUNT(*)::bigint FROM keys_meta
                 WHERE token IS NOT NULL AND btrim(token) <> ''",
                &[],
            )
            .await?;
        Ok(row.get(0))
    }

    pub async fn load_all_keys(&self) -> Result<Vec<PersistedKeyMetadata>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT id, token, name, rpm_limit, monthly_token_limit, expired_at,
                        model_limits, remain_quota, unlimited_quota, max_concurrent,
                        usage_month, tokens_this_month, input_tokens, output_tokens,
                        dashboard_created
                 FROM keys_meta ORDER BY id",
                &[],
            )
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let model_limits: Vec<String> =
                serde_json::from_value(row.get(6)).unwrap_or_default();
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
                dashboard_created: row.get(14),
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
                account_ids: Vec::new(),
                key_ids: Vec::new(),
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
        let endpoints_json = serde_json::to_value(endpoints).context("serialize endpoints")?;
        let last_test_json = last_test
            .map(serde_json::to_value)
            .transpose()
            .context("serialize last_test")?;
        let notes_owned: Option<String> = notes.map(|s| s.to_string());
        let endpoints_pg = Json(&endpoints_json);
        let last_test_pg = last_test_json.as_ref().map(Json);
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![
            &base_url,
            &model,
            &endpoints_pg,
            &notes_owned,
            &last_test_pg,
        ];
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
                &params,
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
        let endpoints: Vec<String> = serde_json::from_value(row.get(2)).unwrap_or_default();
        let notes: Option<String> = row.get(3);
        let last_test: Option<UpstreamTestResult> =
            row.get::<_, Option<serde_json::Value>>(4).and_then(|v| serde_json::from_value(v).ok());

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
                account_id: String::new(),
                priority: 0,
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
                "INSERT INTO upstream_profile_secrets (profile_id, key_id, secret, enabled, account_id, priority)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .await?;

        for s in secrets {
            let priority = i32::try_from(s.priority).unwrap_or(0);
            tx.execute(
                &stmt,
                &[
                    &profile_id,
                    &s.id,
                    &s.secret,
                    &s.enabled,
                    &s.account_id,
                    &priority,
                ],
            )
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
                "SELECT profile_id, key_id, secret, enabled, account_id, priority
                 FROM upstream_profile_secrets ORDER BY profile_id, key_id",
                &[],
            )
            .await?;

        let mut map: HashMap<String, Vec<PersistedUpstreamPoolSecret>> = HashMap::new();
        for row in rows {
            let pid: String = row.get(0);
            let priority_i32: i32 = row.get(5);
            map.entry(pid)
                .or_default()
                .push(PersistedUpstreamPoolSecret {
                    id: row.get(1),
                    secret: row.get(2),
                    enabled: row.get(3),
                    account_id: row.get(4),
                    priority: u32::try_from(priority_i32).unwrap_or(0),
                });
        }
        Ok(map)
    }

    // -----------------------------------------------------------------------
    // upstream_profile_configs CRUD (profile metadata cold store)
    // -----------------------------------------------------------------------

    pub async fn upsert_profile_config(
        &self,
        cfg: &crate::persist::PersistedUpstreamProfileConfig,
    ) -> Result<()> {
        let endpoints = serde_json::to_value(&cfg.endpoints)?;
        let fallback_max_retries: Option<i32> =
            cfg.fallback_max_retries.map(|v| i32::try_from(v).unwrap_or(i32::MAX));
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO upstream_profile_configs
                    (profile_id, provider, base_url, fallback_model, endpoints,
                     tls_sni, proxy_url, fallback_profile_id, fallback_max_retries)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (profile_id) DO UPDATE SET
                    provider = EXCLUDED.provider,
                    base_url = EXCLUDED.base_url,
                    fallback_model = EXCLUDED.fallback_model,
                    endpoints = EXCLUDED.endpoints,
                    tls_sni = EXCLUDED.tls_sni,
                    proxy_url = EXCLUDED.proxy_url,
                    fallback_profile_id = EXCLUDED.fallback_profile_id,
                    fallback_max_retries = EXCLUDED.fallback_max_retries",
                &[
                    &cfg.profile_id,
                    &cfg.provider,
                    &cfg.base_url,
                    &cfg.fallback_model,
                    &endpoints,
                    &cfg.tls_sni,
                    &cfg.proxy_url,
                    &cfg.fallback_profile_id,
                    &fallback_max_retries,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn delete_profile_config(&self, profile_id: &str) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "DELETE FROM upstream_profile_configs WHERE profile_id = $1",
                &[&profile_id],
            )
            .await?;
        Ok(())
    }

    pub async fn load_profile_configs(
        &self,
    ) -> Result<HashMap<String, crate::persist::PersistedUpstreamProfileConfig>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT profile_id, provider, base_url, fallback_model, endpoints,
                        tls_sni, proxy_url, fallback_profile_id, fallback_max_retries
                 FROM upstream_profile_configs ORDER BY profile_id",
                &[],
            )
            .await?;

        let mut map = HashMap::new();
        for row in rows {
            let profile_id: String = row.get(0);
            let endpoints: serde_json::Value = row.get(4);
            let endpoints: Vec<String> = serde_json::from_value(endpoints).unwrap_or_default();
            let fallback_max_retries: Option<i32> = row.get(8);
            map.insert(
                profile_id.clone(),
                crate::persist::PersistedUpstreamProfileConfig {
                    profile_id,
                    provider: row.get(1),
                    base_url: row.get(2),
                    fallback_model: row.get(3),
                    endpoints,
                    tls_sni: row.get(5),
                    proxy_url: row.get(6),
                    fallback_profile_id: row.get(7),
                    fallback_max_retries: fallback_max_retries.map(|v| v as u32),
                },
            );
        }
        Ok(map)
    }

    // -----------------------------------------------------------------------
    // oauth_credentials CRUD (Codex OAuth cold store — full TokenRecord JSON)
    // -----------------------------------------------------------------------

    pub async fn upsert_oauth_credential(
        &self,
        record: &crab_auth::types::TokenRecord,
    ) -> Result<()> {
        let profile_id = record
            .metadata
            .get("profile_id")
            .and_then(|v| v.as_str())
            .unwrap_or("codex")
            .to_string();
        let provider = serde_json::to_string(&record.provider)
            .unwrap_or_else(|_| "\"codex\"".to_string())
            .trim_matches('"')
            .to_string();
        let payload = serde_json::to_value(record).context("serialize oauth credential")?;
        let updated_at = chrono::Utc::now().timestamp();
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO oauth_credentials (id, provider, profile_id, payload, updated_at)
                 VALUES ($1, $2, $3, $4::jsonb, $5)
                 ON CONFLICT (id) DO UPDATE
                 SET provider = EXCLUDED.provider,
                     profile_id = EXCLUDED.profile_id,
                     payload = EXCLUDED.payload,
                     updated_at = EXCLUDED.updated_at",
                &[&record.id, &provider, &profile_id, &payload, &updated_at],
            )
            .await?;
        Ok(())
    }

    pub async fn load_oauth_credential(
        &self,
        id: &str,
    ) -> Result<Option<crab_auth::types::TokenRecord>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT payload FROM oauth_credentials WHERE id = $1",
                &[&id],
            )
            .await?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let payload: serde_json::Value = row.get(0);
        Ok(serde_json::from_value(payload).ok())
    }

    pub async fn load_oauth_credentials(&self) -> Result<Vec<crab_auth::types::TokenRecord>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT payload FROM oauth_credentials ORDER BY profile_id, id",
                &[],
            )
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: serde_json::Value = row.get(0);
            if let Ok(rec) = serde_json::from_value::<crab_auth::types::TokenRecord>(payload) {
                out.push(rec);
            }
        }
        Ok(out)
    }

    pub async fn delete_oauth_credential(&self, id: &str) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute("DELETE FROM oauth_credentials WHERE id = $1", &[&id])
            .await?;
        Ok(())
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
        let sampled_at = to_pg_bigint(snapshot.sampled_at);
        let gateway_uptime = to_pg_bigint(gateway_uptime_secs);
        let payload = serde_json::to_value(snapshot).context("serialize metrics snapshot")?;
        let payload_pg = Json(&payload);
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![
            &sampled_at,
            &gateway_uptime,
            &payload_pg,
        ];
        client
            .execute(
                "INSERT INTO metrics_snapshots (sampled_at, gateway_uptime_secs, payload)
                 VALUES ($1, $2, $3::jsonb)
                 ON CONFLICT (sampled_at) DO NOTHING",
                &params,
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
            let payload: serde_json::Value = row.get(0);
            if let Ok(snap) = serde_json::from_value::<MetricsCounterSnapshot>(payload) {
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
        let profile_configs_map = self.load_profile_configs().await.ok()?;

        let profile_secrets = crate::persist::PersistedProfileSecrets {
            by_profile: profile_secrets_map,
        };
        let profile_configs = crate::persist::PersistedProfileConfigs {
            by_profile: profile_configs_map,
        };

        Some(AdminStateFile {
            version: 5,
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
            upstream_profile_configs: profile_configs,
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

        // Import profile metadata configs
        for cfg in json.upstream_profile_configs.by_profile.values() {
            if let Err(e) = self.upsert_profile_config(cfg).await {
                tracing::error!(
                    error = %e,
                    profile_id = %cfg.profile_id,
                    "Failed to import profile config to PG"
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
                     session_store, stable_session_kind, upstream_outbound_bytes,
                     prefill_ms, pre_header_ms,
                     affinity_key, affinity_kind, backend_name,
                     session_fingerprint, is_coalesced, client_key_id,
                     request_passthrough, request_passthrough_prefix_len,
                     status_code, error_code, limit_source, cache_decision,
                     upstream_result, phase_durations_ms, client_kind)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,
                         $15,$16,$17,$18::jsonb,$19,$20,$21,$22,$23,$24,$25,
                         $26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37,$38,
                         $39,$40,$41,$42,$43,$44,$45,$46,$47,$48,$49::jsonb,$50)
                 ON CONFLICT (request_hash, timestamp_ms) DO NOTHING",
            )
            .await?;

        for e in entries {
            let timestamp_ms = to_pg_bigint(e.timestamp_ms);
            let content_length = e.content_length as i32;
            let semantic_cluster = e.semantic_cluster as i32;
            let prompt_tokens = e.prompt_tokens as i32;
            let composition_pg: Option<Json<serde_json::Value>> = match &e.composition {
                Some(c) => Some(Json(
                    serde_json::to_value(c).context("serialize composition jsonb")?,
                )),
                None => None,
            };
            let input_tokens = e.input_tokens.map(to_pg_bigint);
            let output_tokens = e.output_tokens.map(to_pg_bigint);
            let phase_durations_pg: Option<Json<serde_json::Value>> = match &e.phase_durations_ms {
                Some(v) => Some(Json(
                    serde_json::to_value(v).context("serialize phase_durations jsonb")?,
                )),
                None => None,
            };
            let retired_prefix_messages = e.retired_prefix_messages.map(|v| v as i32);
            let upstream_outbound_bytes = e.upstream_outbound_bytes.map(|v| v as i32);
            let request_passthrough_prefix_len = e.request_passthrough_prefix_len.map(|v| v as i32);
            let status_code = e.status_code.map(|v| v as i32);
            tx.execute(
                &stmt,
                &[
                    &e.request_hash as &(dyn tokio_postgres::types::ToSql + Sync),
                    &timestamp_ms as &(dyn tokio_postgres::types::ToSql + Sync),
                    &content_length as &(dyn tokio_postgres::types::ToSql + Sync),
                    &semantic_cluster as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.model as &(dyn tokio_postgres::types::ToSql + Sync),
                    &prompt_tokens as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.latency_ms as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.cache_hit as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.conversation_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.consumer as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.domain as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.project_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.upstream_latency_ms as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.ttft_ms as &(dyn tokio_postgres::types::ToSql + Sync),
                    &input_tokens as &(dyn tokio_postgres::types::ToSql + Sync),
                    &output_tokens as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.cache_tier as &(dyn tokio_postgres::types::ToSql + Sync),
                    &composition_pg as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.request_messages_snapshot as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.response_preview as &(dyn tokio_postgres::types::ToSql + Sync),
                    &retired_prefix_messages as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.reasoning_strategy as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.prompt_cache_hit_ratio as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.upstream_profile_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.pipeline as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.upstream_model as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.client_body_user_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.upstream_user_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.user_id_audit as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.upstream_key_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.session_store as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.stable_session_kind as &(dyn tokio_postgres::types::ToSql + Sync),
                    &upstream_outbound_bytes as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.prefill_ms as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.pre_header_ms as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.affinity_key as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.affinity_kind as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.backend_name as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.session_fingerprint as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.is_coalesced as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.client_key_id as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.request_passthrough as &(dyn tokio_postgres::types::ToSql + Sync),
                    &request_passthrough_prefix_len as &(dyn tokio_postgres::types::ToSql + Sync),
                    &status_code as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.error_code as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.limit_source as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.cache_decision as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.upstream_result as &(dyn tokio_postgres::types::ToSql + Sync),
                    &phase_durations_pg as &(dyn tokio_postgres::types::ToSql + Sync),
                    &e.client_kind as &(dyn tokio_postgres::types::ToSql + Sync),
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
    pub async fn find_trace_log_by_hash(
        &self,
        request_hash: &str,
    ) -> Result<Option<TraceLogEntry>> {
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
        // Try partition-aware pruning first: drop entire partitions older than cutoff.
        if self.is_trace_logs_partitioned().await.unwrap_or(false) {
            use chrono::{TimeZone, Utc};
            if let Some(cutoff_date) = Utc.timestamp_millis_opt(cutoff_ms as i64).single() {
                let cutoff_date = cutoff_date.date_naive();
                let client = self.pool.get().await?;
                let rows = client
                    .query(
                        "SELECT inhrelid::regclass::text
                         FROM pg_inherits
                         WHERE inhparent = 'trace_logs'::regclass
                           AND inhrelid::regclass::text ~ '^trace_logs_y\\d{4}m\\d{2}d\\d{2}$'",
                        &[],
                    )
                    .await?;

                let mut dropped: u64 = 0;
                for row in rows {
                    let part_name: String = row.get(0);
                    if let Some(date_str) = part_name.strip_prefix("trace_logs_y") {
                        if date_str.len() == 10 {
                            if let Ok(date) = chrono::NaiveDate::parse_from_str(
                                &format!(
                                    "{}-{}-{}",
                                    &date_str[0..4],
                                    &date_str[5..7],
                                    &date_str[8..10]
                                ),
                                "%Y-%m-%d",
                            ) {
                                if date < cutoff_date {
                                    let sql = format!("DROP TABLE IF EXISTS {part_name}");
                                    client.execute(sql.as_str(), &[]).await?;
                                    info!(partition = %part_name, "Dropped old trace partition");
                                    dropped += 1;
                                }
                            }
                        }
                    }
                }
                if dropped > 0 {
                    return Ok(dropped);
                }
            }
        }
        // Fallback: row-level DELETE (for non-partitioned tables or empty partition set).
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM trace_logs WHERE timestamp_ms < $1",
                &[&to_pg_bigint(cutoff_ms)],
            )
            .await?;
        Ok(count)
    }

    /// Query top errors from trace_logs since a timestamp (for dataplane error attribution).
    pub async fn query_top_errors_since(&self, since_ms: u64) -> Result<Vec<serde_json::Value>> {
        let client = self.pool().get().await?;
        let rows = client
            .query(
                "SELECT error_code, status_code, upstream_result, COUNT(*) as cnt
                 FROM trace_logs
                 WHERE timestamp_ms >= $1
                   AND (error_code IS NOT NULL OR status_code >= 400)
                 GROUP BY error_code, status_code, upstream_result
                 ORDER BY cnt DESC
                 LIMIT 10",
                &[&(since_ms as i64)],
            )
            .await?;

        let mut results = Vec::new();
        for row in rows {
            let error_code: Option<String> = row.get(0);
            let status_code: Option<i32> = row.get(1);
            let upstream_result: Option<String> = row.get(2);
            let count: i64 = row.get(3);
            results.push(serde_json::json!({
                "error_code": error_code,
                "status_code": status_code,
                "upstream_result": upstream_result,
                "count": count,
            }));
        }
        Ok(results)
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
                serde_json::to_value(&log.request_payload).context("serialize request_payload")?;
            let payload_pg = Json(&payload_json);
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
                    &payload_pg,
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
            let request_payload: serde_json::Value = row.get::<_, Option<serde_json::Value>>(12)
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

    /// Aggregate consumer usage across all months from `consumer_usage_monthly`.
    ///
    /// Returns `(total_input_tokens, total_output_tokens, total_tokens)` summed
    /// over every month, providing a global cumulative view that survives gateway
    /// restarts.
    pub async fn aggregate_all_consumer_usage(&self) -> Result<(u64, u64, u64)> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "SELECT COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(total_tokens), 0)
                 FROM consumer_usage_monthly",
                &[],
            )
            .await?;
        Ok((
            from_pg_bigint(row.get::<_, i64>(0)),
            from_pg_bigint(row.get::<_, i64>(1)),
            from_pg_bigint(row.get::<_, i64>(2)),
        ))
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
        let detail_json = detail.map(|v| serde_json::to_value(v).unwrap_or_default());
        let detail_pg = detail_json.as_ref().map(Json);
        client
            .execute(
                "INSERT INTO audit_log (action, actor, target, detail, ip_address)
                 VALUES ($1, $2, $3, $4::jsonb, $5)",
                &[
                    &action,
                    &actor,
                    &target,
                    &detail_pg,
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

    // -----------------------------------------------------------------------
    // model_peak_hours CRUD
    // -----------------------------------------------------------------------

    /// Upsert aggregated peak-hour rows (batch).
    /// Each tuple: (model, hour_bucket_ms, request_count, input_tokens, output_tokens).
    pub async fn upsert_model_peak_hours(
        &self,
        rows: &[(String, i64, i64, i64, i64)],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let client = self.pool.get().await?;
        let stmt = client
            .prepare_cached(
                "INSERT INTO model_peak_hours (model, hour_bucket, request_count, input_tokens, output_tokens)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (model, hour_bucket) DO UPDATE SET
                     request_count = EXCLUDED.request_count,
                     input_tokens  = EXCLUDED.input_tokens,
                     output_tokens = EXCLUDED.output_tokens,
                     updated_at    = now()",
            )
            .await?;
        for (model, bucket, count, inp, out) in rows {
            client
                .execute(&stmt, &[model, bucket, count, inp, out])
                .await?;
        }
        Ok(())
    }

    /// Query model_peak_hours for a given time range.
    /// Returns (model, hour_bucket_ms, request_count, input_tokens, output_tokens).
    pub async fn query_model_peak_hours(
        &self,
        since_ms: i64,
    ) -> Result<Vec<(String, i64, i64, i64, i64)>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT model, hour_bucket, request_count, input_tokens, output_tokens
                 FROM model_peak_hours
                 WHERE hour_bucket >= $1
                 ORDER BY model, hour_bucket",
                &[&since_ms],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
            .collect())
    }

    /// Delete model peak hour data.
    /// If `hour_bucket` is 0, deletes all data for the given model.
    pub async fn delete_model_peak_hours(&self, model: &str, hour_bucket: i64) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = if hour_bucket == 0 {
            client
                .execute("DELETE FROM model_peak_hours WHERE model = $1", &[&model])
                .await?
        } else {
            client
                .execute(
                    "DELETE FROM model_peak_hours WHERE model = $1 AND hour_bucket = $2",
                    &[&model, &hour_bucket],
                )
                .await?
        };
        Ok(count)
    }

    /// Get the latest hour_bucket watermark for incremental aggregation.
    pub async fn peak_hours_watermark(&self) -> Result<Option<i64>> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "SELECT COALESCE(MAX(hour_bucket), 0) FROM model_peak_hours",
                &[],
            )
            .await?;
        let v: i64 = row.get(0);
        Ok(if v == 0 { None } else { Some(v) })
    }

    /// Aggregate trace_logs into hourly buckets for model peak hours.
    /// Returns rows of (model, hour_bucket_ms, request_count, input_tokens, output_tokens).
    pub async fn aggregate_trace_logs_for_peak_hours(
        &self,
        since_ms: i64,
    ) -> Result<Vec<(String, i64, i64, i64, i64)>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT
                     model,
                     (timestamp_ms / 3600000) * 3600000 AS hour_bucket,
                     COUNT(*)                          AS request_count,
                     COALESCE(SUM(COALESCE(input_tokens, 0)), 0)  AS input_tokens,
                     COALESCE(SUM(COALESCE(output_tokens, 0)), 0) AS output_tokens
                 FROM trace_logs
                 WHERE timestamp_ms >= $1
                 GROUP BY model, hour_bucket
                 ORDER BY model, hour_bucket",
                &[&since_ms],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<_, String>(0),
                    r.get::<_, i64>(1),
                    r.get::<_, i64>(2),
                    r.get::<_, i64>(3),
                    r.get::<_, i64>(4),
                )
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // system_config KV store
    // -----------------------------------------------------------------------

    /// Upsert multiple system config key-value pairs in a single transaction.
    /// Each entry maps a config key (e.g. "cache_config") to its JSON value.
    pub async fn upsert_system_configs(
        &self,
        configs: &HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        if configs.is_empty() {
            return Ok(());
        }
        let client = self.pool.get().await?;
        let stmt = client
            .prepare_cached(
                "INSERT INTO system_config (key, value, updated_at)
                 VALUES ($1, $2, now())
                 ON CONFLICT (key) DO UPDATE SET
                     value = EXCLUDED.value,
                     updated_at = now()",
            )
            .await?;
        for (key, value) in configs {
            client.execute(&stmt, &[key, &Json(value)]).await?;
        }
        Ok(())
    }

    /// Load a single system config value by key.
    /// Returns `None` if the key does not exist.
    pub async fn load_system_config(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                "SELECT value FROM system_config WHERE key = $1",
                &[&key],
            )
            .await?;
        Ok(row.map(|r| r.get::<_, Json<serde_json::Value>>(0).0))
    }

    /// Load all system config entries as a map.
    pub async fn load_all_system_configs(&self) -> Result<HashMap<String, serde_json::Value>> {
        let client = self.pool.get().await?;
        let rows = client
            .query("SELECT key, value FROM system_config", &[])
            .await?;
        let mut map = HashMap::new();
        for row in rows {
            let key: String = row.get(0);
            let value: Json<serde_json::Value> = row.get(1);
            map.insert(key, value.0);
        }
        Ok(map)
    }

    // -----------------------------------------------------------------------
    // gateway_state_snapshots
    // -----------------------------------------------------------------------

    /// Insert or update a gateway state snapshot.
    pub async fn upsert_gateway_snapshot(
        &self,
        snapshot_type: &str,
        keys_json: Option<&serde_json::Value>,
        runtime_json: Option<&serde_json::Value>,
        profiles_json: Option<&serde_json::Value>,
        key_states_json: Option<&serde_json::Value>,
        domain_policies_json: Option<&serde_json::Value>,
        version: i64,
        source: &str,
    ) -> Result<i64> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "INSERT INTO gateway_state_snapshots
                    (snapshot_type, keys_json, runtime_json, profiles_json,
                     key_states_json, domain_policies_json, version, source)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 RETURNING snapshot_id",
                &[
                    &snapshot_type,
                    &keys_json.map(Json),
                    &runtime_json.map(Json),
                    &profiles_json.map(Json),
                    &key_states_json.map(Json),
                    &domain_policies_json.map(Json),
                    &version,
                    &source,
                ],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Load the most recent gateway state snapshot.
    pub async fn load_latest_gateway_snapshot(
        &self,
    ) -> Result<Option<serde_json::Value>> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                "SELECT keys_json, runtime_json, profiles_json,
                        key_states_json, domain_policies_json, version, source, snapshot_at
                 FROM gateway_state_snapshots
                 ORDER BY snapshot_at DESC LIMIT 1",
                &[],
            )
            .await?;
        Ok(row.map(|r| {
            let keys: Option<Json<serde_json::Value>> = r.get(0);
            let runtime: Option<Json<serde_json::Value>> = r.get(1);
            let profiles: Option<Json<serde_json::Value>> = r.get(2);
            let key_states: Option<Json<serde_json::Value>> = r.get(3);
            let domain_policies: Option<Json<serde_json::Value>> = r.get(4);
            let version: i64 = r.get(5);
            let source: String = r.get(6);
            let snapshot_at: chrono::DateTime<chrono::Utc> = r.get(7);
            serde_json::json!({
                "keys": keys.map(|j| j.0),
                "runtime": runtime.map(|j| j.0),
                "profiles": profiles.map(|j| j.0),
                "key_states": key_states.map(|j| j.0),
                "domain_policies": domain_policies.map(|j| j.0),
                "version": version,
                "source": source,
                "snapshot_at": snapshot_at.to_rfc3339(),
            })
        }))
    }

    /// Prune gateway snapshots older than the given timestamp.
    pub async fn prune_gateway_snapshots(&self, cutoff_ms: u64) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM gateway_state_snapshots
                 WHERE snapshot_at < to_timestamp($1::bigint / 1000.0)",
                &[&to_pg_bigint(cutoff_ms)],
            )
            .await?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // codex_oauth_sessions
    // -----------------------------------------------------------------------

    /// Upsert a Codex OAuth session.
    pub async fn upsert_codex_oauth_session(
        &self,
        session_id: &uuid::Uuid,
        session_type: &str,
        profile_id: &str,
        status: &str,
        credential_id: Option<&str>,
        email: Option<&str>,
        account_id: Option<&str>,
        session_json: &serde_json::Value,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO codex_oauth_sessions
                    (session_id, session_type, profile_id, status,
                     credential_id, email, account_id, session_json, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (session_id) DO UPDATE SET
                     status = EXCLUDED.status,
                     credential_id = EXCLUDED.credential_id,
                     email = EXCLUDED.email,
                     account_id = EXCLUDED.account_id,
                     session_json = EXCLUDED.session_json",
                &[
                    session_id,
                    &session_type,
                    &profile_id,
                    &status,
                    &credential_id,
                    &email,
                    &account_id,
                    &Json(session_json),
                    &expires_at,
                ],
            )
            .await?;
        Ok(())
    }

    /// Load pending (non-expired) Codex OAuth sessions for recovery.
    pub async fn load_pending_codex_oauth_sessions(
        &self,
    ) -> Result<Vec<(uuid::Uuid, String, String, String, Json<serde_json::Value>)>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT session_id, session_type, profile_id, status, session_json
                 FROM codex_oauth_sessions
                 WHERE status = 'pending' AND expires_at > now()
                 ORDER BY created_at DESC",
                &[],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let id: uuid::Uuid = r.get(0);
                let st: String = r.get(1);
                let pid: String = r.get(2);
                let status: String = r.get(3);
                let json: Json<serde_json::Value> = r.get(4);
                (id, st, pid, status, json)
            })
            .collect())
    }

    /// Delete expired Codex OAuth sessions.
    pub async fn prune_codex_oauth_sessions(&self) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM codex_oauth_sessions WHERE expires_at < now()",
                &[],
            )
            .await?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // health_probes
    // -----------------------------------------------------------------------

    /// Insert a health probe record.
    pub async fn insert_health_probe(
        &self,
        gateway_ready: bool,
        redis_status: Option<&str>,
        l2_status: Option<&str>,
        uptime_secs: Option<i64>,
        active_keys: Option<i32>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO health_probes
                    (gateway_ready, redis_status, l2_status, uptime_secs, active_keys)
                 VALUES ($1, $2, $3, $4, $5)",
                &[&gateway_ready, &redis_status, &l2_status, &uptime_secs, &active_keys],
            )
            .await?;
        Ok(())
    }

    /// Query health probes within a time window.
    pub async fn load_health_probes(
        &self,
        since_ms: u64,
    ) -> Result<Vec<serde_json::Value>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT gateway_ready, redis_status, l2_status, uptime_secs,
                        active_keys, probe_at
                 FROM health_probes
                 WHERE probe_at >= to_timestamp($1::bigint / 1000.0)
                 ORDER BY probe_at DESC",
                &[&to_pg_bigint(since_ms)],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let ready: bool = r.get(0);
                let redis: Option<String> = r.get(1);
                let l2: Option<String> = r.get(2);
                let uptime: Option<i64> = r.get(3);
                let keys: Option<i32> = r.get(4);
                let at: chrono::DateTime<chrono::Utc> = r.get(5);
                serde_json::json!({
                    "gateway_ready": ready,
                    "redis_status": redis,
                    "l2_status": l2,
                    "uptime_secs": uptime,
                    "active_keys": keys,
                    "probe_at": at.to_rfc3339(),
                })
            })
            .collect())
    }

    /// Prune health probes older than the given timestamp.
    pub async fn prune_health_probes(&self, cutoff_ms: u64) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM health_probes
                 WHERE probe_at < to_timestamp($1::bigint / 1000.0)",
                &[&to_pg_bigint(cutoff_ms)],
            )
            .await?;
        Ok(count)
    }

    // -----------------------------------------------------------------------
    // alert_rules
    // -----------------------------------------------------------------------

    /// Upsert an alert rule.
    pub async fn upsert_alert_rule(
        &self,
        rule_id: &str,
        rule_type: &str,
        condition_json: &serde_json::Value,
        enabled: bool,
        notify_channels: Option<&serde_json::Value>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO alert_rules (rule_id, rule_type, condition_json, enabled, notify_channels)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (rule_id) DO UPDATE SET
                     rule_type = EXCLUDED.rule_type,
                     condition_json = EXCLUDED.condition_json,
                     enabled = EXCLUDED.enabled,
                     notify_channels = EXCLUDED.notify_channels,
                     updated_at = now()",
                &[&rule_id, &rule_type, &Json(condition_json), &enabled, &notify_channels.map(Json)],
            )
            .await?;
        Ok(())
    }

    /// Load all alert rules.
    pub async fn load_alert_rules(&self) -> Result<Vec<serde_json::Value>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT rule_id, rule_type, condition_json, enabled,
                        notify_channels, last_triggered, created_at, updated_at
                 FROM alert_rules ORDER BY created_at DESC",
                &[],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let id: String = r.get(0);
                let rt: String = r.get(1);
                let cond: Json<serde_json::Value> = r.get(2);
                let enabled: bool = r.get(3);
                let channels: Option<Json<serde_json::Value>> = r.get(4);
                let triggered: Option<chrono::DateTime<chrono::Utc>> = r.get(5);
                let created: chrono::DateTime<chrono::Utc> = r.get(6);
                let updated: chrono::DateTime<chrono::Utc> = r.get(7);
                serde_json::json!({
                    "rule_id": id,
                    "rule_type": rt,
                    "condition": cond.0,
                    "enabled": enabled,
                    "notify_channels": channels.map(|c| c.0),
                    "last_triggered": triggered.map(|t| t.to_rfc3339()),
                    "created_at": created.to_rfc3339(),
                    "updated_at": updated.to_rfc3339(),
                })
            })
            .collect())
    }

    /// Delete an alert rule.
    pub async fn delete_alert_rule(&self, rule_id: &str) -> Result<bool> {
        let client = self.pool.get().await?;
        let count = client
            .execute("DELETE FROM alert_rules WHERE rule_id = $1", &[&rule_id])
            .await?;
        Ok(count > 0)
    }

    /// Update last_triggered timestamp for an alert rule.
    pub async fn touch_alert_rule(&self, rule_id: &str) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "UPDATE alert_rules SET last_triggered = now() WHERE rule_id = $1",
                &[&rule_id],
            )
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // webhook_subscriptions
    // -----------------------------------------------------------------------

    /// Upsert a webhook subscription.
    pub async fn upsert_webhook(
        &self,
        webhook_id: &str,
        url: &str,
        events: &serde_json::Value,
        secret_hash: Option<&str>,
        enabled: bool,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO webhook_subscriptions (webhook_id, url, events, secret_hash, enabled)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (webhook_id) DO UPDATE SET
                     url = EXCLUDED.url,
                     events = EXCLUDED.events,
                     secret_hash = EXCLUDED.secret_hash,
                     enabled = EXCLUDED.enabled,
                     updated_at = now()",
                &[&webhook_id, &url, &Json(events), &secret_hash, &enabled],
            )
            .await?;
        Ok(())
    }

    /// Load all webhook subscriptions.
    pub async fn load_webhooks(&self) -> Result<Vec<serde_json::Value>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT webhook_id, url, events, secret_hash, enabled, created_at, updated_at
                 FROM webhook_subscriptions ORDER BY created_at DESC",
                &[],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let id: String = r.get(0);
                let url: String = r.get(1);
                let events: Json<serde_json::Value> = r.get(2);
                let hash: Option<String> = r.get(3);
                let enabled: bool = r.get(4);
                let created: chrono::DateTime<chrono::Utc> = r.get(5);
                let updated: chrono::DateTime<chrono::Utc> = r.get(6);
                serde_json::json!({
                    "webhook_id": id,
                    "url": url,
                    "events": events.0,
                    "secret_hash": hash,
                    "enabled": enabled,
                    "created_at": created.to_rfc3339(),
                    "updated_at": updated.to_rfc3339(),
                })
            })
            .collect())
    }

    /// Delete a webhook subscription.
    pub async fn delete_webhook(&self, webhook_id: &str) -> Result<bool> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "DELETE FROM webhook_subscriptions WHERE webhook_id = $1",
                &[&webhook_id],
            )
            .await?;
        Ok(count > 0)
    }

    // -----------------------------------------------------------------------
    // dashboard_preferences
    // -----------------------------------------------------------------------

    /// Upsert dashboard preferences for a user.
    pub async fn upsert_dashboard_preferences(
        &self,
        user_key: &str,
        preferences: &serde_json::Value,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO dashboard_preferences (user_key, preferences)
                 VALUES ($1, $2)
                 ON CONFLICT (user_key) DO UPDATE SET
                     preferences = EXCLUDED.preferences,
                     updated_at = now()",
                &[&user_key, &Json(preferences)],
            )
            .await?;
        Ok(())
    }

    /// Load dashboard preferences for a user.
    pub async fn load_dashboard_preferences(
        &self,
        user_key: &str,
    ) -> Result<Option<serde_json::Value>> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                "SELECT preferences FROM dashboard_preferences WHERE user_key = $1",
                &[&user_key],
            )
            .await?;
        Ok(row.map(|r| {
            let Json(v): Json<serde_json::Value> = r.get(0);
            v
        }))
    }

    // -----------------------------------------------------------------------
    // backup_metadata
    // -----------------------------------------------------------------------

    /// Insert a backup metadata record.
    pub async fn insert_backup(
        &self,
        backup_type: &str,
        file_path: Option<&str>,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64> {
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                "INSERT INTO backup_metadata (backup_type, file_path, started_at)
                 VALUES ($1, $2, $3)
                 RETURNING backup_id",
                &[&backup_type, &file_path, &started_at],
            )
            .await?;
        Ok(row.get(0))
    }

    /// Mark a backup as completed.
    pub async fn complete_backup(
        &self,
        backup_id: i64,
        file_size_bytes: Option<i64>,
        completed_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "UPDATE backup_metadata
                 SET status = 'completed', file_size_bytes = $2, completed_at = $3
                 WHERE backup_id = $1",
                &[&backup_id, &file_size_bytes, &completed_at],
            )
            .await?;
        Ok(())
    }

    /// Load recent backup records.
    pub async fn load_backups(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                "SELECT backup_id, backup_type, file_path, file_size_bytes,
                        started_at, completed_at, status
                 FROM backup_metadata
                 ORDER BY started_at DESC LIMIT $1",
                &[&limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let id: i64 = r.get(0);
                let bt: String = r.get(1);
                let path: Option<String> = r.get(2);
                let size: Option<i64> = r.get(3);
                let started: chrono::DateTime<chrono::Utc> = r.get(4);
                let completed: Option<chrono::DateTime<chrono::Utc>> = r.get(5);
                let status: String = r.get(6);
                serde_json::json!({
                    "backup_id": id,
                    "backup_type": bt,
                    "file_path": path,
                    "file_size_bytes": size,
                    "started_at": started.to_rfc3339(),
                    "completed_at": completed.map(|c| c.to_rfc3339()),
                    "status": status,
                })
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Table enhancements (ALTER TABLE ADD COLUMN IF NOT EXISTS)
    // -----------------------------------------------------------------------

    /// Ensure enhanced columns exist on existing tables.
    /// Called once at startup; all statements are idempotent.
    pub async fn ensure_enhanced_columns(&self) -> Result<()> {
        let client = self.pool.get().await?;

        // audit_log: old_value, new_value, actor_ip
        client
            .execute(
                "ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS old_value JSONB",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS new_value JSONB",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS actor_ip TEXT",
                &[],
            )
            .await?;

        // keys_meta: last_used_at, last_used_ip
        client
            .execute(
                "ALTER TABLE keys_meta ADD COLUMN IF NOT EXISTS last_used_at TIMESTAMPTZ",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE keys_meta ADD COLUMN IF NOT EXISTS last_used_ip TEXT",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE keys_meta ADD COLUMN IF NOT EXISTS dashboard_created BOOLEAN NOT NULL DEFAULT false",
                &[],
            )
            .await?;

        // upstream_profile_secrets: last_used_at, error_count_24h
        client
            .execute(
                "ALTER TABLE upstream_profile_secrets ADD COLUMN IF NOT EXISTS last_used_at TIMESTAMPTZ",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE upstream_profile_secrets ADD COLUMN IF NOT EXISTS error_count_24h INTEGER DEFAULT 0",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE upstream_profile_secrets ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0",
                &[],
            )
            .await?;

        // domain_usage: input_cost_usd, output_cost_usd
        client
            .execute(
                "ALTER TABLE domain_usage ADD COLUMN IF NOT EXISTS input_cost_usd DOUBLE PRECISION DEFAULT 0",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE domain_usage ADD COLUMN IF NOT EXISTS output_cost_usd DOUBLE PRECISION DEFAULT 0",
                &[],
            )
            .await?;

        // trace_logs: admin_noted, admin_note, client_ip (partitioned table — propagates to all partitions)
        client
            .execute(
                "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS admin_noted BOOLEAN NOT NULL DEFAULT false",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS admin_note TEXT",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS client_ip TEXT",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE trace_logs ADD COLUMN IF NOT EXISTS client_kind TEXT",
                &[],
            )
            .await?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Enhanced audit_log (with old_value/new_value/actor_ip)
    // -----------------------------------------------------------------------

    /// Insert an audit log entry with optional before/after diff.
    pub async fn insert_audit_log_enhanced(
        &self,
        action: &str,
        actor: &str,
        target: Option<&str>,
        detail: Option<&serde_json::Value>,
        old_value: Option<&serde_json::Value>,
        new_value: Option<&serde_json::Value>,
        actor_ip: Option<&str>,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO audit_log (action, actor, target, detail, old_value, new_value, actor_ip)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &action,
                    &actor,
                    &target,
                    &detail.map(Json),
                    &old_value.map(Json),
                    &new_value.map(Json),
                    &actor_ip,
                ],
            )
            .await?;
        Ok(())
    }

    /// Update keys_meta last_used_at from trace_logs.
    ///
    /// TODO: Dead code — no call sites in crab-admin today. Before wiring this up,
    /// ensure trace_logs.client_key_id is populated from the downstream client key
    /// (`client_key_guard`), not upstream_key_id; historical rows may need backfill.
    pub async fn update_keys_last_used(&self) -> Result<u64> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "UPDATE keys_meta km
                 SET last_used_at = sub.last_ts
                 FROM (
                     SELECT client_key_id, to_timestamp(MAX(timestamp_ms) / 1000.0) AS last_ts
                     FROM trace_logs
                     WHERE client_key_id IS NOT NULL AND client_key_id != ''
                     GROUP BY client_key_id
                 ) sub
                 WHERE km.id = sub.client_key_id
                   AND (km.last_used_at IS NULL OR km.last_used_at < sub.last_ts)",
                &[],
            )
            .await?;
        Ok(count)
    }

    /// Update trace_logs admin note.
    pub async fn update_trace_note(
        &self,
        request_hash: &str,
        timestamp_ms: i64,
        note: Option<&str>,
    ) -> Result<bool> {
        let client = self.pool.get().await?;
        let count = client
            .execute(
                "UPDATE trace_logs
                 SET admin_noted = ($3 IS NOT NULL), admin_note = $3
                 WHERE request_hash = $1 AND timestamp_ms = $2",
                &[&request_hash, &timestamp_ms, &note],
            )
            .await?;
        Ok(count > 0)
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
        assert!(sql.contains("COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) >= $3"));
        assert!(sql.contains("COALESCE(input_tokens, 0) + COALESCE(output_tokens, 0)) <= $4"));
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
            dashboard_created: true,
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
    async fn test_keys_meta_dedupe_by_token() {
        let Some(pg) = test_pg().await else { return };
        let client = pg.pool.get().await.unwrap();
        let _ = client.execute("DELETE FROM keys_meta", &[]).await;

        pg.upsert_key(&PersistedKeyMetadata {
            id: "old-id".to_string(),
            token: "sk-cc-dup".to_string(),
            name: "old".to_string(),
            rpm_limit: 0,
            monthly_token_limit: 0,
            expired_at: None,
            model_limits: Vec::new(),
            remain_quota: -1,
            unlimited_quota: true,
            max_concurrent: 0,
            usage_month: String::new(),
            tokens_this_month: 10,
            input_tokens: 0,
            output_tokens: 0,
            dashboard_created: false,
        })
        .await
        .unwrap();
        client
            .execute(
                "INSERT INTO keys_meta (id, token, name, updated_at)
                 VALUES ($1, $2, $3, now() - interval '1 hour')",
                &[&"newer-id", &"sk-cc-dup", &"newer"],
            )
            .await
            .unwrap();

        let removed = pg.dedupe_keys_meta_by_token().await.unwrap();
        assert_eq!(removed, 1);

        let loaded = pg.load_all_keys().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "newer-id");
        assert_eq!(loaded[0].token, "sk-cc-dup");
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
                account_ids: Vec::new(),
                key_ids: Vec::new(),
            },
            PersistedModel {
                profile_id: "deepseek".to_string(),
                id: "deepseek-v4-flash".to_string(),
                owned_by: "deepseek".to_string(),
                context_length: Some(64_000),
                input_price_per_mtok: Some(0.10),
                output_price_per_mtok: Some(0.40),
                available: true,
                account_ids: Vec::new(),
                key_ids: Vec::new(),
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
            account_ids: Vec::new(),
            key_ids: Vec::new(),
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
                account_id: String::new(),
                priority: 0,
            },
            PersistedUpstreamPoolSecret {
                id: "key-2".to_string(),
                secret: "sk-ds-test456".to_string(),
                enabled: false,
                account_id: String::new(),
                priority: 0,
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
            account_id: String::new(),
            priority: 0,
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
            "upstream_profile_configs",
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
                dashboard_created: false,
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
                    account_ids: Vec::new(),
                    key_ids: Vec::new(),
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
                account_id: String::new(),
                priority: 0,
            }],
            upstream_profile_secrets: crate::persist::PersistedProfileSecrets {
                by_profile: [(
                    "openai".to_string(),
                    vec![PersistedUpstreamPoolSecret {
                        id: "oai-1".to_string(),
                        secret: "sk-oai-1".to_string(),
                        enabled: true,
                        account_id: String::new(),
                        priority: 0,
                    }],
                )]
                .into_iter()
                .collect(),
            },
            upstream_profile_configs: crate::persist::PersistedProfileConfigs::default(),
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
