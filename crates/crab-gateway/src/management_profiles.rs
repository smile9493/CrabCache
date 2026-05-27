//! Management API: multi-vendor upstream profiles.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use crab_control::{
    CircuitBreakerView, ErrorResponse, KeyQuotaInfo, PatchUpstreamKeyRequest,
    ProfileRoutingBackendView, ProfileRoutingView, PutUpstreamProfileKeysRequest,
    PutUpstreamProfileRequest, RoutingKeyPoolSummary, UpstreamKeyView, UpstreamKeysPutMode,
    UpstreamProfileKeysView, UpstreamProfileView, UpstreamProfilesResponse, UpstreamTestResult,
    parse_upstream_base_url, validate_upstream_key,
};
use crab_proxy::{
    ProfileBuildInput, UpstreamKeyPool, UpstreamKeySpec, build_profile_runtime,
    resolve_profile_key_specs,
};
use std::sync::Arc;

use crate::management::{ManagementState, authorize, internal_error, schedule_persist_state};

/// Parse a balance API response into `KeyQuotaInfo`.
///
/// Supports two formats:
/// - **DeepSeek** (`/user/balance`): `balance_infos[]` with per-currency entries.
/// - **Generic / new-api** (`/v1/user/balance`): top-level flat fields.
async fn parse_balance_response(resp: reqwest::Response) -> Option<KeyQuotaInfo> {
    let body: serde_json::Value = resp.json().await.ok()?;
    let parse_f64 = |v: &serde_json::Value| -> Option<f64> {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
    };
    let is_available = body.get("is_available").and_then(|v| v.as_bool());

    // DeepSeek format: balance_infos[{currency, total_balance, granted_balance, ...}]
    if let Some(infos) = body.get("balance_infos").and_then(|v| v.as_array()) {
        let info = infos
            .iter()
            .find(|e| e.get("currency").and_then(|c| c.as_str()) == Some("CNY"))
            .or_else(|| infos.first())?;
        let balance = info.get("total_balance").and_then(parse_f64);
        let total_granted = info.get("granted_balance").and_then(parse_f64);
        return Some(KeyQuotaInfo {
            is_available,
            balance,
            total_granted,
            total_used: None,
        });
    }

    // Generic / new-api format: flat fields at top level or inside "data".
    let src = body.get("data").filter(|d| d.is_object()).unwrap_or(&body);
    let get_field =
        |field: &str| -> Option<&serde_json::Value> { src.get(field).or_else(|| body.get(field)) };
    let balance = get_field("balance").and_then(parse_f64);
    let total_granted = get_field("total_granted").and_then(parse_f64);
    let total_used = get_field("total_used").and_then(parse_f64);
    if balance.is_some() || total_granted.is_some() {
        return Some(KeyQuotaInfo {
            is_available,
            balance,
            total_granted,
            total_used,
        });
    }

    None
}

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
    let mut profiles: Vec<UpstreamProfileView> = {
        let map = runtime.upstream_profiles.read();
        let mut ids: Vec<String> = map.keys().cloned().collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|id: String| profile_view(runtime, &id))
            .collect()
    };
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

    // Resolve key specs with fallback to existing/default/legacy pools instead of passing empty.
    let resolved_specs = resolve_profile_key_specs(Vec::new(), &state.runtime, &id);
    let profile = build_profile_runtime(
        input,
        resolved_specs,
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
    if state.runtime.profile(id).is_none() {
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
    let profile = state
        .runtime
        .profile(id)
        .expect("profile existence verified above");
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
    if state.runtime.profile(profile_id).is_none() {
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
    let profile = state
        .runtime
        .profile(profile_id)
        .expect("profile existence verified above");
    let pool = profile.resolve_upstream_pool();
    if let Some(enabled) = req.enabled
        && !pool.set_enabled(key_id, enabled)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("upstream key '{key_id}' not found"),
            }),
        )
            .into_response());
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
            quota: None,
        }));
    };
    let api_key = guard.bearer_secret().to_string();
    drop(guard);

    let url = format!("{}/v1/models", profile.base_url.trim_end_matches('/'));
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
            // Try balance endpoint for quota info (all providers).
            let mut quota = None;
            if ok {
                let base = profile.base_url.trim_end_matches('/');
                let balance_url = match profile.provider {
                    crab_pipeline::UpstreamProvider::Deepseek => format!("{}/user/balance", base),
                    _ => format!("{}/v1/user/balance", base),
                };
                if let Ok(br) = client
                    .get(&balance_url)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .send()
                    .await
                    && br.status().is_success()
                    && let Some(q) = parse_balance_response(br).await
                {
                    quota = Some(q);
                }
            }
            Ok(Json(UpstreamTestResult {
                ok,
                status_code: status.as_u16(),
                latency_ms,
                model_count,
                error,
                quota,
            }))
        }
        Err(e) => Ok(Json(UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms,
            model_count: None,
            error: Some(e.to_string()),
            quota: None,
        })),
    }
}

