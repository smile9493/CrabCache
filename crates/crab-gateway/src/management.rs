use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use crab_cache::{InvalidateScanOptions, TieredCache};
use crab_client_endpoint::ClientEndpointSnapshot;
use crab_control::{
    ApiKeySpec, BackendSpec, CACHE_INVALIDATE_CONFIRM_ALL, CACHE_INVALIDATE_CONFIRM_HEADER,
    ClearReasoningCacheResponse, ClientEndpointView, ConnectionRuntimeView,
    CreateGatewayKeyRequest, CreateGatewayKeyResponse, CursorModelAliasView,
    CursorModelsConfigView, DomainPolicySpec, DomainUsageEntry, DomainUsageResponse, ErrorResponse,
    FeaturesConfigView, GATEWAY_ADMIN_KEY_HEADER, GatewayStatus, LimitsConfigView,
    ModelPricingView, PatchGatewayKeyRequest, PatchUpstreamKeyRequest, PipelineProfileView,
    PipelineRuntimeConfigView, PreflightView, PricingConfigView, PutBackendsRequest, PutDomainPoliciesRequest,
    PutDomainUsageRequest, PutTtlConfigRequest, PutUpstreamKeysRequest,
    PutUpstreamRelayConfigRequest, ReasoningRuntimeConfigView, RoutingBackendsView,
    RoutingSummaryView, ScoreWeightsView, SemanticRuntimeView, StreamCacheConfig, TtlConfigView,
    ModelCooldownView, ResetModelCooldownRequest, UpstreamKeyView,
    UpstreamKeysPutMode, UpstreamKeysView, UpstreamRelayConfigView, constant_time_eq_str,
    parse_backend_endpoints, parse_upstream_base_url,
};
use crab_pipeline::{
    ClientKind, CursorModelEntry, CursorModelsConfig, PipelineMode, PipelineOverride,
    PipelineRuleEngine, validate_cursor_models,
};
use crab_translator::WireFormat;
use crab_control::{
    PipelineRuleMatchView, PipelineRuleView, PipelineRulesConfigView, PipelineTestRequest,
    PipelineTestResponse,
};
use crab_proxy::{
    BackendRouteStrategy, ClientKeyLimiter, DomainPolicy, FeaturesConfig, PricingConfig,
    PreflightConfig, ReasoningConfig, RuntimeConfig, ScoreWeightsConfig, StoredKey,
    UpstreamKeyPool, UpstreamKeySpec,
};
use crab_proxy::{SemanticRuntimeState, SharedSemanticRuntime};
use crab_reasoning::ReasoningBackend;
use crab_state::{RedisStateStore, persist_runtime_state_with_retry};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[path = "management_profiles.rs"]
mod management_profiles;

/// Extract the host portion from a URL string.
/// Handles IPv6 bracket notation (`[::1]`), userinfo, and port stripping.
fn extract_url_host(url: &str) -> Option<&str> {
    let after_scheme = url.split("://").nth(1)?;
    let host_port = after_scheme.split('/').next()?;
    // Handle IPv6 bracket notation: [::1]:8080 → extract ::1
    if let Some(end) = host_port.find(']') {
        let inner = &host_port[..end];
        let inner = inner.strip_prefix('[').unwrap_or(inner);
        return Some(inner);
    }
    // IPv4 / hostname: strip port, then userinfo
    let host = host_port.split(':').next()?;
    let host = host.rsplit('@').next()?;
    Some(host)
}

/// Check if a URL points to a private, loopback, or reserved IP address.
/// Returns `true` if the URL should be rejected for SSRF protection.
pub(crate) fn is_private_or_reserved_url(url: &str) -> bool {
    let Some(host) = extract_url_host(url) else {
        return false; // Malformed URL, let the actual request fail naturally
    };
    let lower = host.to_lowercase();

    // Check for loopback hostnames
    if lower == "localhost" || lower == "127.0.0.1" || lower == "::1" {
        return true;
    }

    // Try to parse as IP address for precise private/reserved range checks
    if let Ok(ip) = lower.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) => {
                return v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_broadcast();
            }
            std::net::IpAddr::V6(v6) => {
                return v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast();
            }
        }
    }

    // Fallback: string-based checks for IPv4 private ranges (when host is an IP string)
    // 10.0.0.0/8
    if lower.starts_with("10.") {
        return true;
    }
    // 172.16.0.0/12
    if lower.starts_with("172.") {
        let after = &lower[4..];
        if let Some(dot) = after.find('.') {
            if let Ok(octet) = after[..dot].parse::<u8>() {
                if (16..=31).contains(&octet) {
                    return true;
                }
            }
        }
    }
    // 192.168.0.0/16
    if lower.starts_with("192.168.") {
        return true;
    }
    // 169.254.0.0/16 (link-local)
    if lower.starts_with("169.254.") {
        return true;
    }
    // IPv6 unique-local addresses fc00::/7 (fd00::/8 and fc00::/8)
    if lower.starts_with("fc") || lower.starts_with("fd") {
        if let Ok(ip) = lower.parse::<std::net::Ipv6Addr>() {
            // fc00::/7: first 7 bits are 1111 110x
            let octets = ip.octets();
            if octets[0] == 0xfc || octets[0] == 0xfd {
                return true;
            }
        }
    }

    false
}

