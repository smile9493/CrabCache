//! Upstream test and model sync helpers.

use crate::state::AppState;
use crate::types::{SyncResult, UpstreamModel, UpstreamModelsResponse, UpstreamTestResult};
use crab_control::{UpstreamProfileKeysModelsView, parse_upstream_base_url, validate_upstream_key};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CODEX_CHATGPT_BASE: &str = "https://chatgpt.com";
const CODEX_MODELS_CLIENT_VERSION: &str = "0.133.0";
const CODEX_ORIGINATOR: &str = "codex_cli_rs";
const CODEX_USER_AGENT: &str = "codex_cli_rs/0.118.0 (Mac OS 26.3.1; arm64) iTerm.app/3.6.9";

pub async fn test_upstream_connection(base_url: &str, api_key: &str) -> UpstreamTestResult {
    if looks_like_codex_oauth_access_token(api_key) || base_url.contains("chatgpt.com") {
        return UpstreamTestResult {
            ok: true,
            status_code: 200,
            latency_ms: 0,
            model_count: Some(crab_pipeline::CODEX_STATIC_MODELS.len()),
            error: None,
            quota: None,
        };
    }

    if let Err(e) = parse_upstream_base_url(base_url) {
        return UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms: 0,
            model_count: None,
            error: Some(e),
            quota: None,
        };
    }
    if let Err(e) = validate_upstream_key(api_key) {
        return UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms: 0,
            model_count: None,
            error: Some(e),
            quota: None,
        };
    }

    let url = format!("{}/v1/models", base_url.trim().trim_end_matches('/'));
    let start = Instant::now();
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return UpstreamTestResult {
                ok: false,
                status_code: 0,
                latency_ms: 0,
                model_count: None,
                error: Some(format!("HTTP client error: {e}")),
                quota: None,
            };
        }
    };

    let resp = match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return UpstreamTestResult {
                ok: false,
                status_code: 0,
                latency_ms: start.elapsed().as_millis() as u64,
                model_count: None,
                error: Some(format!("Cannot reach upstream: {e}")),
                quota: None,
            };
        }
    };

    let status_code = resp.status().as_u16();
    let latency_ms = start.elapsed().as_millis() as u64;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        let msg = match status_code {
            401 | 403 => format!("Authentication failed ({status_code}). Check your API key."),
            404 => format!("Models endpoint not found at {url}. Check the Base URL."),
            _ => format!(
                "Upstream returned {status_code}: {}",
                body.chars().take(200).collect::<String>()
            ),
        };
        return UpstreamTestResult {
            ok: false,
            status_code,
            latency_ms,
            model_count: None,
            error: Some(msg),
            quota: None,
        };
    }

    let upstream: UpstreamModelsResponse = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return UpstreamTestResult {
                ok: false,
                status_code,
                latency_ms,
                model_count: None,
                error: Some(format!("Failed to parse upstream response: {e}")),
                quota: None,
            };
        }
    };

    UpstreamTestResult {
        ok: true,
        status_code,
        latency_ms,
        model_count: Some(upstream.data.len()),
        error: None,
        quota: None,
    }
}

async fn fetch_upstream_models(
    base_url: &str,
    api_key: &str,
) -> Result<UpstreamModelsResponse, String> {
    let upstream_url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let resp = client
        .get(&upstream_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("Cannot reach upstream: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Upstream returned {}: {}",
            status.as_u16(),
            body.chars().take(200).collect::<String>()
        ));
    }

    resp.json()
        .await
        .map_err(|e| format!("Failed to parse upstream response: {e}"))
}

fn codex_static_models_response() -> UpstreamModelsResponse {
    UpstreamModelsResponse {
        data: crab_pipeline::CODEX_STATIC_MODELS
            .iter()
            .map(|id| UpstreamModel {
                id: (*id).to_string(),
                owned_by: "codex".to_string(),
            })
            .collect(),
    }
}

/// Codex OAuth access tokens are JWTs; they cannot call OpenAI `GET /v1/models` (`api.model.read`).
fn looks_like_codex_oauth_access_token(api_key: &str) -> bool {
    let token = api_key.trim();
    !token.starts_with("sk-") && token.starts_with("eyJ") && token.contains('.')
}

fn normalize_codex_account_id(account_id: &str) -> Option<&str> {
    let trimmed = account_id.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(trimmed)
    }
}

/// Codex OAuth tokens cannot call `GET /v1/models` (missing `api.model.read`); use Codex catalog.
fn is_codex_upstream(provider: &str, profile_id: &str, base_url: &str) -> bool {
    provider.eq_ignore_ascii_case("codex")
        || profile_id.eq_ignore_ascii_case("codex")
        || base_url.contains("chatgpt.com")
}

