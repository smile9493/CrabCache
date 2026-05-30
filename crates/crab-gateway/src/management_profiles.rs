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
    PutUpstreamProfileRequest, RoutingKeyPoolSummary, UpstreamKeyModelsEntry,
    UpstreamKeyView, UpstreamKeysPutMode, UpstreamProfileKeysModelsView,
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
            plan_type: None,
            primary_used_percent: None,
            secondary_used_percent: None,
            primary_reset_after_secs: None,
            secondary_reset_after_secs: None,
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
            plan_type: None,
            primary_used_percent: None,
            secondary_used_percent: None,
            primary_reset_after_secs: None,
            secondary_reset_after_secs: None,
        });
    }

    None
}

async fn fetch_codex_wham_usage(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    account_id: &str,
) -> Option<KeyQuotaInfo> {
    let url = format!(
        "{}/backend-api/wham/usage",
        base_url.trim_end_matches('/')
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("chatgpt-account-id", account_id)
        .header("Accept", "application/json")
        .header("originator", "codex_cli_rs")
        .header(
            "User-Agent",
            "codex_cli_rs/0.118.0 (Mac OS 26.3.1; arm64) iTerm.app/3.6.9",
        )
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let parse_f64 = |v: &serde_json::Value| -> Option<f64> {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
    };
    let parse_u64 = |v: &serde_json::Value| -> Option<u64> {
        v.as_u64()
            .or_else(|| v.as_i64().map(|n| n.max(0) as u64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
    };
    let plan_type = body
        .get("plan_type")
        .or_else(|| body.get("planType"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let rate_limit = body.get("rate_limit").or_else(|| body.get("rateLimit"));
    let primary = rate_limit
        .and_then(|v| v.get("primary_window").or_else(|| v.get("primaryWindow")));
    let secondary = rate_limit
        .and_then(|v| v.get("secondary_window").or_else(|| v.get("secondaryWindow")));
    let primary_used_percent = primary
        .and_then(|v| v.get("used_percent").or_else(|| v.get("usedPercent")))
        .and_then(parse_f64);
    let secondary_used_percent = secondary
        .and_then(|v| v.get("used_percent").or_else(|| v.get("usedPercent")))
        .and_then(parse_f64);
    let primary_reset_after_secs = primary
        .and_then(|v| v.get("reset_after_seconds").or_else(|| v.get("resetAfterSeconds")))
        .and_then(parse_u64);
    let secondary_reset_after_secs = secondary
        .and_then(|v| v.get("reset_after_seconds").or_else(|| v.get("resetAfterSeconds")))
        .and_then(parse_u64);
    let balance = body
        .get("credit_balance")
        .or_else(|| body.get("balance"))
        .and_then(parse_f64);
    let total_used = body
        .get("used")
        .or_else(|| body.get("total_used"))
        .and_then(parse_f64);
    let rate_allowed = rate_limit
        .and_then(|v| v.get("allowed"))
        .and_then(|v| v.as_bool());
    let limit_reached = rate_limit
        .and_then(|v| v.get("limit_reached").or_else(|| v.get("limitReached")))
        .and_then(|v| v.as_bool());
    let is_available = rate_allowed.or_else(|| limit_reached.map(|v| !v));

    if plan_type.is_some()
        || primary_used_percent.is_some()
        || secondary_used_percent.is_some()
        || balance.is_some()
        || total_used.is_some()
        || is_available.is_some()
    {
        return Some(KeyQuotaInfo {
            is_available,
            balance,
            total_granted: None,
            total_used,
            plan_type,
            primary_used_percent,
            secondary_used_percent,
            primary_reset_after_secs,
            secondary_reset_after_secs,
        });
    }
    None
}

fn decode_codex_jwt_claims(token: &str) -> (Option<String>, Option<String>) {
    decode_codex_jwt_claims_opt(token).unwrap_or((None, None))
}

fn decode_codex_jwt_claims_opt(token: &str) -> Option<(Option<String>, Option<String>)> {
    let payload = token.trim().split('.').nth(1)?;
    let padded = match payload.len() % 4 {
        0 => payload.to_string(),
        n => format!("{}{}", payload, "=".repeat(4 - n)),
    };
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        padded.as_bytes(),
    )
    .or_else(|_| {
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            padded.as_bytes(),
        )
    })
    .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let auth = value.get("https://api.openai.com/auth");
    let profile = value.get("https://api.openai.com/profile");
    let plan_type = auth
        .and_then(|v| v.get("chatgpt_plan_type"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let email = profile
        .and_then(|v| v.get("email"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some((email, plan_type))
}

const CODEX_MODELS_CLIENT_VERSION: &str = "0.133.0";

fn normalize_codex_pool_account_id(account_id: &str) -> Option<&str> {
    let trimmed = account_id.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(trimmed)
    }
}

async fn fetch_codex_models_slugs(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    account_id: &str,
) -> Result<Vec<String>, String> {
    let account_id = normalize_codex_pool_account_id(account_id)
        .ok_or_else(|| "Codex OAuth keys must include chatgpt account_id".to_string())?;
    let url = format!(
        "{}/backend-api/codex/models?client_version={CODEX_MODELS_CLIENT_VERSION}",
        base_url.trim_end_matches('/')
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("chatgpt-account-id", account_id)
        .header("Accept", "application/json")
        .header("Originator", "codex_cli_rs")
        .header(
            "User-Agent",
            "codex_cli_rs/0.118.0 (Mac OS 26.3.1; arm64) iTerm.app/3.6.9",
        )
        .send()
        .await
        .map_err(|e| format!("Cannot reach Codex models endpoint: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Codex models endpoint returned {}: {}",
            status.as_u16(),
            body.chars().take(200).collect::<String>()
        ));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Codex models response: {e}"))?;
    let mut ids: Vec<String> = body
        .get("models")
        .and_then(|v| v.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    m.get("slug")
                        .or_else(|| m.get("id"))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Err("Codex models response contained no model slugs".into());
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

async fn probe_codex_key_models(
    client: &reqwest::Client,
    base_url: &str,
    key_id: &str,
    account_id: &str,
    enabled: bool,
    api_key: &str,
) -> crab_control::UpstreamKeyModelsEntry {
    if !enabled {
        return crab_control::UpstreamKeyModelsEntry {
            key_id: key_id.to_string(),
            account_id: account_id.to_string(),
            enabled: false,
            ok: false,
            models: Vec::new(),
            error: Some("key disabled".into()),
            email: None,
            plan_type: None,
            quota: None,
        };
    }
    let (jwt_email, jwt_plan) = decode_codex_jwt_claims(api_key);
    let quota = fetch_codex_wham_usage(client, base_url, api_key, account_id).await;
    let plan_type = quota
        .as_ref()
        .and_then(|q| q.plan_type.clone())
        .or(jwt_plan);
    match fetch_codex_models_slugs(client, base_url, api_key, account_id).await {
        Ok(models) => crab_control::UpstreamKeyModelsEntry {
            key_id: key_id.to_string(),
            account_id: account_id.to_string(),
            enabled: true,
            ok: true,
            models,
            error: None,
            email: jwt_email,
            plan_type,
            quota,
        },
        Err(e) => crab_control::UpstreamKeyModelsEntry {
            key_id: key_id.to_string(),
            account_id: account_id.to_string(),
            enabled: true,
            ok: false,
            models: Vec::new(),
            error: Some(e),
            email: jwt_email,
            plan_type,
            quota,
        },
    }
}

fn format_upstream_test_error(status_code: u16, body: &str) -> String {
    match status_code {
        401 | 403 => format!("authentication failed (HTTP {status_code})"),
        402 => format!(
            "insufficient balance/quota (HTTP 402): {}",
            body.chars().take(200).collect::<String>()
        ),
        429 => format!(
            "rate limited or quota exhausted (HTTP 429): {}",
            body.chars().take(200).collect::<String>()
        ),
        _ => format!(
            "HTTP {status_code}: {}",
            body.chars().take(200).collect::<String>()
        ),
    }
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
        proxy_url: profile.proxy_url.clone(),
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

/// Upsert an upstream profile by id.
///
/// Semantics:
/// - Create if profile does not exist.
/// - Replace if profile already exists.
/// - Write-after-read is guaranteed: after a successful `PUT`, subsequent `GET /v1/upstream/profiles`
///   should immediately include this profile id.
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
        proxy_url: req.proxy_url.clone(),
    };

    // Resolve key specs with fallback to existing/default/legacy pools instead of passing empty.
    let resolved_specs = resolve_profile_key_specs(Vec::new(), &state.runtime, &id);
    let profile = build_profile_runtime(
        input,
        resolved_specs,
        state.upstream_key_cooldown_secs,
        existing_pool,
    )
    .map_err(|e| {
        tracing::warn!(
            profile_id = %id,
            provider = %req.provider.trim().to_lowercase(),
            base_url = %req.base_url.trim(),
            error = %e,
            "Failed to build upstream profile runtime"
        );
        bad_request(&e)
    })?;

    state.runtime.upsert_profile(profile).map_err(|e| {
        tracing::warn!(
            profile_id = %id,
            provider = %req.provider.trim().to_lowercase(),
            base_url = %req.base_url.trim(),
            error = %e,
            "Failed to upsert upstream profile"
        );
        bad_request(&e)
    })?;

    tracing::info!(
        profile_id = %id,
        provider = %req.provider.trim().to_lowercase(),
        base_url = %req.base_url.trim(),
        "Upstream profile upserted"
    );
    schedule_persist_state(&state);

    let view = profile_view(&state.runtime, &id).ok_or_else(|| {
        tracing::error!(
            profile_id = %id,
            provider = %req.provider.trim().to_lowercase(),
            base_url = %req.base_url.trim(),
            "Profile not found immediately after successful upsert"
        );
        internal_error(&format!(
            "upstream profile '{id}' missing after upsert; write-after-read violated"
        ))
    })?;
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
            supported_models: Vec::new(),
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

/// Remove one key from a profile pool.
pub async fn delete_profile_key(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(path): Path<ProfileKeyPath>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile_id = path.id.trim();
    let key_id = path.key_id.trim();
    if state.runtime.profile(profile_id).is_none() {
        return Err(bad_request("unknown upstream profile"));
    }
    let profile = state
        .runtime
        .profile(profile_id)
        .expect("profile existence verified above");
    let pool = profile.resolve_upstream_pool();
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
        .replace_profile_pool(profile_id, new_pool)
        .map_err(|e| bad_request(&e))?;
    schedule_persist_state(&state);
    Ok(StatusCode::NO_CONTENT)
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
    let account_id = guard.account_id().to_string();
    drop(guard);

    let test_base = profile.base_url.as_str().trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| internal_error("http client"))?;

    if profile.provider == crab_pipeline::UpstreamProvider::Codex {
        let start = std::time::Instant::now();
        if account_id.is_empty()
            || account_id == crab_proxy::DEFAULT_UPSTREAM_ACCOUNT_ID
        {
            return Ok(Json(UpstreamTestResult {
                ok: false,
                status_code: 0,
                latency_ms: 0,
                model_count: Some(crab_pipeline::CODEX_STATIC_MODELS.len()),
                error: Some(
                    "Codex OAuth keys must include chatgpt account_id (re-import credential)"
                        .to_string(),
                ),
                quota: None,
            }));
        }
        let quota = fetch_codex_wham_usage(&client, test_base, &api_key, &account_id).await;
        let models = fetch_codex_models_slugs(&client, test_base, &api_key, &account_id).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        let model_count = models.as_ref().ok().map(|m| m.len());
        let mut error = if quota.is_some() {
            None
        } else {
            Some("WHAM usage check failed; verify OAuth token and account_id".to_string())
        };
        if error.is_none() {
            if let Err(e) = &models {
                error = Some(e.clone());
            }
        }
        return Ok(Json(UpstreamTestResult {
            ok: quota.is_some() && models.is_ok(),
            status_code: if quota.is_some() && models.is_ok() {
                200
            } else {
                502
            },
            latency_ms,
            model_count,
            error,
            quota,
        }));
    }

    let url = format!("{}/v1/models", test_base);
    let start = std::time::Instant::now();
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
            let (model_count, error) = if ok {
                (
                    r.json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|v| v.get("data").and_then(|d| d.as_array()).map(|a| a.len())),
                    None,
                )
            } else {
                let body = r.text().await.unwrap_or_default();
                (
                    None,
                    Some(format_upstream_test_error(status.as_u16(), &body)),
                )
            };
            // Try balance endpoint for quota info (all providers).
            let mut quota = None;
            if ok {
                let balance_url = match profile.provider {
                    crab_pipeline::UpstreamProvider::Deepseek => {
                        format!("{}/user/balance", test_base)
                    }
                    _ => format!("{}/v1/user/balance", test_base),
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
/// (DeepSeek: `/user/balance`, Codex: WHAM usage, others: `/v1/user/balance`), then falls back to `/v1/models`.
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
    let account_id = pool
        .list_status()
        .into_iter()
        .find(|k| k.id == key_id)
        .map(|k| k.account_id)
        .unwrap_or_default();

    let test_base = profile.base_url.as_str().trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| internal_error("http client"))?;

    if profile.provider == crab_pipeline::UpstreamProvider::Codex {
        let start = std::time::Instant::now();
        if account_id.is_empty()
            || account_id == crab_proxy::DEFAULT_UPSTREAM_ACCOUNT_ID
        {
            return Ok(Json(UpstreamTestResult {
                ok: false,
                status_code: 0,
                latency_ms: 0,
                model_count: Some(crab_pipeline::CODEX_STATIC_MODELS.len()),
                error: Some(
                    "Codex OAuth keys must include chatgpt account_id (re-import credential)"
                        .to_string(),
                ),
                quota: None,
            }));
        }
        let quota = fetch_codex_wham_usage(&client, test_base, &api_key, &account_id).await;
        let models = fetch_codex_models_slugs(&client, test_base, &api_key, &account_id).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        let model_count = models.as_ref().ok().map(|m| m.len());
        let mut error = if quota.is_some() {
            None
        } else {
            Some("WHAM usage check failed; verify OAuth token and account_id".to_string())
        };
        if error.is_none() {
            if let Err(e) = &models {
                error = Some(e.clone());
            }
        }
        return Ok(Json(UpstreamTestResult {
            ok: quota.is_some() && models.is_ok(),
            status_code: if quota.is_some() && models.is_ok() {
                200
            } else {
                502
            },
            latency_ms,
            model_count,
            error,
            quota,
        }));
    }

    // Step 1: Try balance endpoint for quota info (all providers).
    let balance_url = match profile.provider {
        crab_pipeline::UpstreamProvider::Deepseek => format!("{}/user/balance", test_base),
        _ => format!("{}/v1/user/balance", test_base),
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
    let models_url = format!("{}/v1/models", test_base);
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
            let (model_count, error) = if ok {
                (
                    r.json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|v| v.get("data").and_then(|d| d.as_array()).map(|a| a.len())),
                    None,
                )
            } else {
                let body = r.text().await.unwrap_or_default();
                (
                    None,
                    Some(format_upstream_test_error(status.as_u16(), &body)),
                )
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

    // Query Pingora's health-check status per backend.
    let lb_guard = router.backends();
    let pingora_backends = lb_guard.backends();
    let registered = pingora_backends.get_backend();

    let backend_views: Vec<ProfileRoutingBackendView> = router
        .meta()
        .iter()
        .map(|(addr, m)| {
            // Match Pingora backend by extracting the SocketAddr, consistent with LbRouter pattern.
            let healthy = registered.iter().any(|pb| {
                if let pingora_core::protocols::l4::socket::SocketAddr::Inet(a) = pb.addr {
                    a == *addr && pingora_backends.ready(pb)
                } else {
                    false
                }
            });
            ProfileRoutingBackendView {
                name: m.name.clone(),
                addr: addr.to_string(),
                weight: 1,
                tls_sni: if m.tls_sni.is_empty() {
                    None
                } else {
                    Some(m.tls_sni.clone())
                },
                healthy,
                last_check_ms: 0,
                latency_ms: 0,
                circuit_state: if healthy { "closed" } else { "open" }.to_string(),
                consecutive_failures: 0,
                half_open_successes: 0,
            }
        })
        .collect();
    let pool = profile.resolve_upstream_pool();
    let pool_status = pool.list_status();
    // Align "available" with acquire() semantics: enabled + not in cooldown.
    // Inflight does not make a key unavailable because the pool selects least inflight.
    let available = pool.available_count();

    Ok(Json(ProfileRoutingView {
        profile_id: id.to_string(),
        backends: backend_views,
        circuit_breaker: CircuitBreakerView {
            failure_threshold: 5,
            success_threshold: 3,
            timeout_ms: 30000,
        },
        key_pool: RoutingKeyPoolSummary {
            total: pool_status.len(),
            available,
        },
    }))
}

/// List upstream model catalogs for every key in a profile (Codex: per OAuth account).
pub async fn get_profile_keys_models(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UpstreamProfileKeysModelsView>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile_id = id.trim();
    let profile = state
        .runtime
        .profile(profile_id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;
    let pool = profile.resolve_upstream_pool();
    let test_base = profile.base_url.as_str().trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| internal_error("http client"))?;

    let mut keys = Vec::new();
    for status in pool.list_status() {
        let Some(api_key) = pool.secret_by_id(&status.id) else {
            keys.push(UpstreamKeyModelsEntry {
                key_id: status.id.clone(),
                account_id: status.account_id.clone(),
                enabled: status.enabled,
                ok: false,
                models: Vec::new(),
                error: Some("upstream key secret not found".into()),
                email: None,
                plan_type: None,
                quota: None,
            });
            continue;
        };
        if profile.provider == crab_pipeline::UpstreamProvider::Codex {
            keys.push(
                probe_codex_key_models(
                    &client,
                    test_base,
                    &status.id,
                    &status.account_id,
                    status.enabled,
                    &api_key,
                )
                .await,
            );
        } else {
            keys.push(UpstreamKeyModelsEntry {
                key_id: status.id.clone(),
                account_id: status.account_id.clone(),
                enabled: status.enabled,
                ok: false,
                models: Vec::new(),
                error: Some("per-key model catalog is only supported for Codex profiles".into()),
                email: None,
                plan_type: None,
                quota: None,
            });
        }
    }

    Ok(Json(UpstreamProfileKeysModelsView {
        profile_id: profile_id.to_string(),
        keys,
    }))
}

/// Upstream model catalog for a single key in a profile.
pub async fn get_profile_key_models(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(path): Path<ProfileKeyPath>,
) -> Result<Json<UpstreamKeyModelsEntry>, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile_id = path.id.trim();
    let key_id = path.key_id.trim();
    let profile = state
        .runtime
        .profile(profile_id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;
    let pool = profile.resolve_upstream_pool();
    let status = pool
        .list_status()
        .into_iter()
        .find(|k| k.id == key_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("upstream key '{key_id}' not found"),
                }),
            )
                .into_response()
        })?;
    let api_key = pool.secret_by_id(key_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("upstream key '{key_id}' not found"),
            }),
        )
            .into_response()
    })?;
    let test_base = profile.base_url.as_str().trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| internal_error("http client"))?;

    if profile.provider != crab_pipeline::UpstreamProvider::Codex {
        return Ok(Json(UpstreamKeyModelsEntry {
            key_id: key_id.to_string(),
            account_id: status.account_id,
            enabled: status.enabled,
            ok: false,
            models: Vec::new(),
            error: Some("per-key model catalog is only supported for Codex profiles".into()),
            email: None,
            plan_type: None,
            quota: None,
        }));
    }

    Ok(Json(
        probe_codex_key_models(
            &client,
            test_base,
            key_id,
            &status.account_id,
            status.enabled,
            &api_key,
        )
        .await,
    ))
}

#[derive(serde::Deserialize)]
pub struct PutProfileKeysModelsCatalogRequest {
    pub catalog: std::collections::HashMap<String, Vec<String>>,
}

/// Push per-key upstream model catalogs (used for model-aware key routing).
pub async fn put_profile_keys_models_catalog(
    State(state): State<ManagementState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PutProfileKeysModelsCatalogRequest>,
) -> Result<StatusCode, Response> {
    authorize(&headers, &state.admin_key)?;
    let profile_id = id.trim();
    let profile = state
        .runtime
        .profile(profile_id)
        .ok_or_else(|| bad_request("unknown upstream profile"))?;
    let pool = profile.resolve_upstream_pool();
    pool.update_models_catalog(&req.catalog);
    schedule_persist_state(&state);
    Ok(StatusCode::NO_CONTENT)
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