const INVALIDATE_WINDOW: Duration = Duration::from_secs(60);
const INVALIDATE_MAX_PER_WINDOW: usize = 10;
const INVALIDATE_ALL_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct ManagementState {
    pub runtime: Arc<RuntimeConfig>,
    pub tiered_cache: Arc<TieredCache>,
    pub reasoning_store: Arc<ReasoningBackend>,
    pub state_store: Option<Arc<RedisStateStore>>,
    pub reasoning_config: Arc<RwLock<ReasoningConfig>>,
    pub admin_key: String,
    /// Global rate limiter bucket (used by some deployments; kept for compatibility).
    pub global_rate: Arc<pingora_limits::rate::Rate>,
    pub invalidate_all_in_progress: Arc<AtomicBool>,
    pub invalidate_job: Arc<Mutex<Option<InvalidateJobSnapshot>>>,
    pub invalidate_rate: Arc<Mutex<InvalidateRateState>>,
    pub invalidate_scan_timeout_secs: u64,
    pub client_key_limiter: Arc<ClientKeyLimiter>,
    pub upstream_key_cooldown_secs: u64,
    pub semantic_runtime: SharedSemanticRuntime,
    pub semantic_cache: Option<Arc<crab_semantic::SemanticCache>>,
    pub cors_enabled: Arc<AtomicBool>,
    pub max_request_body_bytes: Arc<AtomicUsize>,
    pub max_concurrent_requests: usize,
    pub pricing: Arc<parking_lot::RwLock<PricingConfig>>,
    pub features: Arc<parking_lot::RwLock<FeaturesConfig>>,
    /// Auto-discovered client Base URL (FRP / OpenResty / Pingora observed headers).
    pub client_endpoint: Arc<RwLock<ClientEndpointSnapshot>>,
    /// Client-level lockout registry (brute-force protection).
    pub client_lockouts: Arc<crab_proxy::client_lockout::ClientLockoutRegistry>,
    /// Model-level lockout registry (per-profile/backend/model cooldowns).
    pub model_lockouts: Arc<crab_proxy::model_lockout::ModelLockoutRegistry>,
    /// Webhook store for managing webhook configurations.
    pub webhook_store: crate::webhook::WebhookStore,
    /// HTTP client for webhook delivery.
    pub webhook_client: reqwest::Client,
    /// Codex quota cache (shared with gateway runtime for background refresh).
    pub codex_quota_cache: Option<Arc<crab_proxy::codex_quota_cache::CodexQuotaCache>>,
    /// Shared HTTP client for upstream key/profile testing.
    pub test_http_client: reqwest::Client,
    /// Fault injection for integration testing.
    pub fault_injection: Arc<crab_proxy::fault_injection::FaultInjection>,
    /// Live log broadcast sender for SSE streaming.
    pub log_broadcast: Option<tokio::sync::broadcast::Sender<crate::live_logs::LogLine>>,
    /// Path to the gateway log file for historical reads.
    pub log_file_path: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct InvalidateJobSnapshot {
    pub scope: String,
    pub phase: String,
    pub error: Option<String>,
    pub started_at_secs: u64,
    pub completed_at_secs: Option<u64>,
}

#[derive(Default)]
pub struct InvalidateRateState {
    recent: VecDeque<Instant>,
    last_all_at: Option<Instant>,
}

impl InvalidateRateState {
    fn check(&mut self, is_all: bool) -> Result<(), &'static str> {
        let now = Instant::now();
        while let Some(front) = self.recent.front() {
            if now.duration_since(*front) > INVALIDATE_WINDOW {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        if self.recent.len() >= INVALIDATE_MAX_PER_WINDOW {
            return Err("cache invalidate rate limit exceeded (10 per 60s)");
        }
        if is_all
            && let Some(last) = self.last_all_at
            && now.duration_since(last) < INVALIDATE_ALL_COOLDOWN
        {
            return Err("full cache invalidation is rate limited to once per 60s");
        }
        self.recent.push_back(now);
        if is_all {
            self.last_all_at = Some(now);
        }
        Ok(())
    }
}

pub fn router(state: ManagementState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/ready", get(ready))
        .route("/v1/status", get(status))
        .route("/v1/client-endpoint", get(get_client_endpoint))
        .route("/v1/keys", get(list_keys).post(create_key))
        .route("/v1/cache/invalidate", post(invalidate_cache))
        .route("/v1/cache/invalidate/status", get(get_invalidate_status))
        .route(
            "/v1/cache/fingerprint",
            get(get_fingerprint).put(put_fingerprint),
        )
        .route(
            "/v1/keys/by-id/:id",
            delete(revoke_key_by_id).patch(patch_key_by_id),
        )
        .route("/v1/keys/:token", delete(revoke_key).patch(patch_key))
        .route(
            "/v1/domains/policies",
            get(list_domain_policies).put(put_domain_policies),
        )
        .route(
            "/v1/domains/policies/:domain",
            delete(delete_domain_policy),
        )
        .route(
            "/v1/domains/usage",
            get(get_domain_usage).put(put_domain_usage),
        )
        .route("/v1/cache/ttl", get(get_ttl).put(put_ttl))
        .route(
            "/v1/runtime/stream_cache",
            get(get_stream_cache).put(put_stream_cache),
        )
        .route(
            "/v1/runtime/reasoning",
            get(get_reasoning_runtime).put(put_reasoning_runtime),
        )
        .route(
            "/v1/runtime/pipeline",
            get(get_pipeline_runtime).put(put_pipeline_runtime),
        )
        .route(
            "/v1/pipeline/rules",
            get(get_pipeline_rules).put(put_pipeline_rules),
        )
        .route("/v1/pipeline/test", post(post_pipeline_test))
        .route(
            "/v1/runtime/semantic",
            get(get_semantic_runtime).put(put_semantic_runtime),
        )
        .route(
            "/v1/runtime/connection",
            get(get_connection_runtime).put(put_connection_runtime),
        )
        .route(
            "/v1/config/limits",
            get(get_limits_config).put(put_limits_config),
        )
        .route(
            "/v1/config/pricing",
            get(get_pricing_config).put(put_pricing_config),
        )
        .route(
            "/v1/config/features",
            get(get_features_config).put(put_features_config),
        )
        .route(
            "/v1/cursor/models",
            get(get_cursor_models).put(put_cursor_models),
        )
        .route("/v1/reasoning/cache", delete(clear_reasoning_cache))
        .route("/v1/routing/backends", get(get_backends).put(put_backends))
        .route(
            "/v1/upstream/keys",
            get(get_upstream_keys).put(put_upstream_keys),
        )
        .route(
            "/v1/upstream/keys/:id",
            patch(patch_upstream_key).delete(delete_upstream_key),
        )
        .route(
            "/v1/upstream/keys/:id/reset-model-cooldown",
            post(reset_model_cooldown),
        )
        .route(
            "/v1/upstream/relay",
            get(get_upstream_relay).put(put_upstream_relay),
        )
        .route(
            "/v1/upstream/profiles",
            get(management_profiles::list_upstream_profiles),
        )
        .route(
            "/v1/upstream/profiles/:id",
            axum::routing::put(management_profiles::put_upstream_profile)
                .delete(management_profiles::delete_upstream_profile),
        )
        .route(
            "/v1/upstream/profiles/:id/keys",
            get(management_profiles::get_profile_keys).put(management_profiles::put_profile_keys),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/export",
            get(management_profiles::export_profile_keys),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/:key_id",
            patch(management_profiles::patch_profile_key)
                .delete(management_profiles::delete_profile_key),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/:key_id/reset-model-cooldown",
            post(management_profiles::reset_profile_key_model_cooldown),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/:key_id/test",
            post(management_profiles::test_upstream_profile_key),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/models",
            get(management_profiles::get_profile_keys_models),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/:key_id/models",
            get(management_profiles::get_profile_key_models),
        )
        .route(
            "/v1/upstream/profiles/:id/keys/models-catalog",
            put(management_profiles::put_profile_keys_models_catalog),
        )
        .route(
            "/v1/upstream/profiles/:id/test",
            post(management_profiles::test_upstream_profile),
        )
        .route(
            "/v1/upstream/profiles/:id/routing",
            get(management_profiles::get_profile_routing),
        )
        .route("/v1/routing/summary", get(get_routing_summary))
        .route("/v1/resilience/lockouts", get(get_lockouts))
        .route(
            "/v1/resilience/lockouts/model/:profile/:backend/:model",
            delete(clear_model_lockout),
        )
        .route("/v1/system/restart", post(restart_gateway_handler))
        .route("/v1/state/snapshot", get(get_state_snapshot))
        .route("/v1/logs", get(crate::live_logs::get_logs))
        .route("/v1/logs/stream", get(crate::live_logs::stream_logs))
        .merge(crate::webhook_admin::build_webhook_routes())
        // Debug-only: fault injection control (returns 403 in release builds)
        .route(
            "/v1/debug/fault-injection",
            get(get_fault_injection).put(put_fault_injection).delete(delete_fault_injection),
        )
        .with_state(state)
}

#[derive(serde::Deserialize)]
struct InvalidateRequest {
    scope: String,
}

#[derive(serde::Serialize)]
struct InvalidateResponse {
    scope: String,
    status: String,
}

#[derive(serde::Serialize)]
struct InvalidateStatusResponse {
    all_in_progress: bool,
    job: Option<InvalidateJobSnapshot>,
}

/// Response type for lockout snapshots.
#[derive(serde::Serialize)]
struct LockoutsResponse {
    client_lockouts: Vec<crab_proxy::client_lockout::ClientLockoutSnapshot>,
    model_lockouts: Vec<crab_proxy::model_lockout::ModelLockoutSnapshot>,
}

/// GET /v1/resilience/lockouts — list all active lockouts.
async fn get_lockouts(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<LockoutsResponse>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(LockoutsResponse {
        client_lockouts: state.client_lockouts.snapshots(),
        model_lockouts: state.model_lockouts.snapshots(),
    }))
}

/// DELETE /v1/resilience/lockouts/model/{profile}/{backend}/{model} — clear a model lockout.
async fn clear_model_lockout(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path((profile, backend, model)): Path<(String, String, String)>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    state.model_lockouts.clear(&profile, &backend, &model);
    Ok(StatusCode::NO_CONTENT)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn get_invalidate_status(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<InvalidateStatusResponse>, Response> {
    authorize(&headers, &state.admin_key)?;

    let job = state.invalidate_job.lock().await.clone();

    Ok(Json(InvalidateStatusResponse {
        all_in_progress: state.invalidate_all_in_progress.load(Ordering::SeqCst),
        job,
    }))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FingerprintRequest {
    version: u32,
    #[serde(default = "default_fingerprint_normalize")]
    normalize_content: bool,
}

fn default_fingerprint_normalize() -> bool {
    true
}

pub enum InvalidateAction {
    All,
    Prefix(String),
    Key(String),
}

fn parse_invalidate_scope(scope: &str) -> Result<InvalidateAction, String> {
    let trimmed = scope.trim();
    if trimmed.is_empty() {
        return Err("scope cannot be empty".to_string());
    }
    if trimmed == "all" {
        return Ok(InvalidateAction::All);
    }
    if let Some(prefix) = trimmed.strip_prefix("prefix:") {
        if prefix.is_empty() {
            return Err("prefix cannot be empty".to_string());
        }
        return Ok(InvalidateAction::Prefix(prefix.to_string()));
    }
    Ok(InvalidateAction::Key(trimmed.to_string()))
}

/// `scope=all` requires `x-cache-invalidate-confirm: all`.
pub fn require_invalidate_confirm(
    action: &InvalidateAction,
    headers: &HeaderMap,
) -> Result<(), String> {
    if !matches!(action, InvalidateAction::All) {
        return Ok(());
    }
    let confirmed = headers
        .get(CACHE_INVALIDATE_CONFIRM_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == CACHE_INVALIDATE_CONFIRM_ALL)
        .unwrap_or(false);
    if confirmed {
        Ok(())
    } else {
        Err(format!(
            "scope=all requires header {CACHE_INVALIDATE_CONFIRM_HEADER}: {CACHE_INVALIDATE_CONFIRM_ALL}"
        ))
    }
}

async fn invalidate_cache(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<InvalidateRequest>,
) -> Result<Json<InvalidateResponse>, Response> {
    authorize(&headers, &state.admin_key)?;

    let action = parse_invalidate_scope(&req.scope)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response())?;

    require_invalidate_confirm(&action, &headers)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response())?;

    let is_all = matches!(action, InvalidateAction::All);

    {
        let mut rate = state.invalidate_rate.lock().await;
        rate.check(is_all).map_err(|e| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        })?;
    }

    if is_all
        && state
            .invalidate_all_in_progress
            .swap(true, Ordering::SeqCst)
    {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "a full cache invalidation is already in progress".to_string(),
            }),
        )
            .into_response());
    }

    let tiered_cache = state.tiered_cache.clone();
    let in_progress = state.invalidate_all_in_progress.clone();
    let invalidate_job = state.invalidate_job.clone();
    let scope_label = req.scope.clone();
    let started_at_secs = now_secs();
    let scan_opts = InvalidateScanOptions::from_timeout_secs(state.invalidate_scan_timeout_secs);

    {
        let mut slot = invalidate_job.lock().await;
        *slot = Some(InvalidateJobSnapshot {
            scope: scope_label.clone(),
            phase: "running".to_string(),
            error: None,
            started_at_secs,
            completed_at_secs: None,
        });
    }

    tokio::spawn(async move {
        /// Guard that resets `invalidate_all_in_progress` on drop (even on panic).
        struct InProgressGuard {
            flag: Arc<AtomicBool>,
        }
        impl Drop for InProgressGuard {
            fn drop(&mut self) {
                self.flag.store(false, Ordering::SeqCst);
            }
        }

        // If this is a scope=all job, the guard guarantees the flag is cleared
        // even if the match arms panic.
        let _guard = is_all.then(|| InProgressGuard { flag: in_progress.clone() });

        let result = match action {
            InvalidateAction::All => {
                tracing::info!(scope = %scope_label, "Starting full cache invalidation");
                let r = tiered_cache.invalidate_all(scan_opts).await;
                match &r {
                    Ok(_) => {
                        tracing::info!(scope = %scope_label, "Full cache invalidation completed")
                    }
                    Err(e) => {
                        tracing::warn!(scope = %scope_label, error = %e, "Full cache invalidation failed")
                    }
                }
                r
            }
            InvalidateAction::Prefix(p) => {
                tracing::info!(scope = %scope_label, prefix = %p, "Starting prefix cache invalidation");
                let r = tiered_cache.invalidate_prefix(&p, scan_opts).await;
                match &r {
                    Ok(_) => {
                        tracing::info!(scope = %scope_label, prefix = %p, "Prefix cache invalidation completed")
                    }
                    Err(e) => {
                        tracing::warn!(scope = %scope_label, prefix = %p, error = %e, "Prefix cache invalidation failed")
                    }
                }
                r
            }
            InvalidateAction::Key(k) => {
                tracing::info!(scope = %scope_label, key = %k, "Starting single key cache invalidation");
                let r = tiered_cache.invalidate(&k).await;
                match &r {
                    Ok(_) => {
                        tracing::info!(scope = %scope_label, key = %k, "Single key cache invalidation completed")
                    }
                    Err(e) => {
                        tracing::warn!(scope = %scope_label, key = %k, error = %e, "Single key cache invalidation failed")
                    }
                }
                r
            }
        };

        {
            let mut slot = invalidate_job.lock().await;
            let completed_at_secs = now_secs();
            let snapshot = match &result {
                Ok(_) => InvalidateJobSnapshot {
                    scope: scope_label.clone(),
                    phase: "completed".to_string(),
                    error: None,
                    started_at_secs,
                    completed_at_secs: Some(completed_at_secs),
                },
                Err(e) => InvalidateJobSnapshot {
                    scope: scope_label.clone(),
                    phase: "failed".to_string(),
                    error: Some(e.to_string()),
                    started_at_secs,
                    completed_at_secs: Some(completed_at_secs),
                },
            };
            *slot = Some(snapshot);
        }

        drop(result);
    });

    Ok(Json(InvalidateResponse {
        scope: req.scope,
        status: "accepted".to_string(),
    }))
}

