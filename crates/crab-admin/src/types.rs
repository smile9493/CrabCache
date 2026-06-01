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
        priority: k.priority,
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
        priority: req.priority,
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
        email: None,
        plan_type: None,
        models: Vec::new(),
        quota: None,
        priority: k.priority,
    }
}

pub fn enrich_upstream_key_view(
    mut key: UpstreamKeyView,
    entry: &crab_control::UpstreamKeyModelsEntry,
) -> UpstreamKeyView {
    key.models = entry.models.clone();
    key.email = entry.email.clone();
    key.plan_type = entry
        .plan_type
        .clone()
        .or_else(|| entry.quota.as_ref().and_then(|q| q.plan_type.clone()));
    key.quota = entry
        .quota
        .as_ref()
        .map(|q| key_quota_from_control(q.clone()));
    key
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
        plan_type: q.plan_type,
        primary_used_percent: q.primary_used_percent,
        secondary_used_percent: q.secondary_used_percent,
        primary_reset_after_secs: q.primary_reset_after_secs,
        secondary_reset_after_secs: q.secondary_reset_after_secs,
        primary_reset_at_secs: q.primary_reset_at_secs,
        secondary_reset_at_secs: q.secondary_reset_at_secs,
        codex_windows: q.codex_windows.map(|ws| {
            ws.into_iter()
                .map(|w| CodexQuotaWindowItem {
                    id: w.id,
                    label: w.label,
                    used_percent: w.used_percent,
                    reset_at_secs: w.reset_at_secs,
                })
                .collect()
        }),
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

// ── P1: Per-key concurrency / routing / session timeline types ────────

/// A single in-flight or recent request for a given key.
#[derive(Debug, Clone, Serialize)]
pub struct KeyConcurrencyEntry {
    pub request_hash: String,
    pub timestamp_ms: u64,
    pub model: String,
    pub consumer: Option<String>,
    pub affinity_key: Option<String>,
    pub affinity_kind: Option<String>,
    pub backend_name: Option<String>,
    pub session_fingerprint: Option<String>,
    pub is_coalesced: bool,
    pub latency_ms: f64,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
}

/// Response for `GET /api/admin/keys/{id}/concurrency`.
#[derive(Debug, Clone, Serialize)]
pub struct KeyConcurrencyResponse {
    pub key_id: String,
    pub window_secs: u32,
    pub total_requests: usize,
    pub concurrent_peak: u32,
    pub active_now: usize,
    pub entries: Vec<KeyConcurrencyEntry>,
}

/// A backend routing bucket for a specific key.
#[derive(Debug, Clone, Serialize)]
pub struct KeyRoutingBackend {
    pub backend_name: String,
    pub request_count: u64,
    pub affinity_kind: Option<String>,
    pub avg_latency_ms: f64,
    pub cache_hit_rate: f64,
}

/// An affinity migration event: when the same session switched backends.
#[derive(Debug, Clone, Serialize)]
pub struct AffinityMigration {
    pub session_fingerprint: String,
    pub from_backend: String,
    pub to_backend: String,
    pub timestamp_ms: u64,
}

/// Response for `GET /api/admin/keys/{id}/routing`.
#[derive(Debug, Clone, Serialize)]
pub struct KeyRoutingResponse {
    pub key_id: String,
    pub window_secs: u32,
    pub backends: Vec<KeyRoutingBackend>,
    pub migrations: Vec<AffinityMigration>,
    pub prefix_break_count: u64,
}

/// A single event in a session timeline.
#[derive(Debug, Clone, Serialize)]
pub struct SessionEvent {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub model: String,
    pub consumer: Option<String>,
    pub affinity_key: Option<String>,
    pub backend_name: Option<String>,
    pub is_coalesced: bool,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
    pub latency_ms: f64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Response for `GET /api/admin/sessions/{fingerprint}`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionTimelineResponse {
    pub session_fingerprint: String,
    pub window_secs: u32,
    pub total_events: usize,
    pub unique_keys: Vec<String>,
    pub events: Vec<SessionEvent>,
}
