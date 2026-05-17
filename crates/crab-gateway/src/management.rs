use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use crab_cache::{InvalidateScanOptions, TieredCache};
use crab_control::{
    parse_backend_endpoints, ApiKeySpec, BackendSpec, CreateGatewayKeyRequest,
    CreateGatewayKeyResponse, ErrorResponse, GatewayStatus, PatchGatewayKeyRequest,
    PutBackendsRequest, PutTtlConfigRequest, RoutingBackendsView, StreamCacheConfig,
    CACHE_INVALIDATE_CONFIRM_ALL, CACHE_INVALIDATE_CONFIRM_HEADER, TtlConfigView,
    GATEWAY_ADMIN_KEY_HEADER,
};
use crab_proxy::{RuntimeConfig, StoredKey};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INVALIDATE_WINDOW: Duration = Duration::from_secs(60);
const INVALIDATE_MAX_PER_WINDOW: usize = 10;
const INVALIDATE_ALL_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct ManagementState {
    pub runtime: Arc<RuntimeConfig>,
    pub tiered_cache: Arc<TieredCache>,
    pub admin_key: String,
    pub invalidate_all_in_progress: Arc<AtomicBool>,
    pub invalidate_rate: Arc<Mutex<InvalidateRateState>>,
    pub invalidate_scan_timeout_secs: u64,
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
        .route("/v1/status", get(status))
        .route("/v1/keys", get(list_keys).post(create_key))
        .route("/v1/cache/invalidate", post(invalidate_cache))
        .route(
            "/v1/cache/fingerprint",
            get(get_fingerprint).put(put_fingerprint),
        )
        .route("/v1/keys/{token}", delete(revoke_key).patch(patch_key))
        .route("/v1/cache/ttl", get(get_ttl).put(put_ttl))
        .route("/v1/runtime/stream_cache", get(get_stream_cache).put(put_stream_cache))
        .route("/v1/routing/backends", get(get_backends).put(put_backends))
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
pub fn require_invalidate_confirm(action: &InvalidateAction, headers: &HeaderMap) -> Result<(), String> {
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

    let action = parse_invalidate_scope(&req.scope).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )
            .into_response()
    })?;

    require_invalidate_confirm(&action, &headers).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: e }),
        )
            .into_response()
    })?;

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
    let scope_label = req.scope.clone();
    let scan_opts = InvalidateScanOptions::from_timeout_secs(state.invalidate_scan_timeout_secs);

    tokio::spawn(async move {
        let result = match action {
            InvalidateAction::All => {
                tracing::info!(scope = %scope_label, "Starting full cache invalidation");
                let r = tiered_cache.invalidate_all(scan_opts).await;
                match &r {
                    Ok(_) => tracing::info!(scope = %scope_label, "Full cache invalidation completed"),
                    Err(e) => tracing::warn!(scope = %scope_label, error = %e, "Full cache invalidation failed"),
                }
                r
            }
            InvalidateAction::Prefix(p) => {
                tracing::info!(scope = %scope_label, prefix = %p, "Starting prefix cache invalidation");
                let r = tiered_cache.invalidate_prefix(&p, scan_opts).await;
                match &r {
                    Ok(_) => tracing::info!(scope = %scope_label, prefix = %p, "Prefix cache invalidation completed"),
                    Err(e) => tracing::warn!(scope = %scope_label, prefix = %p, error = %e, "Prefix cache invalidation failed"),
                }
                r
            }
            InvalidateAction::Key(k) => {
                tracing::info!(scope = %scope_label, key = %k, "Starting single key cache invalidation");
                let r = tiered_cache.invalidate(&k).await;
                match &r {
                    Ok(_) => tracing::info!(scope = %scope_label, key = %k, "Single key cache invalidation completed"),
                    Err(e) => tracing::warn!(scope = %scope_label, key = %k, error = %e, "Single key cache invalidation failed"),
                }
                r
            }
        };

        if is_all {
            in_progress.store(false, Ordering::SeqCst);
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

    Ok(Json(FingerprintRequest {
        version: cfg.version,
        normalize_content: cfg.normalize_content,
    }))
}

async fn health() -> StatusCode {
    StatusCode::OK
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
    Ok(Json(GatewayStatus {
        uptime_secs: state.runtime.uptime_secs(),
        active_keys: state.runtime.keys.len() as u64,
        backend_count,
        stream_cache_enabled: state.runtime.stream_cache_enabled(),
    }))
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
    };
    state.runtime.keys.insert(token.clone(), stored);

    Ok(Json(CreateGatewayKeyResponse {
        id,
        name: req.name,
        key_full: token.clone(),
        key_preview: key_preview(&token),
        enabled: req.enabled,
    }))
}

async fn revoke_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    if state.runtime.keys.remove(&token).is_some() {
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

    let spec = stored_to_spec(&token, &entry, false);
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
    Ok(Json(TtlConfigView {
        default_ttl_secs: cfg.default_ttl_secs,
        model_overrides: cfg.model_overrides.clone(),
        consumer_overrides: cfg.consumer_overrides.clone(),
    }))
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
    Ok(Json(StreamCacheConfig {
        enabled: state.runtime.stream_cache_enabled(),
    }))
}

fn backend_to_spec(b: &crab_route::Backend, health: Option<&crab_route::BackendHealth>) -> BackendSpec {
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