/// Test a specific upstream key by ID: first tries balance endpoint for quota info
/// (DeepSeek: `/user/balance`, others: `/v1/user/balance`), then falls back to `/v1/models`.
pub async fn test_upstream_profile_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(path): Path<ProfileKeyPath>,
) -> Result<Json<UpstreamTestResult>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile_id = path.id.trim();
    let key_id = path.key_id.trim();
    let profile = state
        .runtime
        .profile(profile_id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;
    let pool = profile.resolve_upstream_pool();
    let api_key = pool.secret_by_id(key_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("upstream key '{key_id}' not found"),
            }),
        )
            .into_response()
    })?;

    let base_url = profile.base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| internal_error("http client"))?;

    // Step 1: Try balance endpoint for quota info (all providers).
    // DeepSeek: GET /user/balance; others: GET /v1/user/balance (new-api compatible).
    let balance_url = match profile.provider {
        crab_pipeline::UpstreamProvider::Deepseek => {
            format!("{}/user/balance", base_url)
        }
        _ => format!("{}/v1/user/balance", base_url),
    };
    let balance_start = std::time::Instant::now();
    let balance_resp = client
        .get(&balance_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await;
    let balance_latency_ms = balance_start.elapsed().as_millis() as u64;

    if let Ok(r) = balance_resp
        && r.status().is_success()
        && let Some(quota) = parse_balance_response(r).await
    {
        return Ok(Json(UpstreamTestResult {
            ok: true,
            status_code: 200,
            latency_ms: balance_latency_ms,
            model_count: None,
            error: None,
            quota: Some(quota),
        }));
    }

    // Step 2: Fallback to GET /v1/models (generic validity check)
    let models_url = format!("{}/v1/models", base_url);
    let start2 = std::time::Instant::now();
    let resp = client
        .get(&models_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await;
    let latency_ms2 = start2.elapsed().as_millis() as u64;
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
                latency_ms: balance_latency_ms + latency_ms2,
                model_count,
                error,
                quota: None,
            }))
        }
        Err(e) => Ok(Json(UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms: balance_latency_ms + latency_ms2,
            model_count: None,
            error: Some(e.to_string()),
            quota: None,
        })),
    }
}

pub async fn get_profile_routing(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProfileRoutingView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let id = id.trim();
    let profile = state
        .runtime
        .profile(id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;

    let router = &profile.router;
    let backends = router.backends();
    let health_map = state.runtime.backend_health.read();

    let backend_views: Vec<ProfileRoutingBackendView> = backends
        .iter()
        .map(|b| {
            let h = health_map.get(&b.name);
            let (
                healthy,
                last_check_ms,
                latency_ms,
                circuit_state,
                consecutive_failures,
                half_open_successes,
            ) = match h {
                Some(health) => {
                    let state_str = serde_json::to_value(health.circuit_state)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| "closed".to_string());
                    (
                        health.healthy,
                        health.last_check_ms,
                        health.latency_ms,
                        state_str,
                        health.consecutive_failures,
                        health.half_open_successes,
                    )
                }
                None => (true, 0, 0, "closed".to_string(), 0, 0),
            };
            ProfileRoutingBackendView {
                name: b.name.clone(),
                addr: b.addr.to_string(),
                weight: b.weight,
                tls_sni: if b.tls_sni.is_empty() {
                    None
                } else {
                    Some(b.tls_sni.clone())
                },
                healthy,
                last_check_ms,
                latency_ms,
                circuit_state,
                consecutive_failures,
                half_open_successes,
            }
        })
        .collect();

    let cb = state.runtime.circuit_breaker_config;
    let pool = profile.resolve_upstream_pool();
    let pool_status = pool.list_status();
    // Align "available" with acquire() semantics: enabled + not in cooldown.
    // Inflight does not make a key unavailable because the pool selects least inflight.
    let available = pool.available_count();

    Ok(Json(ProfileRoutingView {
        profile_id: id.to_string(),
        backends: backend_views,
        circuit_breaker: CircuitBreakerView {
            failure_threshold: cb.failure_threshold,
            success_threshold: cb.success_threshold,
            timeout_ms: cb.timeout_ms,
        },
        key_pool: RoutingKeyPoolSummary {
            total: pool_status.len(),
            available,
        },
    }))
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