async fn get_fingerprint(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<FingerprintRequest>, Response> {
    authorize(&headers, &state.admin_key)?;

    let cfg = state.runtime.fingerprint.read();

    Ok(Json(FingerprintRequest {
        version: cfg.version,
        normalize_content: cfg.normalize_content,
    }))
}

async fn put_fingerprint(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<FingerprintRequest>,
) -> Result<Json<FingerprintRequest>, Response> {
    authorize(&headers, &state.admin_key)?;

    let old = state.runtime.fingerprint.read().clone();
    let mut new_cfg = (*old).clone();
    new_cfg.version = req.version;
    new_cfg.normalize_content = req.normalize_content;

    tracing::info!(
        version = req.version,
        normalize = req.normalize_content,
        "Fingerprint config updated"
    );

    let resp = FingerprintRequest {
        version: new_cfg.version,
        normalize_content: new_cfg.normalize_content,
    };
    *state.runtime.fingerprint.write() = std::sync::Arc::new(new_cfg);
    schedule_persist_state(&state);
    Ok(Json(resp))
}

async fn health() -> StatusCode {
    StatusCode::OK
}

#[derive(serde::Serialize)]
struct ReadyResponse {
    ready: bool,
    redis: &'static str,
    l2: &'static str,
}

async fn ready(State(state): State<ManagementState>) -> (StatusCode, Json<ReadyResponse>) {
    let redis_ok = state.tiered_cache.ping().await;
    let l2_status: &'static str = if state.semantic_cache.is_some() {
        "ok"
    } else {
        "disabled"
    };
    if redis_ok {
        (
            StatusCode::OK,
            Json(ReadyResponse {
                ready: true,
                redis: "ok",
                l2: l2_status,
            }),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                ready: false,
                redis: "unavailable",
                l2: l2_status,
            }),
        )
    }
}

pub(crate) fn schedule_persist_state(state: &ManagementState) {
    let Some(store) = state.state_store.clone() else {
        return;
    };
    let runtime = state.runtime.clone();
    tokio::spawn(async move {
        if let Err(e) = persist_runtime_state_with_retry(store.as_ref(), &runtime).await {
            tracing::error!(error = %e, "Failed to persist control plane state to Redis after retries");
        }
    });
}

/// Await Redis persistence for control-plane mutations that must survive hot reload.
pub(crate) async fn persist_state_sync(state: &ManagementState) {
    let Some(store) = state.state_store.clone() else {
        return;
    };
    if let Err(e) = persist_runtime_state_with_retry(store.as_ref(), &state.runtime).await {
        tracing::error!(error = %e, "Failed to sync-persist control plane state to Redis");
    }
}

pub(crate) fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), Response> {
    let provided = headers
        .get(GATEWAY_ADMIN_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq_str(provided, expected) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid or missing admin key".to_string(),
            }),
        )
            .into_response());
    }
    Ok(())
}

async fn status(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<GatewayStatus>, Response> {
    authorize(&headers, &state.admin_key)?;
    let backend_count = state.runtime.router.read().meta().len();
    let (upstream_key_count, upstream_keys_available) = state.runtime.default_upstream_key_stats();
    let upstream_base_url = Some(state.runtime.upstream_base_url.read().clone());
    let upstream_model = Some(state.runtime.fallback_model.read().clone());
    Ok(Json(GatewayStatus {
        uptime_secs: state.runtime.uptime_secs(),
        active_keys: state.runtime.keys.len() as u64,
        backend_count,
        stream_cache_enabled: state.runtime.stream_cache_enabled(),
        upstream_key_count,
        upstream_keys_available,
        global_rps_estimate: state.global_rate.rate(&crate::GLOBAL_RATE_KEY),
        upstream_base_url,
        upstream_model,
    }))
}

fn client_endpoint_view(snap: &ClientEndpointSnapshot) -> ClientEndpointView {
    ClientEndpointView {
        gateway_url: snap.gateway_url.clone(),
        gateway_url_lan: snap.gateway_url_lan.clone(),
        gateway_url_public: snap.gateway_url_public.clone(),
        public_source: snap.public_source.map(|s| match s {
            crab_client_endpoint::PublicUrlSource::Env => "env".to_string(),
            crab_client_endpoint::PublicUrlSource::Observed => "observed".to_string(),
            crab_client_endpoint::PublicUrlSource::Frp => "frp".to_string(),
            crab_client_endpoint::PublicUrlSource::Openresty => "openresty".to_string(),
        }),
    }
}

async fn get_client_endpoint(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<ClientEndpointView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let snap = state.client_endpoint.read().clone();
    Ok(Json(client_endpoint_view(&snap)))
}

fn upstream_relay_view(runtime: &RuntimeConfig) -> UpstreamRelayConfigView {
    let base_url = runtime.upstream_base_url.read().clone();
    let model = runtime.fallback_model.read().clone();
    let api_key = {
        let raw = runtime.upstream_pool().admin_secret();
        match raw {
            Some(ref s) if s.len() > 8 => Some(format!("{}****", &s[..4])),
            Some(_) => Some("****".to_string()),
            None => None,
        }
    };
    UpstreamRelayConfigView {
        base_url,
        model,
        api_key,
    }
}

async fn get_upstream_relay(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<UpstreamRelayConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(upstream_relay_view(&state.runtime)))
}

async fn put_upstream_relay(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutUpstreamRelayConfigRequest>,
) -> Result<Json<UpstreamRelayConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;

    let parsed = parse_upstream_base_url(&req.base_url)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response())?;

    {
        let mut base = state.runtime.upstream_base_url.write();
        *base = parsed.normalized.clone();
    }

    if let Some(model) = &req.model {
        if model.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "model must not be empty when provided".to_string(),
                }),
            )
                .into_response());
        }
        let mut fallback = state.runtime.fallback_model.write();
        *fallback = model.trim().to_string();
    }

    let endpoints = req
        .endpoints
        .clone()
        .unwrap_or_else(|| vec![parsed.endpoint.clone()]);
    let tls_sni = req
        .tls_sni
        .clone()
        .unwrap_or_else(|| parsed.tls_sni.clone());

    let parsed_backends = parse_backend_endpoints(&endpoints, 1, &tls_sni).map_err(|errors| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: errors.join("; "),
            }),
        )
            .into_response()
    })?;

    let mut router = state.runtime.router.write();
    router.rebuild(&parsed_backends).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()
    })?;
    drop(router);

    let default_id = state.runtime.default_upstream_profile_id();
    if let Some(existing) = state.runtime.profile(&default_id) {
        let model = state.runtime.fallback_model.read().clone();
        let input = crab_proxy::ProfileBuildInput {
            id: default_id.clone(),
            provider: existing.provider.as_str().to_string(),
            base_url: parsed.normalized.clone(),
            fallback_model: model,
            endpoints: endpoints.clone(),
            tls_sni: Some(tls_sni.clone()),
            default_weight: 1,
            proxy_url: None,
            fallback_profile_id: None,
            fallback_max_retries: 2,
            connection: existing.connection.clone(),
            key_source: "management-routing",
        };
        if let Ok(profile) = crab_proxy::build_profile_runtime(
            input,
            Vec::new(),
            state.upstream_key_cooldown_secs,
            Some(Arc::clone(&existing.upstream_pool)),
        ) {
            let _ = state.runtime.upsert_profile(profile);
        }
    }

    tracing::info!(
        base_url = %parsed.normalized,
        endpoints = ?endpoints,
        tls_sni = %tls_sni,
        "Upstream relay config updated"
    );

    schedule_persist_state(&state);
    Ok(Json(upstream_relay_view(&state.runtime)))
}

async fn get_upstream_keys(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<UpstreamKeysView>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(upstream_keys_view(&state.runtime)))
}

