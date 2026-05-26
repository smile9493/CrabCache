use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use crab_cache::{InvalidateScanOptions, TieredCache};
use crab_control::{
    ApiKeySpec, BackendSpec, CACHE_INVALIDATE_CONFIRM_ALL, CACHE_INVALIDATE_CONFIRM_HEADER,
    ClearReasoningCacheResponse, ConnectionRuntimeView, CreateGatewayKeyRequest,
    CreateGatewayKeyResponse, CursorModelAliasView, CursorModelsConfigView, DomainPolicySpec,
    ErrorResponse, GATEWAY_ADMIN_KEY_HEADER, GatewayStatus, PatchGatewayKeyRequest,
    PatchUpstreamKeyRequest, PipelineProfileView, PipelineRuntimeConfigView, PutBackendsRequest,
    PutDomainPoliciesRequest, PutTtlConfigRequest, PutUpstreamKeysRequest,
    PutUpstreamRelayConfigRequest, ReasoningRuntimeConfigView, RoutingBackendsView,
    RoutingSummaryView, SemanticRuntimeView, StreamCacheConfig, TtlConfigView, UpstreamKeyView,
    UpstreamKeysPutMode, UpstreamKeysView, UpstreamRelayConfigView, constant_time_eq_str,
    parse_backend_endpoints, parse_upstream_base_url,
};
use crab_pipeline::{
    CursorModelEntry, CursorModelsConfig, PipelineMode, PipelineOverride, validate_cursor_models,
};
use crab_proxy::{
    ClientKeyLimiter, DomainPolicy, ReasoningConfig, RuntimeConfig, StoredKey, UpstreamKeyPool,
    UpstreamKeySpec,
};
use crab_proxy::{SemanticRuntimeState, SharedSemanticRuntime};
use crab_reasoning::ReasoningBackend;
use crab_state::{RedisStateStore, persist_runtime_state_with_retry};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "management_profiles.rs"]
mod management_profiles;

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
    pub invalidate_all_in_progress: Arc<AtomicBool>,
    pub invalidate_job: Arc<Mutex<Option<InvalidateJobSnapshot>>>,
    pub invalidate_rate: Arc<Mutex<InvalidateRateState>>,
    pub invalidate_scan_timeout_secs: u64,
    pub client_key_limiter: Arc<ClientKeyLimiter>,
    pub upstream_key_cooldown_secs: u64,
    pub semantic_runtime: SharedSemanticRuntime,
    pub semantic_cache: Option<Arc<crab_semantic::SemanticCache>>,
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
        .route("/v1/keys", get(list_keys).post(create_key))
        .route("/v1/cache/invalidate", post(invalidate_cache))
        .route("/v1/cache/invalidate/status", get(get_invalidate_status))
        .route(
            "/v1/cache/fingerprint",
            get(get_fingerprint).put(put_fingerprint),
        )
        .route(
            "/v1/keys/by-id/{id}",
            delete(revoke_key_by_id).patch(patch_key_by_id),
        )
        .route("/v1/keys/{token}", delete(revoke_key).patch(patch_key))
        .route(
            "/v1/domains/policies",
            get(list_domain_policies).put(put_domain_policies),
        )
        .route(
            "/v1/domains/policies/{domain}",
            delete(delete_domain_policy),
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
            "/v1/runtime/semantic",
            get(get_semantic_runtime).put(put_semantic_runtime),
        )
        .route(
            "/v1/runtime/connection",
            get(get_connection_runtime).put(put_connection_runtime),
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
        .route("/v1/upstream/keys/{id}", patch(patch_upstream_key))
        .route(
            "/v1/upstream/relay",
            get(get_upstream_relay).put(put_upstream_relay),
        )
        .route(
            "/v1/upstream/profiles",
            get(management_profiles::list_upstream_profiles),
        )
        .route(
            "/v1/upstream/profiles/{id}",
            axum::routing::put(management_profiles::put_upstream_profile)
                .delete(management_profiles::delete_upstream_profile),
        )
        .route(
            "/v1/upstream/profiles/{id}/keys",
            get(management_profiles::get_profile_keys).put(management_profiles::put_profile_keys),
        )
        .route(
            "/v1/upstream/profiles/{id}/keys/{key_id}",
            patch(management_profiles::patch_profile_key),
        )
        .route(
            "/v1/upstream/profiles/{id}/keys/{key_id}/test",
            post(management_profiles::test_upstream_profile_key),
        )
        .route(
            "/v1/upstream/profiles/{id}/test",
            post(management_profiles::test_upstream_profile),
        )
        .route(
            "/v1/upstream/profiles/{id}/routing",
            get(management_profiles::get_profile_routing),
        )
        .route(
            "/v1/routing/summary",
            get(get_routing_summary),
        )
        .route("/v1/system/restart", post(restart_gateway_handler))
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

        if is_all {
            in_progress.store(false, Ordering::SeqCst);
        }

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
    let backend_count = state.runtime.router.read().backends().len();
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
        upstream_base_url,
        upstream_model,
    }))
}

