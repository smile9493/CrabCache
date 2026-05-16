use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use crab_control::{
    parse_backend_endpoints, ApiKeySpec, BackendSpec, CreateGatewayKeyRequest,
    CreateGatewayKeyResponse, ErrorResponse, GatewayStatus, PatchGatewayKeyRequest,
    PutBackendsRequest, PutTtlConfigRequest, RoutingBackendsView, TtlConfigView,
    GATEWAY_ADMIN_KEY_HEADER,
};
use crab_proxy::{RuntimeConfig, StoredKey};
use std::sync::Arc;

#[derive(Clone)]
pub struct ManagementState {
    pub runtime: Arc<RuntimeConfig>,
    pub admin_key: String,
}

pub fn router(state: ManagementState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/keys", get(list_keys).post(create_key))
        .route("/v1/keys/{token}", delete(revoke_key).patch(patch_key))
        .route("/v1/cache/ttl", get(get_ttl).put(put_ttl))
        .route("/v1/routing/backends", get(get_backends).put(put_backends))
        .with_state(state)
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

fn backend_to_spec(b: &crab_route::Backend) -> BackendSpec {
    BackendSpec {
        name: b.name.clone(),
        addr: b.addr.to_string(),
        weight: b.weight,
        tls_sni: b.tls_sni.clone(),
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
    let backends = router
        .backends()
        .iter()
        .map(|b| backend_to_spec(b.as_ref()))
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

    let backends = router
        .backends()
        .iter()
        .map(|b| backend_to_spec(b.as_ref()))
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