async fn put_upstream_keys(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutUpstreamKeysRequest>,
) -> Result<Json<UpstreamKeysView>, Response> {
    authorize(&headers, &state.admin_key)?;
    if req.keys.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "at least one upstream key is required".to_string(),
            }),
        )
            .into_response());
    }
    for (i, k) in req.keys.iter().enumerate() {
        if k.secret.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("upstream key #{} secret must not be empty", i + 1),
                }),
            )
                .into_response());
        }
    }
    let specs: Vec<UpstreamKeySpec> = req
        .keys
        .into_iter()
        .map(|k| UpstreamKeySpec {
            id: k.id,
            secret: k.secret,
            enabled: k.enabled,
            account_id: k.account_id,
            supported_models: Vec::new(),
            priority: k.priority,
        })
        .collect();

    // 1) Build new pool for the default profile (from its current pool + incoming specs).
    let current = state.runtime.upstream_pool();
    let new_pool = match req.mode {
        UpstreamKeysPutMode::Append => UpstreamKeyPool::merge_append(&current, specs.clone()),
        UpstreamKeysPutMode::Replace => UpstreamKeyPool::hot_replace(&current, specs.clone()),
    };
    let default_id = state.runtime.default_upstream_profile_id();
    state
        .runtime
        .replace_profile_pool(&default_id, new_pool)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        })?;

    if let Some(qc) = &state.codex_quota_cache {
        if let Some(default_profile) = state.runtime.profile(&default_id) {
            if default_profile.provider == crab_pipeline::UpstreamProvider::Codex {
                let pool_arc = default_profile.upstream_pool.read().clone();
                pool_arc.set_quota_cache(qc.clone());
            }
        }
    }

    tracing::info!(
        profile_id = %default_id,
        mode = ?req.mode,
        key_count = specs.len(),
        "Updated default upstream profile key pool (other profiles unchanged)"
    );

    schedule_persist_state(&state);
    Ok(Json(upstream_keys_view(&state.runtime)))
}

async fn patch_upstream_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PatchUpstreamKeyRequest>,
) -> Result<Json<UpstreamKeyView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let pool = state.runtime.upstream_pool();
    if req.enabled.is_none() && req.secret.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "no fields to update".to_string(),
            }),
        )
            .into_response());
    }
    if let Some(enabled) = req.enabled {
        if !pool.set_enabled(&id, enabled) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("upstream key '{id}' not found"),
                }),
            )
                .into_response());
        }
    }
    if req.secret.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "rotating secret via PATCH is not supported; use PUT /v1/upstream/keys"
                    .to_string(),
            }),
        )
            .into_response());
    }
    let view = pool
        .list_status()
        .into_iter()
        .find(|k| k.id == id)
        .map(|k| UpstreamKeyView {
            id: k.id,
            preview: k.preview,
            account_id: k.account_id,
            enabled: k.enabled,
            inflight: k.inflight,
            cooldown_remaining_secs: k.cooldown_remaining_secs,
            priority: k.priority,
            model_cooldowns: k
                .model_cooldowns
                .into_iter()
                .map(|mc| ModelCooldownView {
                    model: mc.model,
                    remaining_secs: mc.remaining_secs,
                    backoff_level: mc.backoff_level,
                })
                .collect(),
        })
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("upstream key '{id}' not found"),
                }),
            )
                .into_response()
        })?;
    schedule_persist_state(&state);
    Ok(Json(view))
}

async fn delete_upstream_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    let key_id = id.trim();
    let default_id = state.runtime.default_upstream_profile_id();
    let pool = state.runtime.upstream_pool();
    let Some(new_pool) = UpstreamKeyPool::remove_key(&pool, key_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("upstream key '{key_id}' not found"),
            }),
        )
            .into_response());
    };
    state
        .runtime
        .replace_profile_pool(&default_id, new_pool)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        })?;
    schedule_persist_state(&state);
    Ok(StatusCode::NO_CONTENT)
}

/// POST `/v1/upstream/keys/:id/reset-model-cooldown`
async fn reset_model_cooldown(
    State(state): State<ManagementState>,
    Path(key_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ResetModelCooldownRequest>,
) -> Result<impl IntoResponse, Response> {
    authorize(&headers, &state.admin_key)?;
    let pool = state.runtime.default_profile().resolve_upstream_pool();
    let removed = pool.reset_model_cooldowns(&key_id, body.model.as_deref());
    if removed == 0 && !pool.key_exists(&key_id) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("upstream key '{key_id}' not found"),
            }),
        )
            .into_response());
    }
    schedule_persist_state(&state);
    Ok(Json(serde_json::json!({
        "key_id": key_id,
        "model_cooldowns_cleared": removed,
    })))
}

fn upstream_keys_view(runtime: &RuntimeConfig) -> UpstreamKeysView {
    let pool = runtime.default_profile().resolve_upstream_pool();
    UpstreamKeysView {
        keys: pool
            .list_status()
            .into_iter()
            .map(|k| UpstreamKeyView {
                id: k.id,
                preview: k.preview,
                account_id: k.account_id,
                enabled: k.enabled,
                inflight: k.inflight,
                cooldown_remaining_secs: k.cooldown_remaining_secs,
                priority: k.priority,
                model_cooldowns: k
                    .model_cooldowns
                    .into_iter()
                    .map(|mc| ModelCooldownView {
                        model: mc.model,
                        remaining_secs: mc.remaining_secs,
                        backoff_level: mc.backoff_level,
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn key_preview(token: &str) -> String {
    if token.len() <= 10 {
        token.to_string()
    } else {
        format!("{}...", &token[..8])
    }
}

fn stored_to_spec(
    token: &str,
    key: &StoredKey,
    include_full: bool,
    limiter: &ClientKeyLimiter,
) -> ApiKeySpec {
    ApiKeySpec {
        id: key.id.clone(),
        name: key.name.clone(),
        key_preview: key_preview(token),
        key_full: if include_full {
            Some(token.to_string())
        } else {
            None
        },
        enabled: key.enabled,
        domain: key.domain.clone(),
        project_id: key.project_id.clone(),
        pipeline: key.pipeline.clone(),
        upstream_profile: key.upstream_profile.clone(),
        max_concurrent: key.max_concurrent,
        rpm_limit: key.rpm_limit,
        inflight: limiter.inflight(token),
    }
}

async fn list_keys(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKeySpec>>, Response> {
    authorize(&headers, &state.admin_key)?;
    let keys: Vec<ApiKeySpec> = state
        .runtime
        .keys
        .iter()
        .map(|entry| stored_to_spec(entry.key(), entry.value(), true, &state.client_key_limiter))
        .collect();
    Ok(Json(keys))
}

async fn create_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<CreateGatewayKeyRequest>,
) -> Result<Json<CreateGatewayKeyResponse>, Response> {
    authorize(&headers, &state.admin_key)?;

    let token = req.token.clone().unwrap_or_else(|| {
        format!(
            "sk-cc-{}",
            &uuid::Uuid::new_v4().to_string().replace('-', "")[..24]
        )
    });

    // Validate token format and length
    if token.len() > 512 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "token must not exceed 512 characters".to_string(),
            }),
        )
            .into_response());
    }
    if token.contains(|c: char| c.is_control()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "token must not contain control characters".to_string(),
            }),
        )
            .into_response());
    }
    if token.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "token must not be empty or whitespace-only".to_string(),
            }),
        )
            .into_response());
    }

    if state.runtime.keys.contains_key(&token) {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "key already exists".to_string(),
            }),
        )
            .into_response());
    }

    let project_id = match req
        .project_id
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(raw) => Some(crab_proxy::sanitize_user_id(raw).map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response()
        })?),
    };

    let id = uuid::Uuid::new_v4().to_string();
    let max_concurrent = req.max_concurrent.unwrap_or(0);
    let stored = StoredKey {
        id: id.clone(),
        name: req.name.clone(),
        key_hash: token.clone(),
        enabled: req.enabled,
        domain: req.domain.clone(),
        project_id: project_id.clone(),
        pipeline: req.pipeline.clone(),
        upstream_profile: req.upstream_profile.clone(),
        max_concurrent,
        rpm_limit: req.rpm_limit.unwrap_or(0),
    };
    state.runtime.keys.insert(token.clone(), stored.clone());
    state.client_key_limiter.sync_key(&token, &stored);

    // Async persist: sync snapshot can block on runtime locks and stall Management API (502/timeouts).
    schedule_persist_state(&state);
    Ok(Json(CreateGatewayKeyResponse {
        id,
        name: req.name,
        key_full: token.clone(),
        key_preview: key_preview(&token),
        enabled: req.enabled,
        domain: req.domain,
        project_id: project_id.clone(),
        pipeline: req.pipeline,
        upstream_profile: req.upstream_profile,
        max_concurrent,
        rpm_limit: stored.rpm_limit,
    }))
}

fn token_for_stored_key_id(
    runtime: &crab_proxy::RuntimeConfig,
    id: &str,
) -> Result<String, Response> {
    runtime
        .keys
        .iter()
        .find(|entry| entry.value().id == id)
        .map(|entry| entry.key().clone())
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "key not found".to_string(),
                }),
            )
                .into_response()
        })
}

async fn revoke_key_by_id(
    state: State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Response> {
    let token = token_for_stored_key_id(&state.runtime, &id)?;
    revoke_key(state, headers, Path(token)).await
}

async fn patch_key_by_id(
    state: State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Json<PatchGatewayKeyRequest>,
) -> Result<Json<ApiKeySpec>, Response> {
    let token = token_for_stored_key_id(&state.runtime, &id)?;
    patch_key(state, headers, Path(token), req).await
}

async fn revoke_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    if state.runtime.keys.remove(&token).is_some() {
        state.client_key_limiter.remove_key(&token);
        schedule_persist_state(&state);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "key not found".to_string(),
            }),
        )
            .into_response())
    }
}

async fn list_domain_policies(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DomainPolicySpec>>, Response> {
    authorize(&headers, &state.admin_key)?;
    let specs: Vec<DomainPolicySpec> = state
        .runtime
        .list_domain_policies()
        .into_iter()
        .map(|(domain, policy)| DomainPolicySpec {
            domain,
            monthly_token_budget: policy.monthly_token_budget,
            monthly_cost_budget_usd: policy.monthly_cost_budget_usd,
            min_hit_rate: policy.min_hit_rate,
            enabled: policy.enabled,
            pipeline: policy.pipeline.clone(),
            upstream_profile: policy.upstream_profile.clone(),
        })
        .collect();
    Ok(Json(specs))
}

async fn put_domain_policies(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutDomainPoliciesRequest>,
) -> Result<Json<Vec<DomainPolicySpec>>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut map = indexmap::IndexMap::new();
    for p in req.policies {
        map.insert(
            p.domain.clone(),
            DomainPolicy {
                monthly_token_budget: p.monthly_token_budget,
                monthly_cost_budget_usd: p.monthly_cost_budget_usd,
                min_hit_rate: p.min_hit_rate,
                enabled: p.enabled,
                pipeline: p.pipeline.clone(),
                upstream_profile: p.upstream_profile.clone(),
            },
        );
    }
    state.runtime.replace_domain_policies(map);
    persist_state_sync(&state).await;
    list_domain_policies(State(state), headers).await
}