fn upstream_relay_view(runtime: &RuntimeConfig) -> UpstreamRelayConfigView {
    let base_url = runtime.upstream_base_url.read().clone();
    let model = runtime.fallback_model.read().clone();
    let api_key = runtime.upstream_pool().admin_secret();
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
    router.update(&parsed_backends).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()
    })?;

    {
        let mut health = state.runtime.backend_health.write();
        let keep: std::collections::HashSet<String> =
            router.backends().iter().map(|b| b.name.clone()).collect();
        health.retain(|name, _| keep.contains(name));
        for b in router.backends() {
            health
                .entry(b.name.clone())
                .or_insert_with(crab_route::BackendHealth::new_healthy);
        }
    }
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

    // 2) Fan-out: sync the same specs to every other profile pool (inflight/cooldown isolated).
    let profile_ids: Vec<String> = {
        let profiles = state.runtime.upstream_profiles.read();
        profiles.keys().cloned().collect()
    };
    let mut profiles_updated = vec![default_id.clone()];
    for pid in &profile_ids {
        if *pid == default_id {
            continue;
        }
        if let Some(profile) = state.runtime.profile(pid) {
            let peer_current = profile.resolve_upstream_pool();
            let peer_pool = match req.mode {
                UpstreamKeysPutMode::Append => {
                    UpstreamKeyPool::merge_append(&peer_current, specs.clone())
                }
                UpstreamKeysPutMode::Replace => {
                    UpstreamKeyPool::hot_replace(&peer_current, specs.clone())
                }
            };
            if let Err(e) = state.runtime.replace_profile_pool(pid, peer_pool) {
                tracing::warn!(profile_id = %pid, error = %e, "Failed to sync keys to profile");
            } else {
                profiles_updated.push(pid.clone());
            }
        }
    }
    tracing::info!(profiles = ?profiles_updated, "Upstream keys synced to profile pools");

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
        // Fan-out: sync enabled status to all other profile pools.
        let profile_ids: Vec<String> = {
            let profiles = state.runtime.upstream_profiles.read();
            profiles.keys().cloned().collect()
        };
        let default_id = state.runtime.default_upstream_profile_id();
        for pid in &profile_ids {
            if *pid == default_id {
                continue;
            }
            if let Some(profile) = state.runtime.profile(pid) {
                let peer_pool = profile.resolve_upstream_pool();
                peer_pool.set_enabled(&id, enabled);
            }
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
        .map(|entry| stored_to_spec(entry.key(), entry.value(), false, &state.client_key_limiter))
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
    schedule_persist_state(&state);
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
        entry.pipeline = Some(pipeline);
    }
    if let Some(upstream_profile) = req.upstream_profile {
        entry.upstream_profile = Some(upstream_profile);
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
    *state.runtime.conn_config.write() = Arc::new(conn);
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
        format!("{}***@{}", &trimmed[..scheme_end], &trimmed[at + 1..])
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
    b: &crab_route::Backend,
    health: Option<&crab_route::BackendHealth>,
) -> BackendSpec {
    let (healthy, last_check_ms, latency_ms) = match health {
        Some(h) => (h.healthy, h.last_check_ms, h.latency_ms),
        None => (true, 0, 0),
    };
    BackendSpec {
        name: b.name.clone(),
        addr: b.addr.to_string(),
        weight: b.weight,
        tls_sni: b.tls_sni.clone(),
        healthy,
        last_check_ms,
        latency_ms,
    }
}

async fn get_routing_summary(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<RoutingSummaryView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile = state.runtime.default_profile();
    let router = &profile.router;
    let backends = router.backends();
    let health_map = state.runtime.backend_health.read();

    let backends_total = backends.len();
    let mut backends_healthy = 0usize;
    let mut circuit_open_count = 0usize;
    for b in backends {
        if let Some(h) = health_map.get(&b.name) {
            if h.healthy {
                backends_healthy += 1;
            }
            if h.circuit_state == crab_route::CircuitState::Open {
                circuit_open_count += 1;
            }
        } else {
            backends_healthy += 1;
        }
    }

    let pool = profile.resolve_upstream_pool();
    let pool_status = pool.list_status();
    let upstream_keys_available = pool_status
        .iter()
        .filter(|k| k.enabled && k.inflight == 0 && k.cooldown_remaining_secs == 0)
        .count();

    Ok(Json(RoutingSummaryView {
        backends_healthy,
        backends_total,
        circuit_open_count,
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
    let router = state.runtime.router.read();
    let health = state.runtime.backend_health.read();
    let backends = router
        .backends()
        .iter()
        .map(|b| backend_to_spec(b.as_ref(), health.get(&b.name)))
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
    router.update(&parsed).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()
    })?;

    {
        let mut health = state.runtime.backend_health.write();
        let keep: std::collections::HashSet<String> =
            router.backends().iter().map(|b| b.name.clone()).collect();
        health.retain(|name, _| keep.contains(name));
        for b in router.backends() {
            health
                .entry(b.name.clone())
                .or_insert_with(crab_route::BackendHealth::new_healthy);
        }
    }

    let backends = router
        .backends()
        .iter()
        .map(|b| backend_to_spec(b.as_ref(), None))
        .collect();
    drop(router);
    schedule_persist_state(&state);
    Ok(Json(RoutingBackendsView { backends }))
}

/// Trigger a graceful restart of the gateway process.
/// Returns 200 OK, then spawns a task that calls std::process::exit(0)
/// after a short delay to allow the response to be sent.
async fn restart_gateway_handler(
    headers: HeaderMap,
    State(state): State<ManagementState>,
) -> Result<Json<serde_json::Value>, Response> {
    authorize(&headers, &state.admin_key)?;

    tracing::info!("Gateway restart requested via management API");
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });

    Ok(Json(serde_json::json!({"status": "restarting"})))
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
