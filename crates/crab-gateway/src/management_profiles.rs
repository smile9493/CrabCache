//! Management API: multi-vendor upstream profiles.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use crab_control::{
    ErrorResponse, PatchUpstreamKeyRequest, PutUpstreamProfileKeysRequest, PutUpstreamProfileRequest,
    UpstreamKeyView,
    UpstreamKeysPutMode, UpstreamProfileKeysView, UpstreamProfileView, UpstreamProfilesResponse,
    UpstreamTestResult, parse_upstream_base_url, validate_upstream_key,
};
use crab_proxy::{
    ProfileBuildInput, UpstreamKeyPool, UpstreamKeySpec, build_profile_runtime,
};
use std::sync::Arc;

use crate::management::{ManagementState, authorize, internal_error, schedule_persist_state};

fn profile_view(runtime: &crab_proxy::RuntimeConfig, id: &str) -> Option<UpstreamProfileView> {
    let profile = runtime.profile(id)?;
    let pool = profile.resolve_upstream_pool();
    Some(UpstreamProfileView {
        id: profile.id.clone(),
        provider: profile.provider.as_str().to_string(),
        base_url: profile.base_url.clone(),
        fallback_model: profile.fallback_model.clone(),
        endpoints: runtime.profile_endpoints(&profile.id),
        tls_sni: profile.tls_sni.clone(),
        key_pool_count: pool.len(),
        keys_available: pool.available_count(),
    })
}

fn profiles_response(runtime: &crab_proxy::RuntimeConfig) -> UpstreamProfilesResponse {
    let mut profiles: Vec<UpstreamProfileView> = runtime
        .upstream_profiles
        .read()
        .map(|map| {
            let mut ids: Vec<String> = map.keys().cloned().collect();
            ids.sort();
            ids.into_iter()
                .filter_map(|id| profile_view(runtime, &id))
                .collect()
        })
        .unwrap_or_default();
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    UpstreamProfilesResponse {
        default_profile_id: runtime.default_upstream_profile_id(),
        profiles,
    }
}

pub async fn list_upstream_profiles(
    State(state): State<ManagementState>,
    headers: HeaderMap,
) -> Result<Json<UpstreamProfilesResponse>, Response> {
    authorize(&headers, &state.admin_key)?;
    Ok(Json(profiles_response(&state.runtime)))
}

pub async fn put_upstream_profile(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PutUpstreamProfileRequest>,
) -> Result<Json<UpstreamProfileView>, Response> {
    authorize(&headers, &state.admin_key)?;

    let id = id.trim().to_string();
    if id.is_empty() {
        return Err(bad_request("profile id must not be empty"));
    }
    if req.provider.trim().is_empty() {
        return Err(bad_request("provider must not be empty"));
    }
    if req.fallback_model.trim().is_empty() {
        return Err(bad_request("fallback_model must not be empty"));
    }
    let _ = parse_upstream_base_url(&req.base_url).map_err(|e| bad_request(&e))?;

    let existing = state.runtime.profile(&id);
    let existing_pool = existing.as_ref().map(|p| Arc::clone(&p.upstream_pool));

    let input = ProfileBuildInput {
        id: id.clone(),
        provider: req.provider.trim().to_lowercase(),
        base_url: req.base_url.trim().to_string(),
        fallback_model: req.fallback_model.trim().to_string(),
        endpoints: req.endpoints.clone(),
        tls_sni: req.tls_sni.clone(),
        default_weight: req.default_weight.max(1),
    };

    let profile = build_profile_runtime(
        input,
        Vec::new(),
        state.upstream_key_cooldown_secs,
        existing_pool,
    )
    .map_err(|e| bad_request(&e))?;

    state
        .runtime
        .upsert_profile(profile)
        .map_err(|e| bad_request(&e))?;

    tracing::info!(profile_id = %id, "Upstream profile upserted");
    schedule_persist_state(&state);

    let view = profile_view(&state.runtime, &id)
        .ok_or_else(|| internal_error("profile missing after upsert"))?;
    Ok(Json(view))
}

