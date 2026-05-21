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
    ClearReasoningCacheResponse,     CreateGatewayKeyRequest, CreateGatewayKeyResponse, DomainPolicySpec, ErrorResponse,
    GATEWAY_ADMIN_KEY_HEADER, GatewayStatus, PatchGatewayKeyRequest, PatchUpstreamKeyRequest,
    PutBackendsRequest, PutDomainPoliciesRequest, PutTtlConfigRequest, PutUpstreamKeysRequest,
    PutUpstreamRelayConfigRequest,
    CursorModelAliasView, CursorModelsConfigView, PipelineProfileView, PipelineRuntimeConfigView,
    ReasoningRuntimeConfigView,
    RoutingBackendsView, StreamCacheConfig, TtlConfigView,
    UpstreamKeyView, UpstreamKeysPutMode, UpstreamKeysView, UpstreamRelayConfigView,
    parse_backend_endpoints, parse_upstream_base_url,
};
use crab_pipeline::{
    validate_cursor_models, CursorModelEntry, CursorModelsConfig, PipelineMode, PipelineOverride,
};
use crab_proxy::{
    DomainPolicy, ReasoningConfig, RuntimeConfig, StoredKey, UpstreamKeyPool, UpstreamKeySpec,
};
use std::collections::HashMap;
use crab_reasoning::ReasoningBackend;
use crab_state::{RedisStateStore, persist_runtime_state_with_retry};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
        if is_all {
            if let Some(last) = self.last_all_at {
                if now.duration_since(last) < INVALIDATE_ALL_COOLDOWN {
                    return Err("full cache invalidation is rate limited to once per 60s");
                }
            }
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

    let job = state
        .invalidate_job
        .lock()
        .map_err(|_| internal_error("invalidate job lock poisoned"))?
        .clone();

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

pub(crate) enum InvalidateAction {
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
        let mut rate = state.invalidate_rate.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "invalidate rate limit lock poisoned".to_string(),
                }),
            )
                .into_response()
        })?;
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

    if let Ok(mut slot) = invalidate_job.lock() {
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

        if let Ok(mut slot) = invalidate_job.lock() {
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

    let cfg = state
        .runtime
        .fingerprint
        .read()
        .map_err(|_| internal_error("fingerprint lock poisoned"))?;

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

    let mut cfg = state
        .runtime
        .fingerprint
        .write()
        .map_err(|_| internal_error("fingerprint lock poisoned"))?;
    cfg.version = req.version;
    cfg.normalize_content = req.normalize_content;

    tracing::info!(
        version = req.version,
        normalize = req.normalize_content,
        "Fingerprint config updated"
    );

    let resp = FingerprintRequest {
        version: cfg.version,
        normalize_content: cfg.normalize_content,
    };
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
}

async fn ready(State(state): State<ManagementState>) -> (StatusCode, Json<ReadyResponse>) {
    if state.tiered_cache.ping().await {
        (
            StatusCode::OK,
            Json(ReadyResponse {
                ready: true,
                redis: "ok",
            }),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                ready: false,
                redis: "unavailable",
            }),
        )
    }
}