fn profile_has_codex_oauth_keys(state: &Arc<AppState>, profile_id: &str) -> bool {
    state
        .upstream_profile_secrets
        .read()
        .get(profile_id)
        .is_some_and(|pool| {
            pool.iter().any(|k| {
                k.enabled
                    && !k.secret.is_empty()
                    && normalize_codex_account_id(&k.account_id).is_some()
            })
        })
}

fn should_use_codex_model_catalog(
    provider: &str,
    profile_id: &str,
    base_url: &str,
    api_key: &str,
    state: &Arc<AppState>,
) -> bool {
    is_codex_upstream(provider, profile_id, base_url)
        || profile_has_codex_oauth_keys(state, profile_id)
        || looks_like_codex_oauth_access_token(api_key)
}

/// CLIProxyAPI / new-api: `GET https://chatgpt.com/backend-api/codex/models?client_version=...`
async fn fetch_codex_models_dynamic(
    access_token: &str,
    account_id: &str,
) -> Result<UpstreamModelsResponse, String> {
    let account_id = normalize_codex_account_id(account_id)
        .ok_or_else(|| "Codex OAuth sync requires account_id on the key".to_string())?;

    let url = format!(
        "{CODEX_CHATGPT_BASE}/backend-api/codex/models?client_version={CODEX_MODELS_CLIENT_VERSION}"
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token.trim()))
        .header("chatgpt-account-id", account_id)
        .header("Accept", "application/json")
        .header("Originator", CODEX_ORIGINATOR)
        .header("User-Agent", CODEX_USER_AGENT)
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

    Ok(UpstreamModelsResponse {
        data: ids
            .into_iter()
            .map(|id| UpstreamModel {
                id,
                owned_by: "codex".to_string(),
            })
            .collect(),
    })
}

fn pick_sync_key_context(state: &AppState, profile_id: &str) -> Option<(String, String)> {
    let profiles = state.upstream_profile_secrets.read();
    if let Some(pool) = profiles.get(profile_id)
        && let Some(key) = pool.iter().find(|k| k.enabled && !k.secret.is_empty())
    {
        return Some((key.secret.clone(), key.account_id.clone()));
    }
    drop(profiles);

    state
        .pick_sync_api_key(profile_id)
        .map(|secret| (secret, String::new()))
}

async fn profile_provider_async(state: &Arc<AppState>, profile_id: &str) -> String {
    state
        .gateway
        .list_upstream_profiles()
        .await
        .ok()
        .and_then(|r| {
            r.profiles
                .into_iter()
                .find(|p| p.id == profile_id)
                .map(|p| p.provider)
        })
        .unwrap_or_else(|| profile_id.to_string())
}

async fn fetch_models_for_profile(
    state: &Arc<AppState>,
    profile_id: &str,
    base_url: &str,
    api_key: &str,
    account_id: &str,
) -> Result<UpstreamModelsResponse, String> {
    let provider = profile_provider_async(state, profile_id).await;
    if !should_use_codex_model_catalog(&provider, profile_id, base_url, api_key, state) {
        return fetch_upstream_models(base_url, api_key).await;
    }

    if normalize_codex_account_id(account_id).is_some() {
        if let Ok(dynamic) = fetch_codex_models_dynamic(api_key, account_id).await {
            return Ok(dynamic);
        }
    }

    Ok(codex_static_models_response())
}

struct ProfileModelCatalog {
    models: Vec<UpstreamModel>,
    account_ids: HashMap<String, Vec<String>>,
    key_ids: HashMap<String, Vec<String>>,
}

fn merge_codex_key_catalogs(
    view: UpstreamProfileKeysModelsView,
    owned_by: &str,
) -> Result<ProfileModelCatalog, String> {
    let mut account_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut key_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_ids: HashSet<String> = HashSet::new();
    let mut any_ok = false;

    for entry in view.keys {
        if !entry.ok {
            continue;
        }
        any_ok = true;
        for model in entry.models {
            all_ids.insert(model.clone());
            if normalize_codex_account_id(&entry.account_id).is_some() {
                account_ids
                    .entry(model.clone())
                    .or_default()
                    .insert(entry.account_id.clone());
            }
            key_ids
                .entry(model)
                .or_default()
                .insert(entry.key_id.clone());
        }
    }

    if !any_ok {
        return Err("no Codex keys returned an upstream model catalog".into());
    }

    let mut ids: Vec<String> = all_ids.into_iter().collect();
    ids.sort();

    Ok(ProfileModelCatalog {
        models: ids
            .iter()
            .map(|id| UpstreamModel {
                id: id.clone(),
                owned_by: owned_by.to_string(),
            })
            .collect(),
        account_ids: account_ids
            .into_iter()
            .map(|(k, v)| {
                let mut ids: Vec<String> = v.into_iter().collect();
                ids.sort();
                (k, ids)
            })
            .collect(),
        key_ids: key_ids
            .into_iter()
            .map(|(k, v)| {
                let mut ids: Vec<String> = v.into_iter().collect();
                ids.sort();
                (k, ids)
            })
            .collect(),
    })
}