async fn delete_domain_policy(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(domain): Path<String>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    {
        let mut guard = state.runtime.domain_policies.write();
        if guard.remove(&domain).is_some() {
            schedule_persist_state(&state);
            return Ok(StatusCode::NO_CONTENT);
        }
    }
    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "domain policy not found".to_string(),
        }),
    )
        .into_response())
}

async fn get_domain_usage(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<DomainUsageResponse>, Response> {
    authorize(&headers, &state.admin_key)?;
    let snapshot = state.runtime.domain_usage_snapshot();
    let month = chrono::Utc::now().format("%Y-%m").to_string();
    let usage = snapshot
        .into_iter()
        .map(|(domain, u)| DomainUsageEntry {
            domain,
            tokens: u.tokens,
            spend_usd: u.spend_usd,
        })
        .collect();
    Ok(Json(DomainUsageResponse { usage, month }))
}

async fn put_domain_usage(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutDomainUsageRequest>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut map = std::collections::HashMap::new();
    for entry in req.usage {
        map.insert(
            entry.domain,
            crab_proxy::DomainUsage {
                tokens: entry.tokens,
                spend_usd: entry.spend_usd,
            },
        );
    }
    state.runtime.replace_domain_usage(map);
    Ok(StatusCode::NO_CONTENT)
}

async fn patch_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(token): Path<String>,
    Json(req): Json<PatchGatewayKeyRequest>,
) -> Result<Json<ApiKeySpec>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut entry = state.runtime.keys.get_mut(&token).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "key not found".to_string(),
            }),
        )
            .into_response()
    })?;

    if let Some(name) = req.name {
        entry.name = name;
    }
    if let Some(enabled) = req.enabled {
        entry.enabled = enabled;
    }
    if let Some(domain) = req.domain {
        entry.domain = Some(domain);
    }
    if let Some(project_id) = req.project_id {
        let pid = project_id.trim();
        if pid.is_empty() {
            entry.project_id = None;
        } else {
            entry.project_id = Some(crab_proxy::sanitize_user_id(pid).map_err(|e| {
                (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response()
            })?);
        }
    }
    if let Some(pipeline) = req.pipeline {
        let p = pipeline.trim();
        if p.is_empty() {
            entry.pipeline = None;
        } else {
            entry.pipeline = Some(p.to_string());
        }
    }
    if let Some(upstream_profile) = req.upstream_profile {
        let p = upstream_profile.trim();
        if p.is_empty() {
            entry.upstream_profile = None;
        } else {
            entry.upstream_profile = Some(p.to_string());
        }
    }
    if let Some(max_concurrent) = req.max_concurrent {
        entry.max_concurrent = max_concurrent;
    }
    if let Some(rpm_limit) = req.rpm_limit {
        entry.rpm_limit = rpm_limit;
    }

    let synced = entry.clone();
    drop(entry);
    let spec = stored_to_spec(&token, &synced, false, &state.client_key_limiter);
    state.client_key_limiter.sync_key(&token, &synced);
    schedule_persist_state(&state);
    Ok(Json(spec))
}

async fn get_ttl(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<TtlConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let cfg = state.runtime.ttl.read();
    Ok(Json(TtlConfigView {
        default_ttl_secs: cfg.default_ttl_secs,
        model_overrides: cfg.model_overrides.clone(),
        consumer_overrides: cfg.consumer_overrides.clone(),
        consumer_model_overrides: cfg.consumer_model_overrides.clone(),
        stale_while_revalidate_ttl_secs: 0,
    }))
}

async fn put_ttl(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutTtlConfigRequest>,
) -> Result<Json<TtlConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut cfg = state.runtime.ttl.write();
    cfg.default_ttl_secs = req.default_ttl_secs;
    cfg.model_overrides = req.model_overrides.clone();
    cfg.consumer_overrides = req.consumer_overrides.clone();
    cfg.consumer_model_overrides = req.consumer_model_overrides.clone();
    let view = TtlConfigView {
        default_ttl_secs: cfg.default_ttl_secs,
        model_overrides: cfg.model_overrides.clone(),
        consumer_overrides: cfg.consumer_overrides.clone(),
        consumer_model_overrides: cfg.consumer_model_overrides.clone(),
        stale_while_revalidate_ttl_secs: 0,
    };
    drop(cfg);
    // Sync the ArcSwap in TieredCache so DynamicTtlExpiry picks up the change.
    state.tiered_cache.sync_ttl();
    schedule_persist_state(&state);
    Ok(Json(view))
}

async fn get_stream_cache(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<StreamCacheConfig>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(StreamCacheConfig {
        enabled: state.runtime.stream_cache_enabled(),
    }))
}

async fn put_stream_cache(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<StreamCacheConfig>,
) -> Result<Json<StreamCacheConfig>, Response> {
    authorize(&headers, &state.admin_key)?;
    state.runtime.set_stream_cache_enabled(req.enabled);
    schedule_persist_state(&state);
    Ok(Json(StreamCacheConfig {
        enabled: state.runtime.stream_cache_enabled(),
    }))
}

fn connection_runtime_view(conn: &crab_proxy::ConnectionConfig) -> ConnectionRuntimeView {
    ConnectionRuntimeView {
        tcp_keepalive_idle_secs: conn.tcp_keepalive_idle_secs.unwrap_or(60),
        tcp_keepalive_interval_secs: conn.tcp_keepalive_interval_secs.unwrap_or(10),
        tcp_keepalive_count: conn.tcp_keepalive_count.unwrap_or(3),
        idle_timeout_secs: conn.idle_timeout_secs.unwrap_or(90),
        h2_ping_interval_secs: conn.h2_ping_interval_secs.unwrap_or(30),
        h2_ping_timeout_secs: conn.h2_ping_timeout_secs.unwrap_or(0),
        upstream_force_http1: conn.upstream_force_http1,
        upstream_disable_keepalive: conn.upstream_disable_keepalive,
        upstream_tls_curves: conn.upstream_tls_curves.clone(),
        upstream_request_timeout_secs: conn.upstream_request_timeout_secs.unwrap_or(300),
        upstream_write_timeout_secs: conn.upstream_write_timeout_secs.unwrap_or(300),
        upstream_connection_timeout_secs: conn.upstream_connection_timeout_secs.unwrap_or(60),
    }
}

async fn get_connection_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<ConnectionRuntimeView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let conn = state.runtime.conn_config.read();
    Ok(Json(connection_runtime_view(&conn)))
}

async fn put_connection_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<ConnectionRuntimeView>,
) -> Result<Json<ConnectionRuntimeView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut conn = state.runtime.conn_config.read().as_ref().clone();
    conn.tcp_keepalive_idle_secs = Some(req.tcp_keepalive_idle_secs);
    conn.tcp_keepalive_interval_secs = Some(req.tcp_keepalive_interval_secs);
    conn.tcp_keepalive_count = Some(req.tcp_keepalive_count);
    conn.idle_timeout_secs = Some(req.idle_timeout_secs);
    conn.h2_ping_interval_secs = Some(req.h2_ping_interval_secs);
    conn.h2_ping_timeout_secs = Some(req.h2_ping_timeout_secs);
    conn.upstream_force_http1 = req.upstream_force_http1;
    conn.upstream_disable_keepalive = req.upstream_disable_keepalive;
    conn.upstream_tls_curves = req.upstream_tls_curves.clone();
    conn.upstream_request_timeout_secs = Some(req.upstream_request_timeout_secs);
    conn.upstream_write_timeout_secs = Some(req.upstream_write_timeout_secs);
    conn.upstream_connection_timeout_secs = Some(req.upstream_connection_timeout_secs);
    *state.runtime.conn_config.write() = Arc::new(conn);
    schedule_persist_state(&state);
    Ok(Json(req))
}

fn semantic_runtime_view(state: &SemanticRuntimeState) -> SemanticRuntimeView {
    SemanticRuntimeView {
        enabled: state.enabled,
        similarity_threshold: state.threshold as f64,
        min_query_chars: state.gate.min_query_chars,
        max_query_chars: state.gate.max_query_chars,
        embed_only_on_exact_miss: state.gate.embed_only_on_exact_miss,
    }
}

async fn get_limits_config(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<LimitsConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(LimitsConfigView {
        max_request_body_bytes: state.max_request_body_bytes.load(Ordering::Acquire),
        max_concurrent_requests: state.max_concurrent_requests,
        legacy_api_key_as_client_auth: state
            .runtime
            .legacy_api_key_as_client_auth
            .load(Ordering::Acquire),
        cors_enabled: state.cors_enabled.load(Ordering::Acquire),
    }))
}

async fn put_limits_config(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<LimitsConfigView>,
) -> Result<Json<LimitsConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    state
        .max_request_body_bytes
        .store(req.max_request_body_bytes, Ordering::Release);
    state
        .cors_enabled
        .store(req.cors_enabled, Ordering::Release);
    state
        .runtime
        .legacy_api_key_as_client_auth
        .store(req.legacy_api_key_as_client_auth, Ordering::Release);
    Ok(Json(LimitsConfigView {
        max_request_body_bytes: state.max_request_body_bytes.load(Ordering::Acquire),
        max_concurrent_requests: state.max_concurrent_requests,
        legacy_api_key_as_client_auth: state
            .runtime
            .legacy_api_key_as_client_auth
            .load(Ordering::Acquire),
        cors_enabled: state.cors_enabled.load(Ordering::Acquire),
    }))
}

async fn get_pricing_config(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<PricingConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let pricing = state.pricing.read();
    Ok(Json(pricing_view(&pricing)))
}

