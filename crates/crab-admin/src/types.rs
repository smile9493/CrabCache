//! Admin BFF wire types: shared with Dashboard via `crab-admin-types`, plus server-only queries
//! and `crab-control` bridges.

pub use crab_admin_types::*;

use serde::{Deserialize, Serialize};

/// Historical alias used across admin overview routes (same wire shape as [`GatewayHealth`]).
pub type GatewayHealthView = GatewayHealth;

/// PostgreSQL connection health status.
#[derive(Debug, Clone, Serialize)]
pub struct PgHealth {
    /// Whether PG is configured (CRADMIN_PG_URL is set).
    pub configured: bool,
    /// Whether the connection pool is reachable.
    pub connected: bool,
    /// Number of connections currently available in the pool.
    pub pool_available: Option<usize>,
    /// Maximum pool size.
    pub pool_max: Option<usize>,
    /// Error message (if any).
    pub error: Option<String>,
}

/// Query parameters for `GET /api/admin/models`.
#[derive(Debug, Deserialize)]
pub struct ModelsQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
}

/// Query parameters for `POST /api/admin/models/sync`.
#[derive(Debug, Deserialize)]
pub struct ModelSyncQuery {
    pub profile_id: String,
}

/// Query parameters for `GET /api/admin/logs`.
#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    pub consumer: Option<String>,
    pub model: Option<String>,
    pub cache_tier: Option<String>,
    pub request_hash: Option<String>,
    pub latency_min: Option<f64>,
    pub latency_max: Option<f64>,
    pub token_min: Option<u64>,
    pub token_max: Option<u64>,
}

pub fn upstream_key_input_to_control(k: &UpstreamKeyInput) -> crab_control::UpstreamKeyInput {
    crab_control::UpstreamKeyInput {
        id: k.id.clone(),
        secret: k.secret.clone(),
        enabled: k.enabled,
        account_id: k.account_id.clone(),
    }
}

pub fn put_upstream_keys_to_control(
    req: &PutUpstreamKeysRequest,
) -> crab_control::PutUpstreamKeysRequest {
    crab_control::PutUpstreamKeysRequest {
        keys: req.keys.iter().map(upstream_key_input_to_control).collect(),
        mode: upstream_keys_put_mode_to_control(req.mode),
    }
}

pub fn put_upstream_profile_keys_to_control(
    keys: &[UpstreamKeyInput],
    mode: UpstreamKeysPutMode,
) -> crab_control::PutUpstreamProfileKeysRequest {
    crab_control::PutUpstreamProfileKeysRequest {
        keys: keys.iter().map(upstream_key_input_to_control).collect(),
        mode: upstream_keys_put_mode_to_control(mode),
    }
}

pub fn patch_upstream_key_to_control(
    req: &PatchUpstreamKeyRequest,
) -> crab_control::PatchUpstreamKeyRequest {
    crab_control::PatchUpstreamKeyRequest {
        enabled: req.enabled,
        secret: req.secret.clone(),
    }
}

pub fn upstream_key_view_from_control(k: crab_control::UpstreamKeyView) -> UpstreamKeyView {
    UpstreamKeyView {
        id: k.id,
        preview: k.preview,
        account_id: k.account_id,
        enabled: k.enabled,
        inflight: k.inflight,
        cooldown_remaining_secs: k.cooldown_remaining_secs,
    }
}

pub fn upstream_keys_view_from_control(v: crab_control::UpstreamKeysView) -> UpstreamKeysView {
    UpstreamKeysView {
        keys: v
            .keys
            .into_iter()
            .map(upstream_key_view_from_control)
            .collect(),
    }
}

fn key_quota_from_control(q: crab_control::KeyQuotaInfo) -> KeyQuotaInfo {
    KeyQuotaInfo {
        is_available: q.is_available,
        balance: q.balance,
        total_granted: q.total_granted,
        total_used: q.total_used,
    }
}

pub fn upstream_test_from_control(t: crab_control::UpstreamTestResult) -> UpstreamTestResult {
    UpstreamTestResult {
        ok: t.ok,
        status_code: t.status_code,
        latency_ms: t.latency_ms,
        model_count: t.model_count,
        error: t.error,
        quota: t.quota.map(key_quota_from_control),
    }
}

fn upstream_keys_put_mode_to_control(
    mode: UpstreamKeysPutMode,
) -> crab_control::UpstreamKeysPutMode {
    match mode {
        UpstreamKeysPutMode::Replace => crab_control::UpstreamKeysPutMode::Replace,
        UpstreamKeysPutMode::Append => crab_control::UpstreamKeysPutMode::Append,
    }
}