fn schedule_persist_state(state: &ManagementState) {
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

fn authorize(headers: &HeaderMap, expected: &str) -> Result<(), Response> {
    let provided = headers
        .get(GATEWAY_ADMIN_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided != expected {
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
    let backend_count = state
        .runtime
        .router
        .read()
        .map(|r| r.backends().len())
        .unwrap_or(0);
    let pool = state.runtime.upstream_pool();
    let upstream_base_url = state
        .runtime
        .upstream_base_url
        .read()
        .map(|u| u.clone())
        .ok();
    let upstream_model = state.runtime.fallback_model.read().map(|m| m.clone()).ok();
    Ok(Json(GatewayStatus {
        uptime_secs: state.runtime.uptime_secs(),
        active_keys: state.runtime.keys.len() as u64,
        backend_count,
        stream_cache_enabled: state.runtime.stream_cache_enabled(),
        upstream_key_count: pool.len(),
        upstream_keys_available: pool.available_count(),
        upstream_base_url,
        upstream_model,
    }))
}

fn upstream_relay_view(runtime: &RuntimeConfig) -> UpstreamRelayConfigView {
    let base_url = runtime
        .upstream_base_url
        .read()
        .map(|u| u.clone())
        .unwrap_or_else(|_| "https://api.deepseek.com".to_string());
    let model = runtime
        .fallback_model
        .read()
        .map(|m| m.clone())
        .unwrap_or_else(|_| "deepseek-v4-pro".to_string());
    UpstreamRelayConfigView { base_url, model }
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
        let mut base = state
            .runtime
            .upstream_base_url
            .write()
            .map_err(|_| internal_error("upstream_base_url lock poisoned"))?;
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
        let mut fallback = state
            .runtime
            .fallback_model
            .write()
            .map_err(|_| internal_error("fallback_model lock poisoned"))?;
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

    let mut router = state
        .runtime
        .router
        .write()
        .map_err(|_| internal_error("router lock poisoned"))?;
    router.update(&parsed_backends).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()
    })?;

    if let Ok(mut health) = state.runtime.backend_health.write() {
        health.clear();
        for b in router.backends() {
            health.insert(b.name.clone(), crab_route::BackendHealth::new_healthy());
        }
    }
    drop(router);

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
        })
        .collect();
    let current = state.runtime.upstream_pool();
    let new_pool = match req.mode {
        UpstreamKeysPutMode::Append => UpstreamKeyPool::merge_append(&current, specs),
        UpstreamKeysPutMode::Replace => UpstreamKeyPool::hot_replace(&current, specs),
    };
    state.runtime.replace_upstream_pool(new_pool);
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
    let pool = runtime.upstream_pool();
    UpstreamKeysView {
        keys: pool
            .list_status()
            .into_iter()
            .map(|k| UpstreamKeyView {
                id: k.id,
                preview: k.preview,
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

fn stored_to_spec(token: &str, key: &StoredKey, include_full: bool) -> ApiKeySpec {
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
        pipeline: key.pipeline.clone(),
        upstream_profile: key.upstream_profile.clone(),
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
        .map(|entry| stored_to_spec(entry.key(), entry.value(), false))
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

    let id = uuid::Uuid::new_v4().to_string();
    let stored = StoredKey {
        id: id.clone(),
        name: req.name.clone(),
        key_hash: token.clone(),
        enabled: req.enabled,
        domain: req.domain.clone(),
        pipeline: req.pipeline.clone(),
        upstream_profile: req.upstream_profile.clone(),
    };
    state.runtime.keys.insert(token.clone(), stored);

    schedule_persist_state(&state);
    Ok(Json(CreateGatewayKeyResponse {
        id,
        name: req.name,
        key_full: token.clone(),
        key_preview: key_preview(&token),
        enabled: req.enabled,
        domain: req.domain,
        pipeline: req.pipeline,
        upstream_profile: req.upstream_profile,
    }))
}

async fn revoke_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    if state.runtime.keys.remove(&token).is_some() {
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
    let mut map = HashMap::new();
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
    if let Ok(mut guard) = state.runtime.domain_policies.write() {
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
    if let Some(pipeline) = req.pipeline {
        entry.pipeline = Some(pipeline);
    }
    if let Some(upstream_profile) = req.upstream_profile {
        entry.upstream_profile = Some(upstream_profile);
    }

    let spec = stored_to_spec(&token, &entry, false);
    drop(entry);
    schedule_persist_state(&state);
    Ok(Json(spec))
}

async fn get_ttl(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<TtlConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let cfg = state
        .runtime
        .ttl
        .read()
        .map_err(|_| internal_error("TTL lock poisoned"))?;
    Ok(Json(TtlConfigView {
        default_ttl_secs: cfg.default_ttl_secs,
        model_overrides: cfg.model_overrides.clone(),
        consumer_overrides: cfg.consumer_overrides.clone(),
    }))
}

async fn put_ttl(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<PutTtlConfigRequest>,
) -> Result<Json<TtlConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let mut cfg = state
        .runtime
        .ttl
        .write()
        .map_err(|_| internal_error("TTL lock poisoned"))?;
    cfg.default_ttl_secs = req.default_ttl_secs;
    cfg.model_overrides = req.model_overrides.clone();
    cfg.consumer_overrides = req.consumer_overrides.clone();
    let view = TtlConfigView {
        default_ttl_secs: cfg.default_ttl_secs,
        model_overrides: cfg.model_overrides.clone(),
        consumer_overrides: cfg.consumer_overrides.clone(),
    };
    drop(cfg);
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

fn pipeline_runtime_view(runtime: &RuntimeConfig) -> PipelineRuntimeConfigView {
    let globals = runtime.pipeline_globals();
    let profiles = runtime
        .profile_descriptors()
        .into_iter()
        .map(|d| PipelineProfileView {
            id: d.id,
            provider: d.provider.as_str().to_string(),
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

fn reasoning_runtime_view(config: &ReasoningConfig) -> ReasoningRuntimeConfigView {
    ReasoningRuntimeConfigView {
        thinking_mode: config.thinking_mode.clone(),
        reasoning_effort: config.reasoning_effort.clone(),
        missing_reasoning_strategy: config.missing_reasoning_strategy.clone(),
        display_reasoning: config.display_reasoning,
        collapsible_reasoning: config.collapsible_reasoning,
    }
}

async fn get_reasoning_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<ReasoningRuntimeConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let cfg = state
        .reasoning_config
        .read()
        .map_err(|_| internal_error("reasoning config lock poisoned"))?;
    Ok(Json(reasoning_runtime_view(&cfg)))
}

async fn put_reasoning_runtime(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Json(req): Json<ReasoningRuntimeConfigView>,
) -> Result<Json<ReasoningRuntimeConfigView>, Response> {
    authorize(&headers, &state.admin_key)?;

    if req.thinking_mode != "enabled" && req.thinking_mode != "disabled" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "thinking_mode must be \"enabled\" or \"disabled\"".to_string(),
            }),
        )
            .into_response());
    }
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

    let mut cfg = state
        .reasoning_config
        .write()
        .map_err(|_| internal_error("reasoning config lock poisoned"))?;
    cfg.thinking_mode = req.thinking_mode;
    cfg.reasoning_effort = req.reasoning_effort;
    cfg.missing_reasoning_strategy = req.missing_reasoning_strategy;
    cfg.display_reasoning = req.display_reasoning;
    cfg.collapsible_reasoning = req.collapsible_reasoning;

    tracing::info!(
        thinking_mode = %cfg.thinking_mode,
        missing_reasoning_strategy = %cfg.missing_reasoning_strategy,
        "Reasoning runtime config updated"
    );

    Ok(Json(reasoning_runtime_view(&cfg)))
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

async fn get_backends(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<RoutingBackendsView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let router = state
        .runtime
        .router
        .read()
        .map_err(|_| internal_error("router lock poisoned"))?;
    let health = state
        .runtime
        .backend_health
        .read()
        .map_err(|_| internal_error("health lock poisoned"))?;
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

    let mut router = state
        .runtime
        .router
        .write()
        .map_err(|_| internal_error("router lock poisoned"))?;
    router.update(&parsed).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()
    })?;

    if let Ok(mut health) = state.runtime.backend_health.write() {
        health.clear();
        for b in router.backends() {
            health.insert(b.name.clone(), crab_route::BackendHealth::new_healthy());
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

fn internal_error(msg: &str) -> Response {
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