pub async fn delete_upstream_profile(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    state
        .runtime
        .remove_profile(id.trim())
        .map_err(|e| bad_request(&e))?;
    schedule_persist_state(&state);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_profile_keys(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UpstreamProfileKeysView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let id = id.trim();
    let profile = state
        .runtime
        .profile(id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;
    let pool = profile.resolve_upstream_pool();
    let keys: Vec<UpstreamKeyView> = pool
        .list_status()
        .into_iter()
        .map(|s| UpstreamKeyView {
            id: s.id,
            preview: s.preview,
            account_id: s.account_id,
            enabled: s.enabled,
            inflight: s.inflight,
            cooldown_remaining_secs: s.cooldown_remaining_secs,
        })
        .collect();
    Ok(Json(UpstreamProfileKeysView {
        profile_id: id.to_string(),
        keys,
    }))
}

pub async fn put_profile_keys(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PutUpstreamProfileKeysRequest>,
) -> Result<Json<UpstreamProfileKeysView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let id = id.trim();
    if !state.runtime.profile(id).is_some() {
        return Err(bad_request("unknown upstream profile"));
    }
    if req.keys.is_empty() {
        return Err(bad_request("at least one upstream key is required"));
    }
    for (i, k) in req.keys.iter().enumerate() {
        if k.secret.trim().is_empty() {
            return Err(bad_request(&format!(
                "upstream key #{} secret must not be empty",
                i + 1
            )));
        }
        validate_upstream_key(&k.secret).map_err(|e| bad_request(&e))?;
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
    let profile = state.runtime.profile(id).unwrap();
    let current = profile.resolve_upstream_pool();
    let new_pool = match req.mode {
        UpstreamKeysPutMode::Append => UpstreamKeyPool::merge_append(&current, specs),
        UpstreamKeysPutMode::Replace => UpstreamKeyPool::hot_replace(&current, specs),
    };
    state
        .runtime
        .replace_profile_pool(id, new_pool)
        .map_err(|e| bad_request(&e))?;
    schedule_persist_state(&state);
    get_profile_keys(State(state), headers, Path(id.to_string())).await
}

#[derive(serde::Deserialize)]
pub struct ProfileKeyPath {
    pub id: String,
    pub key_id: String,
}

pub async fn patch_profile_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(path): Path<ProfileKeyPath>,
    Json(req): Json<PatchUpstreamKeyRequest>,
) -> Result<Json<UpstreamKeyView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile_id = path.id.trim();
    let key_id = path.key_id.trim();
    if !state.runtime.profile(profile_id).is_some() {
        return Err(bad_request("unknown upstream profile"));
    }
    if req.enabled.is_none() && req.secret.is_none() {
        return Err(bad_request("no fields to update"));
    }
    if req.secret.is_some() {
        return Err(bad_request(
            "rotating secret via PATCH is not supported; use PUT /v1/upstream/profiles/{id}/keys",
        ));
    }
    let profile = state.runtime.profile(profile_id).unwrap();
    let pool = profile.resolve_upstream_pool();
    if let Some(enabled) = req.enabled {
        if !pool.set_enabled(key_id, enabled) {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("upstream key '{key_id}' not found"),
                }),
            )
                .into_response());
        }
    }
    let view = pool
        .list_status()
        .into_iter()
        .find(|k| k.id == key_id)
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
                    error: format!("upstream key '{key_id}' not found"),
                }),
            )
                .into_response()
        })?;
    schedule_persist_state(&state);
    Ok(Json(view))
}

pub async fn test_upstream_profile(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UpstreamTestResult>, Response> {
    authorize(&headers, &state.admin_key)?;
    let id = id.trim();
    let profile = state
        .runtime
        .profile(id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;
    let pool = profile.resolve_upstream_pool();
    let Some(guard) = pool.acquire() else {
        return Ok(Json(UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms: 0,
            model_count: None,
            error: Some("no upstream API keys available".to_string()),
        }));
    };
    let api_key = guard.bearer_secret().to_string();
    drop(guard);

    let url = format!(
        "{}/v1/models",
        profile.base_url.trim_end_matches('/')
    );
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| internal_error("http client"))?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await;
    let latency_ms = start.elapsed().as_millis() as u64;
    match resp {
        Ok(r) => {
            let status = r.status();
            let ok = status.is_success();
            let model_count = if ok {
                r.json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|v| v.get("data").and_then(|d| d.as_array()).map(|a| a.len()))
            } else {
                None
            };
            let error = if ok {
                None
            } else {
                Some(format!("HTTP {}", status.as_u16()))
            };
            Ok(Json(UpstreamTestResult {
                ok,
                status_code: status.as_u16(),
                latency_ms,
                model_count,
                error,
            }))
        }
        Err(e) => Ok(Json(UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms,
            model_count: None,
            error: Some(e.to_string()),
        })),
    }
}

fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
        .into_response()
}