async fn put_pricing_config(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PricingConfigView>,
) -> Result<Json<PricingConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut pricing = state.pricing.write();
    pricing.default_input_price_per_million = req.default_input_price_per_million;
    pricing.default_output_price_per_million = req.default_output_price_per_million;
    pricing.model_overrides = req
        .model_overrides
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                crab_proxy::ModelPricing {
                    input_price_per_million: v.input,
                    output_price_per_million: v.output,
                },
            )
        })
        .collect();
    Ok(Json(pricing_view(&pricing)))
}

fn pricing_view(p: &PricingConfig) -> PricingConfigView {
    PricingConfigView {
        default_input_price_per_million: p.default_input_price_per_million,
        default_output_price_per_million: p.default_output_price_per_million,
        model_overrides: p
            .model_overrides
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    ModelPricingView {
                        input: v.input_price_per_million,
                        output: v.output_price_per_million,
                    },
                )
            })
            .collect(),
    }
}

async fn get_features_config(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<FeaturesConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let f = state.features.read();
    Ok(Json(features_view(&f)))
}

async fn put_features_config(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<FeaturesConfigView>,
) -> Result<Json<FeaturesConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut f = state.features.write();
    f.prefix_aware_cache = req.prefix_aware_cache;
    f.streaming_body_forward = req.streaming_body_forward;
    f.connection_prewarm = req.connection_prewarm;
    f.affinity_prompt_cache_feedback = req.affinity_prompt_cache_feedback;
    f.delta_cache = req.delta_cache;
    f.io_uring_backend = req.io_uring_backend;
    f.wasm_filters = req.wasm_filters;
    f.mimo_context_compression = req.mimo_context_compression;
    f.mimo_compression_threshold = req.mimo_compression_threshold;
    f.upstream_request_gzip = req.upstream_request_gzip;
    f.upstream_request_gzip_min_bytes = req.upstream_request_gzip_min_bytes;
    f.mimo_retire_prefix_messages = req.mimo_retire_prefix_messages;
    f.mimo_keep_recent_turns = req.mimo_keep_recent_turns;
    f.mimo_session_store = req.mimo_session_store;
    f.mimo_session_store_ttl_secs = req.mimo_session_store_ttl_secs;
    f.mimo_session_store_max_messages = req.mimo_session_store_max_messages;
    f.passthrough_prefix_bytes = req.passthrough_prefix_bytes;
    // P1-1: Multi-factor routing
    f.backend_route_strategy = BackendRouteStrategy::from_str(&req.backend_route_strategy);
    f.backend_load_aware_routing_enabled = req.backend_load_aware_routing_enabled;
    f.backend_concurrency_limit_enabled = req.backend_concurrency_limit_enabled;
    f.default_max_inflight_per_backend = req.default_max_inflight_per_backend;
    f.backend_prefill_overload_threshold_ms = req.backend_prefill_overload_threshold_ms;
    f.backend_overload_cooldown_ms = req.backend_overload_cooldown_ms;
    f.score_weights = ScoreWeightsConfig {
        health: req.score_weights.health,
        latency_inv: req.score_weights.latency_inv,
        load_inv: req.score_weights.load_inv,
        affinity_hit: req.score_weights.affinity_hit,
        rate_429_inv: req.score_weights.rate_429_inv,
    };
    // P1-2: Quota Preflight
    f.preflight = PreflightConfig {
        enabled: req.preflight.enabled,
        cooldown_ms: req.preflight.cooldown_ms,
        max_consecutive_429: req.preflight.max_consecutive_429,
        skip_threshold: req.preflight.skip_threshold,
    };
    Ok(Json(features_view(&f)))
}

fn features_view(f: &FeaturesConfig) -> FeaturesConfigView {
    FeaturesConfigView {
        prefix_aware_cache: f.prefix_aware_cache,
        streaming_body_forward: f.streaming_body_forward,
        connection_prewarm: f.connection_prewarm,
        affinity_prompt_cache_feedback: f.affinity_prompt_cache_feedback,
        delta_cache: f.delta_cache,
        io_uring_backend: f.io_uring_backend,
        wasm_filters: f.wasm_filters,
        mimo_context_compression: f.mimo_context_compression,
        mimo_compression_threshold: f.mimo_compression_threshold,
        upstream_request_gzip: f.upstream_request_gzip,
        upstream_request_gzip_min_bytes: f.upstream_request_gzip_min_bytes,
        mimo_retire_prefix_messages: f.mimo_retire_prefix_messages,
        mimo_keep_recent_turns: f.mimo_keep_recent_turns,
        mimo_session_store: f.mimo_session_store,
        mimo_session_store_ttl_secs: f.mimo_session_store_ttl_secs,
        mimo_session_store_max_messages: f.mimo_session_store_max_messages,
        passthrough_prefix_bytes: f.passthrough_prefix_bytes,
        // P1-1: Multi-factor routing
        backend_route_strategy: f.backend_route_strategy.as_str().to_string(),
        backend_load_aware_routing_enabled: f.backend_load_aware_routing_enabled,
        backend_concurrency_limit_enabled: f.backend_concurrency_limit_enabled,
        default_max_inflight_per_backend: f.default_max_inflight_per_backend,
        backend_prefill_overload_threshold_ms: f.backend_prefill_overload_threshold_ms,
        backend_overload_cooldown_ms: f.backend_overload_cooldown_ms,
        score_weights: ScoreWeightsView {
            health: f.score_weights.health,
            latency_inv: f.score_weights.latency_inv,
            load_inv: f.score_weights.load_inv,
            affinity_hit: f.score_weights.affinity_hit,
            rate_429_inv: f.score_weights.rate_429_inv,
        },
        // P1-2: Quota Preflight
        preflight: PreflightView {
            enabled: f.preflight.enabled,
            cooldown_ms: f.preflight.cooldown_ms,
            max_consecutive_429: f.preflight.max_consecutive_429,
            skip_threshold: f.preflight.skip_threshold,
        },
    }
}

async fn get_semantic_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<SemanticRuntimeView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let runtime = state.semantic_runtime.read();
    Ok(Json(semantic_runtime_view(&runtime)))
}

async fn put_semantic_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<SemanticRuntimeView>,
) -> Result<Json<SemanticRuntimeView>, Response> {
    authorize(&headers, &state.admin_key)?;
    if req.enabled && state.semantic_cache.is_none() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "semantic cache is not enabled at gateway startup ([semantic].enabled); restart gateway to enable L2"
                    .to_string(),
            }),
        )
            .into_response());
    }
    if !(0.0..=1.0).contains(&req.similarity_threshold) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "similarity_threshold must be between 0 and 1".to_string(),
            }),
        )
            .into_response());
    }
    {
        let mut runtime = state.semantic_runtime.write();
        runtime.enabled = req.enabled;
        runtime.threshold = req.similarity_threshold as f32;
        runtime.gate = crab_semantic::SemanticGateConfig {
            min_query_chars: req.min_query_chars,
            max_query_chars: req.max_query_chars,
            embed_only_on_exact_miss: req.embed_only_on_exact_miss,
        };
    }
    if let Some(cache) = &state.semantic_cache {
        cache.set_threshold(req.similarity_threshold as f32);
    }
    Ok(Json(SemanticRuntimeView {
        enabled: req.enabled,
        similarity_threshold: req.similarity_threshold,
        min_query_chars: req.min_query_chars,
        max_query_chars: req.max_query_chars,
        embed_only_on_exact_miss: req.embed_only_on_exact_miss,
    }))
}

fn pipeline_runtime_view(runtime: &RuntimeConfig) -> PipelineRuntimeConfigView {
    let globals = runtime.pipeline_globals();
    let profiles = runtime
        .profile_descriptors()
        .into_iter()
        .map(|d| {
            let detail = runtime.profile(&d.id);
            PipelineProfileView {
                id: d.id,
                provider: d.provider.as_str().to_string(),
                base_url: detail
                    .as_ref()
                    .map(|p| p.base_url.clone())
                    .unwrap_or_default(),
                fallback_model: detail
                    .as_ref()
                    .map(|p| p.fallback_model.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();
    PipelineRuntimeConfigView {
        pipeline_mode: globals.pipeline_mode.as_str().to_string(),
        default_upstream_profile: runtime.default_upstream_profile_id(),
        profiles,
    }
}

async fn get_pipeline_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<PipelineRuntimeConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(pipeline_runtime_view(&state.runtime)))
}

async fn put_pipeline_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PipelineRuntimeConfigView>,
) -> Result<Json<PipelineRuntimeConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;

    let mode = req.pipeline_mode.trim();
    if mode != "auto" && mode != "force_cursor_v4" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "pipeline_mode must be \"auto\" or \"force_cursor_v4\"".to_string(),
            }),
        )
            .into_response());
    }

    let default_profile = req.default_upstream_profile.trim();
    if default_profile.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "default_upstream_profile must not be empty".to_string(),
            }),
        )
            .into_response());
    }

    state
        .runtime
        .set_pipeline_runtime(PipelineMode::from_str(mode), default_profile)
        .map_err(|msg| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: msg.to_string(),
                }),
            )
                .into_response()
        })?;

    tracing::info!(
        pipeline_mode = %mode,
        default_upstream_profile = %default_profile,
        "Pipeline runtime config updated"
    );

    schedule_persist_state(&state);
    Ok(Json(pipeline_runtime_view(&state.runtime)))
}

// ── Pipeline Rules CRUD ───────────────────────────────────────────────

fn rule_engine_to_view(engine: &PipelineRuleEngine) -> PipelineRulesConfigView {
    PipelineRulesConfigView {
        rules: engine
            .rules()
            .iter()
            .map(|r| PipelineRuleView {
                name: r.name.clone(),
                priority: r.priority,
                pipeline: r.pipeline.as_str().to_string(),
                match_conditions: PipelineRuleMatchView {
                    client: r.match_conditions.client.as_ref().map(|v| {
                        v.iter().map(|c| c.as_str().to_string()).collect()
                    }),
                    provider: r.match_conditions.provider.as_ref().map(|v| {
                        v.iter().map(|p| p.as_str().to_string()).collect()
                    }),
                    model_pattern: r.match_conditions.model_pattern.clone(),
                    wire_format: r.match_conditions.wire_format.as_ref().map(|v| {
                        v.iter().map(|f| f.as_str().to_string()).collect()
                    }),
                },
            })
            .collect(),
    }
}