async fn fetch_profile_model_catalog(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<ProfileModelCatalog, String> {
    let base_url = profile_base_url_async(state, profile_id).await?;
    let provider = profile_provider_async(state, profile_id).await;
    if is_codex_upstream(&provider, profile_id, &base_url) {
        if let Ok(view) = state
            .gateway
            .get_upstream_profile_keys_models(profile_id)
            .await
        {
            if let Ok(merged) = merge_codex_key_catalogs(view, &provider) {
                return Ok(merged);
            }
        }
    }

    let (api_key, account_id) = pick_sync_key_context(state, profile_id).ok_or_else(|| {
        format!("Configure API keys for profile '{profile_id}' before syncing models.")
    })?;
    let upstream =
        fetch_models_for_profile(state, profile_id, &base_url, &api_key, &account_id).await?;
    Ok(ProfileModelCatalog {
        models: upstream.data,
        account_ids: HashMap::new(),
        key_ids: HashMap::new(),
    })
}

/// Per-key upstream model catalogs for a profile (proxies gateway key pool).
pub async fn probe_profile_key_models_internal(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<crab_control::UpstreamProfileKeysModelsView, String> {
    state
        .gateway
        .get_upstream_profile_keys_models(profile_id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn profile_base_url_async(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<String, String> {
    let list = state
        .gateway
        .list_upstream_profiles()
        .await
        .map_err(|e| e.to_string())?;
    let profile = list
        .profiles
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("unknown upstream profile '{profile_id}'"))?;
    Ok(profile.base_url)
}

pub async fn sync_models_internal(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<crate::types::SyncResult, String> {
    let model_catalog = fetch_profile_model_catalog(state, profile_id).await?;
    let upstream_ids: Vec<String> = model_catalog.models.iter().map(|m| m.id.clone()).collect();
    let owned_by_default = state
        .gateway
        .list_upstream_profiles()
        .await
        .ok()
        .and_then(|r| {
            r.profiles
                .into_iter()
                .find(|p| p.id == profile_id)
                .map(|p| p.provider)
        })
        .unwrap_or_else(|| profile_id.to_string());

    let (added, removed, unchanged, profile_total) = {
        let mut stored = state.models.write();
        let existing_for_profile: Vec<crate::state::StoredModel> = stored
            .models
            .iter()
            .filter(|m| m.profile_id == profile_id)
            .cloned()
            .collect();
        let existing_ids: Vec<String> = existing_for_profile.iter().map(|m| m.id.clone()).collect();

        let added: Vec<String> = upstream_ids
            .iter()
            .filter(|id| !existing_ids.contains(id))
            .cloned()
            .collect();

        let removed: Vec<String> = existing_ids
            .iter()
            .filter(|id| !upstream_ids.contains(id))
            .cloned()
            .collect();

        let unchanged = upstream_ids
            .iter()
            .filter(|id| existing_ids.contains(id))
            .count();

        stored.models.retain(|m| m.profile_id != profile_id);

        for m in model_catalog.models {
            let existing = existing_for_profile.iter().find(|e| e.id == m.id);
            stored.models.push(crate::state::StoredModel {
                profile_id: profile_id.to_string(),
                id: m.id.clone(),
                owned_by: if m.owned_by.is_empty() {
                    owned_by_default.clone()
                } else {
                    m.owned_by
                },
                context_length: existing.and_then(|e| e.context_length),
                input_price_per_mtok: existing.and_then(|e| e.input_price_per_mtok),
                output_price_per_mtok: existing.and_then(|e| e.output_price_per_mtok),
                available: true,
                account_ids: model_catalog
                    .account_ids
                    .get(&m.id)
                    .cloned()
                    .unwrap_or_default(),
                key_ids: model_catalog
                    .key_ids
                    .get(&m.id)
                    .cloned()
                    .unwrap_or_default(),
            });
        }

        let profile_total = stored
            .models
            .iter()
            .filter(|m| m.profile_id == profile_id)
            .count();
        let synced_at = chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        stored
            .synced_at_by_profile
            .insert(profile_id.to_string(), synced_at);

        (added, removed, unchanged, profile_total)
    };

    state.flush_persist();

    let provider = profile_provider_async(state, profile_id).await;
    let base_url = profile_base_url_async(state, profile_id)
        .await
        .unwrap_or_default();
    if provider.eq_ignore_ascii_case("codex") || is_codex_upstream(&provider, profile_id, &base_url)
    {
        let mut routing_catalog: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (model, key_ids) in &model_catalog.key_ids {
            for key_id in key_ids {
                routing_catalog
                    .entry(key_id.clone())
                    .or_default()
                    .push(model.clone());
            }
        }
        for models in routing_catalog.values_mut() {
            models.sort();
            models.dedup();
        }
        let _ = state
            .gateway
            .put_upstream_profile_keys_models_catalog(profile_id, &routing_catalog)
            .await;
    }

    Ok(crate::types::SyncResult {
        added,
        removed,
        unchanged,
        total: profile_total,
    })
}

pub async fn detect_models_internal(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<crate::types::ModelDetectResponse, String> {
    let catalog = fetch_profile_model_catalog(state, profile_id).await?;
    let upstream_ids: Vec<String> = catalog.models.iter().map(|m| m.id.clone()).collect();
    let stored = state.models.read();
    let existing_ids: Vec<String> = stored
        .models
        .iter()
        .filter(|m| m.profile_id == profile_id)
        .map(|m| m.id.clone())
        .collect();

    let to_add: Vec<String> = upstream_ids
        .iter()
        .filter(|id| !existing_ids.contains(id))
        .cloned()
        .collect();
    let to_remove: Vec<String> = existing_ids
        .iter()
        .filter(|id| !upstream_ids.contains(id))
        .cloned()
        .collect();
    let unchanged = upstream_ids
        .iter()
        .filter(|id| existing_ids.contains(id))
        .count();

    Ok(crate::types::ModelDetectResponse {
        to_add,
        to_remove,
        unchanged,
        upstream_total: upstream_ids.len(),
    })
}

pub fn apply_models_internal(
    state: &Arc<AppState>,
    profile_id: &str,
    add: Vec<String>,
    remove: Vec<String>,
) -> SyncResult {
    let owned_by = state.profile_provider(profile_id);
    let mut stored = state.models.write();
    let mut existing_ids: Vec<String> = stored
        .models
        .iter()
        .filter(|m| m.profile_id == profile_id)
        .map(|m| m.id.clone())
        .collect();

    for id in &remove {
        stored
            .models
            .retain(|m| !(m.profile_id == profile_id && &m.id == id));
        existing_ids.retain(|e| e != id);
    }

    let mut added = Vec::new();
    for id in add {
        if !existing_ids.contains(&id) {
            stored.models.push(crate::state::StoredModel {
                profile_id: profile_id.to_string(),
                id: id.clone(),
                owned_by: owned_by.clone(),
                context_length: None,
                input_price_per_mtok: None,
                output_price_per_mtok: None,
                available: true,
                account_ids: Vec::new(),
                key_ids: Vec::new(),
            });
            existing_ids.push(id.clone());
            added.push(id);
        }
    }

    let total = stored
        .models
        .iter()
        .filter(|m| m.profile_id == profile_id)
        .count();
    let unchanged = total.saturating_sub(added.len());
    drop(stored);
    state.flush_persist();

    SyncResult {
        added,
        removed: remove,
        unchanged,
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_jwt_detected_as_codex_token() {
        assert!(looks_like_codex_oauth_access_token(
            "eyJhbGciOiJSUzI1NiIs.eyJzdWIiOiIxMjM0In0.signature"
        ));
    }

    #[test]
    fn sk_key_not_treated_as_codex_oauth() {
        assert!(!looks_like_codex_oauth_access_token("sk-proj-abc123"));
    }

    #[test]
    fn default_account_id_is_not_codex_account() {
        assert!(normalize_codex_account_id("").is_none());
        assert!(normalize_codex_account_id("default").is_none());
        assert_eq!(normalize_codex_account_id("user-abc"), Some("user-abc"));
    }

    #[test]
    fn merge_codex_key_catalogs_unions_per_account_models() {
        let view = crab_control::UpstreamProfileKeysModelsView {
            profile_id: "codex".to_string(),
            keys: vec![
                crab_control::UpstreamKeyModelsEntry {
                    key_id: "key-2".to_string(),
                    account_id: "acct-plus".to_string(),
                    enabled: true,
                    ok: true,
                    models: vec!["gpt-5.5".into(), "gpt-5.3-codex".into()],
                    error: None,
                },
                crab_control::UpstreamKeyModelsEntry {
                    key_id: "key-3".to_string(),
                    account_id: "acct-free".to_string(),
                    enabled: true,
                    ok: true,
                    models: vec!["gpt-5.5".into(), "gpt-5.4-mini".into()],
                    error: None,
                },
            ],
            total_count: None,
            truncated: false,
        };
        let merged = merge_codex_key_catalogs(view, "codex").unwrap();
        assert_eq!(merged.models.len(), 3);
        assert_eq!(
            merged.account_ids.get("gpt-5.5"),
            Some(&vec!["acct-free".to_string(), "acct-plus".to_string()])
        );
        assert_eq!(
            merged.key_ids.get("gpt-5.3-codex"),
            Some(&vec!["key-2".to_string()])
        );
    }
}