fn view_to_rule_engine(view: &PipelineRulesConfigView) -> Result<PipelineRuleEngine, String> {
    use crab_pipeline::{PipelineMatchConditions, PipelineRule, RequestPipeline, UpstreamProvider};
    let rules: Vec<PipelineRule> = view
        .rules
        .iter()
        .map(|r| {
            let client = r.match_conditions.client.as_ref().map(|v| {
                v.iter()
                    .map(|s| match s.to_lowercase().as_str() {
                        "cursor" => ClientKind::Cursor,
                        "codex" => ClientKind::Codex,
                        "windsurf" => ClientKind::Windsurf,
                        "aider" => ClientKind::Aider,
                        "continue" => ClientKind::Continue,
                        _ => ClientKind::Generic,
                    })
                    .collect()
            });
            let provider = r.match_conditions.provider.as_ref().map(|v| {
                v.iter()
                    .map(|s| UpstreamProvider::from_str(s))
                    .collect()
            });
            let wire_format = r.match_conditions.wire_format.as_ref().map(|v| {
                v.iter().map(|s| WireFormat::from_str(s)).collect()
            });
            Ok(PipelineRule {
                name: r.name.clone(),
                priority: r.priority,
                match_conditions: PipelineMatchConditions {
                    client,
                    provider,
                    model_pattern: r.match_conditions.model_pattern.clone(),
                    wire_format,
                },
                pipeline: RequestPipeline::from_str(&r.pipeline),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(PipelineRuleEngine::new(rules))
}

async fn get_pipeline_rules(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<PipelineRulesConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let view = match state.runtime.rule_engine() {
        Some(engine) => rule_engine_to_view(&engine),
        None => PipelineRulesConfigView { rules: vec![] },
    };
    Ok(Json(view))
}

async fn put_pipeline_rules(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PipelineRulesConfigView>,
) -> Result<Json<PipelineRulesConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let engine = view_to_rule_engine(&req).map_err(|msg| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })).into_response()
    })?;
    let rule_count = engine.rules().len();
    state.runtime.set_rule_engine(Some(engine));
    tracing::info!(rule_count, "Pipeline rules updated");
    schedule_persist_state(&state);
    Ok(Json(req))
}

async fn post_pipeline_test(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PipelineTestRequest>,
) -> Result<Json<PipelineTestResponse>, Response> {
    authorize(&headers, &state.admin_key)?;
    let client_kind = req
        .client
        .as_deref()
        .map(|s| match s.to_lowercase().as_str() {
            "cursor" => ClientKind::Cursor,
            "codex" => ClientKind::Codex,
            "windsurf" => ClientKind::Windsurf,
            "aider" => ClientKind::Aider,
            "continue" => ClientKind::Continue,
            _ => ClientKind::Generic,
        })
        .unwrap_or(ClientKind::Generic);
    let provider = req
        .provider
        .as_deref()
        .map(crab_pipeline::UpstreamProvider::from_str)
        .unwrap_or(crab_pipeline::UpstreamProvider::Other);
    let wire_format = req
        .wire_format
        .as_deref()
        .map(WireFormat::from_str)
        .unwrap_or(WireFormat::ChatCompletions);
    let globals = state.runtime.pipeline_globals();
    if let Some(engine) = &globals.rule_engine {
        let input = crab_pipeline::RuleMatchInput {
            client_kind,
            provider,
            model: &req.model,
            wire_format,
        };
        if let Some((pipeline, rule_name)) = engine.select(&input) {
            return Ok(Json(PipelineTestResponse {
                matched: true,
                rule_name: Some(rule_name.to_string()),
                pipeline: Some(pipeline.as_str().to_string()),
            }));
        }
    }
    Ok(Json(PipelineTestResponse {
        matched: false,
        rule_name: None,
        pipeline: None,
    }))
}

fn cursor_models_view(runtime: &RuntimeConfig) -> CursorModelsConfigView {
    let cfg = runtime.cursor_models();
    CursorModelsConfigView {
        force_deepseek_profile_for_aliases: cfg.force_deepseek_profile_for_aliases,
        synthetic_models_enabled: cfg.synthetic_models_enabled,
        aliases: cfg
            .aliases
            .iter()
            .map(|(id, e)| {
                (
                    id.clone(),
                    CursorModelAliasView {
                        upstream: e.upstream.clone(),
                        pipeline: e.pipeline.as_str().to_string(),
                    },
                )
            })
            .collect(),
    }
}

fn cursor_models_from_view(view: &CursorModelsConfigView) -> Result<CursorModelsConfig, String> {
    let aliases = view
        .aliases
        .iter()
        .map(|(id, a)| {
            (
                id.clone(),
                CursorModelEntry {
                    upstream: a.upstream.clone(),
                    pipeline: PipelineOverride::from_str(&a.pipeline),
                },
            )
        })
        .collect();
    let cfg = CursorModelsConfig {
        aliases,
        force_deepseek_profile_for_aliases: view.force_deepseek_profile_for_aliases,
        synthetic_models_enabled: view.synthetic_models_enabled,
    };
    validate_cursor_models(&cfg)?;
    Ok(cfg)
}

async fn get_cursor_models(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<CursorModelsConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(cursor_models_view(&state.runtime)))
}

async fn put_cursor_models(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<CursorModelsConfigView>,
) -> Result<Json<CursorModelsConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let cfg = cursor_models_from_view(&req).map_err(|msg| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })).into_response()
    })?;
    state.runtime.set_cursor_models(cfg).map_err(|msg| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })).into_response()
    })?;
    tracing::info!(
        alias_count = req.aliases.len(),
        synthetic = req.synthetic_models_enabled,
        "Cursor model aliases updated"
    );
    schedule_persist_state(&state);
    Ok(Json(cursor_models_view(&state.runtime)))
}

fn mask_redis_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(at) = trimmed.find('@') {
        let scheme_end = trimmed.find("://").map(|i| i + 3).unwrap_or(0);
        let scheme_end = scheme_end.min(at);
        let host_start = (at + 1).min(trimmed.len());
        format!("{}***@{}", &trimmed[..scheme_end], &trimmed[host_start..])
    } else {
        trimmed.to_string()
    }
}

fn reasoning_runtime_view(config: &ReasoningConfig) -> ReasoningRuntimeConfigView {
    ReasoningRuntimeConfigView {
        thinking_mode: config.thinking_mode.clone(),
        reasoning_effort: config.reasoning_effort.clone(),
        missing_reasoning_strategy: config.missing_reasoning_strategy.clone(),
        display_reasoning: config.display_reasoning,
        collapsible_reasoning: config.collapsible_reasoning,
        cache_invalidate_recommended: false,
        storage_backend: Some(config.backend.clone()),
        cache_db_path: Some(config.cache_db_path.clone()),
        redis_url_masked: config
            .redis_url
            .as_ref()
            .map(|u| mask_redis_url(u))
            .filter(|s| !s.is_empty()),
    }
}

async fn get_reasoning_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<ReasoningRuntimeConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let cfg = state.reasoning_config.read();
    Ok(Json(reasoning_runtime_view(&cfg)))
}

async fn put_reasoning_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<ReasoningRuntimeConfigView>,
) -> Result<Json<ReasoningRuntimeConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;

    if req.thinking_mode != "enabled"
        && req.thinking_mode != "disabled"
        && req.thinking_mode != "auto"
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "thinking_mode must be \"auto\", \"enabled\", or \"disabled\"".to_string(),
            }),
        )
            .into_response());
    }
    let thinking_mode = if req.thinking_mode == "auto" {
        "enabled".to_string()
    } else {
        req.thinking_mode.clone()
    };
    if req.missing_reasoning_strategy != "recover" && req.missing_reasoning_strategy != "reject" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "missing_reasoning_strategy must be \"recover\" or \"reject\" (deepseek-cursor-proxy)"
                    .to_string(),
            }),
        )
            .into_response());
    }

    let mut cfg = state.reasoning_config.write();
    let display_reasoning_changed = cfg.display_reasoning != req.display_reasoning;
    cfg.thinking_mode = thinking_mode;
    cfg.reasoning_effort = req.reasoning_effort;
    cfg.missing_reasoning_strategy = req.missing_reasoning_strategy;
    cfg.display_reasoning = req.display_reasoning;
    cfg.collapsible_reasoning = req.collapsible_reasoning;

    if display_reasoning_changed {
        tracing::warn!(
            display_reasoning = cfg.display_reasoning,
            "display_reasoning changed: invalidate L0/L1 cache (POST /v1/cache/invalidate scope=all) \
             or bump fingerprint to avoid stale SSE/content"
        );
    }

    tracing::info!(
        thinking_mode = %cfg.thinking_mode,
        missing_reasoning_strategy = %cfg.missing_reasoning_strategy,
        "Reasoning runtime config updated"
    );

    let mut view = reasoning_runtime_view(&cfg);
    view.cache_invalidate_recommended = display_reasoning_changed;
    Ok(Json(view))
}

async fn clear_reasoning_cache(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<ClearReasoningCacheResponse>, Response> {
    authorize(&headers, &state.admin_key)?;
    let deleted = state
        .reasoning_store
        .clear()
        .map_err(|e| internal_error(&e.to_string()))?;
    tracing::info!(deleted, "Reasoning cache cleared via management API");
    Ok(Json(ClearReasoningCacheResponse { deleted }))
}

fn backend_to_spec(
    name: &str,
    addr: &std::net::SocketAddr,
    tls_sni: &str,
    healthy: bool,
) -> BackendSpec {
    BackendSpec {
        name: name.to_string(),
        addr: addr.to_string(),
        weight: 1,
        tls_sni: tls_sni.to_string(),
        healthy,
        last_check_ms: 0,
        latency_ms: 0,
    }
}

async fn get_routing_summary(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<RoutingSummaryView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile = state.runtime.default_profile();
    let router = &profile.router;

    // Query Pingora's built-in health map instead of our custom backend_health.
    let backends_meta = router.meta();

    let backends_total = backends_meta.len();
    let mut backends_healthy = 0usize;
    let mut backends_unhealthy = 0usize;
    for (addr, _meta) in backends_meta.iter() {
        // Check Pingora's health map using the backend's hash key.
        let pb = pingora_load_balancing::Backend {
            addr: pingora_core::protocols::l4::socket::SocketAddr::Inet(*addr),
            weight: 1,
            ext: pingora_load_balancing::Extensions::new(),
        };
        if router.backends().backends().ready(&pb) {
            backends_healthy += 1;
        } else {
            backends_unhealthy += 1;
        }
    }

    let pool = profile.resolve_upstream_pool();
    let pool_status = pool.list_status();
    // Keep this consistent with upstream pool acquire() behavior.
    let upstream_keys_available = pool.available_count();

    Ok(Json(RoutingSummaryView {
        backends_healthy,
        backends_total,
        backends_unhealthy,
        upstream_keys_available,
        upstream_keys_total: pool_status.len(),
        profile_id: profile.id.clone(),
    }))
}

async fn get_backends(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<RoutingBackendsView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile = state.runtime.default_profile();
    let router = &profile.router;
    let backends = router
        .meta()
        .iter()
        .map(|(addr, meta)| {
            let pb = pingora_load_balancing::Backend {
                addr: pingora_core::protocols::l4::socket::SocketAddr::Inet(*addr),
                weight: 1,
                ext: pingora_load_balancing::Extensions::new(),
            };
            let healthy = router.backends().backends().ready(&pb);
            backend_to_spec(&meta.name, addr, &meta.tls_sni, healthy)
        })
        .collect();
    Ok(Json(RoutingBackendsView { backends }))
}

async fn put_backends(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutBackendsRequest>,
) -> Result<Json<RoutingBackendsView>, Response> {
    authorize(&headers, &state.admin_key)?;

    let parsed = parse_backend_endpoints(&req.endpoints, req.default_weight, &req.tls_sni)
        .map_err(|errors| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorResponse {
                    error: errors.join("; "),
                }),
            )
                .into_response()
        })?;

    let mut router = state.runtime.router.write();
    router.rebuild(&parsed).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()
    })?;

    let backends = router
        .meta()
        .iter()
        .map(|(addr, meta)| backend_to_spec(&meta.name, addr, &meta.tls_sni, true))
        .collect();
    drop(router);
    schedule_persist_state(&state);
    Ok(Json(RoutingBackendsView { backends }))
}

/// Trigger a graceful restart of the gateway process.
/// Returns 200 OK, then spawns a task that calls std::process::exit(0)
/// after a short delay to allow the response to be sent.
/// Rejects requests made within 60 seconds of a previous restart request.
async fn restart_gateway_handler(
    headers: HeaderMap,
    State(state): State<ManagementState>,
) -> Result<Json<serde_json::Value>, Response> {
    authorize(&headers, &state.admin_key)?;

    static LAST_RESTART_SECS: AtomicU64 = AtomicU64::new(0);
    const RESTART_COOLDOWN_SECS: u64 = 60;

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let prev = LAST_RESTART_SECS.load(Ordering::SeqCst);
    if prev != 0 && now_secs.saturating_sub(prev) < RESTART_COOLDOWN_SECS {
        let remaining = RESTART_COOLDOWN_SECS - now_secs.saturating_sub(prev);
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorResponse {
                error: format!(
                    "restart cooldown active, retry after {}s",
                    remaining
                ),
            }),
        )
            .into_response());
    }

    LAST_RESTART_SECS.store(now_secs, Ordering::SeqCst);
    tracing::info!("Gateway restart requested via management API");
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });

    Ok(Json(serde_json::json!({"status": "restarting"})))
}

/// GET /v1/state/snapshot — Return the current control-plane state as JSON.
/// Used by Admin for periodic PG persistence (gateway_state_sync).
async fn get_state_snapshot(
    headers: HeaderMap,
    State(state): State<ManagementState>,
) -> Result<Json<serde_json::Value>, Response> {
    authorize(&headers, &state.admin_key)?;

    let snap = crab_state::build_snapshot_from_runtime(&state.runtime);
    // Mask sensitive key_hash fields and token map keys in the snapshot
    let keys_json = {
        let mut val = serde_json::to_value(&snap.keys).unwrap_or_default();
        if let Some(map) = val.as_object_mut() {
            // Collect keys to mask (can't mutate map while iterating keys)
            let keys_to_mask: Vec<String> = map.keys().cloned().collect();
            // Track used masked keys to avoid collisions
            let mut used_masked_keys = std::collections::HashSet::new();
            for token_key in keys_to_mask {
                // Mask the map key (which is the full token)
                let base_masked = if token_key.len() > 8 {
                    format!("{}****", &token_key[..4])
                } else if !token_key.is_empty() {
                    "****".to_string()
                } else {
                    continue;
                };
                // Append suffix on collision to avoid overwriting entries
                let mut masked_key = base_masked.clone();
                let mut suffix = 2u32;
                while used_masked_keys.contains(&masked_key) {
                    masked_key = format!("{}-{}", &base_masked, suffix);
                    suffix += 1;
                }
                used_masked_keys.insert(masked_key.clone());
                if let Some(key_obj) = map.remove(&token_key) {
                    // Mask key_hash field inside the value
                    let mut key_obj = key_obj;
                    if let Some(obj) = key_obj.as_object_mut() {
                        if let Some(hash) = obj.get_mut("key_hash") {
                            if let Some(s) = hash.as_str() {
                                if s.len() > 8 {
                                    *hash = serde_json::Value::String(format!("{}****", &s[..4]));
                                }
                            }
                        }
                    }
                    map.insert(masked_key, key_obj);
                }
            }
        }
        val
    };
    let runtime_json = serde_json::to_value(&snap.runtime).unwrap_or_default();
    let profiles_json = serde_json::to_value(&snap.upstream_profiles).unwrap_or_default();
    let key_states_json = serde_json::to_value(&snap.key_states).unwrap_or_default();
    let domain_policies_json = serde_json::to_value(&snap.domain_policies).unwrap_or_default();

    Ok(Json(serde_json::json!({
        "keys": keys_json,
        "runtime": runtime_json,
        "profiles": profiles_json,
        "key_states": key_states_json,
        "domain_policies": domain_policies_json,
    })))
}

pub(crate) fn internal_error(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
        .into_response()
}

pub async fn serve(listen_addr: &str, state: ManagementState) -> anyhow::Result<()> {
    let router = router(state);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    tracing::info!(addr = %listen_addr, "Management API listening");
    axum::serve(listener, router).await?;
    Ok(())
}

/// Like [`serve`], but gracefully stops when the Pingora shutdown signal fires.
pub async fn serve_with_shutdown(
    listen_addr: &str,
    state: ManagementState,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let router = router(state);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    tracing::info!(addr = %listen_addr, "Management API listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        })
        .await?;
    Ok(())
}

// ── Fault Injection handlers (debug/test builds) ──────────────────────

async fn get_fault_injection(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<crab_proxy::fault_injection::FaultInjectionSnapshot>, Response> {
    authorize(&headers, &state.admin_key)?;
    if !cfg!(debug_assertions) && !cfg!(feature = "fault-injection") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "fault injection is only available in debug builds".to_string(),
            }),
        )
            .into_response()
            .into());
    }
    Ok(Json(state.fault_injection.snapshot()))
}

async fn put_fault_injection(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<crab_proxy::fault_injection::FaultInjectionSnapshot>,
) -> Result<Json<crab_proxy::fault_injection::FaultInjectionSnapshot>, Response> {
    authorize(&headers, &state.admin_key)?;
    if !cfg!(debug_assertions) && !cfg!(feature = "fault-injection") {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "fault injection is only available in debug builds".to_string(),
            }),
        )
            .into_response()
            .into());
    }
    let fi = &state.fault_injection;
    use std::sync::atomic::Ordering;
    fi.redis_down.store(req.redis_down, Ordering::Relaxed);
    fi.force_upstream_429
        .store(req.force_upstream_429, Ordering::Relaxed);
    fi.upstream_delay_ms
        .store(req.upstream_delay_ms, Ordering::Relaxed);
    fi.corrupt_l0_cache
        .store(req.corrupt_l0_cache, Ordering::Relaxed);
    fi.force_coalesce_leader_fail
        .store(req.force_coalesce_leader_fail, Ordering::Relaxed);
    fi.force_connection_fail
        .store(req.force_connection_fail, Ordering::Relaxed);
    fi.trigger_after_count
        .store(req.trigger_after_count, Ordering::Relaxed);
    tracing::info!(?req, "Fault injection updated");
    Ok(Json(fi.snapshot()))
}

async fn delete_fault_injection(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<crab_proxy::fault_injection::FaultInjectionSnapshot>, Response> {
    authorize(&headers, &state.admin_key)?;
    state.fault_injection.reset();
    tracing::info!("Fault injection cleared");
    Ok(Json(state.fault_injection.snapshot()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn require_confirm_rejects_all_without_header() {
        let action = InvalidateAction::All;
        let headers = HeaderMap::new();
        assert!(require_invalidate_confirm(&action, &headers).is_err());
    }

    #[test]
    fn require_confirm_accepts_all_with_header() {
        let action = InvalidateAction::All;
        let mut headers = HeaderMap::new();
        headers.insert(
            CACHE_INVALIDATE_CONFIRM_HEADER,
            HeaderValue::from_static(CACHE_INVALIDATE_CONFIRM_ALL),
        );
        assert!(require_invalidate_confirm(&action, &headers).is_ok());
    }

    #[test]
    fn require_confirm_ignores_key_scope() {
        let action = InvalidateAction::Key("abc".to_string());
        let headers = HeaderMap::new();
        assert!(require_invalidate_confirm(&action, &headers).is_ok());
    }

    #[test]
    fn invalidate_rate_limits_burst() {
        let mut rate = InvalidateRateState::default();
        for _ in 0..INVALIDATE_MAX_PER_WINDOW {
            assert!(rate.check(false).is_ok());
        }
        assert!(rate.check(false).is_err());
    }
}
