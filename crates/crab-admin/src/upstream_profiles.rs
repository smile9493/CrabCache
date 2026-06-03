//! Admin API proxy for gateway upstream profiles.
//!
//! **Cold storage policy**: profile metadata → `upstream_profile_configs` (PG);
//! key pool mutations → `upstream_profile_secrets` (PG). Both must persist before
//! pushing to Gateway. Gateway Redis holds runtime hot state only.

use crate::persist::{PersistedUpstreamPoolSecret, PersistedUpstreamProfileConfig};
use crate::state::{AppState, UpstreamPoolSecret};
use crate::types::upstream_test_from_control;
use crate::types::{
    PatchUpstreamKeyRequest, PutUpstreamProfileAdminRequest, UpstreamKeyInput,
    UpstreamKeyPoolEntry, UpstreamKeysPutMode, UpstreamProfileAdminView,
    UpstreamProfileKeysAdminView, UpstreamProfilesAdminResponse, patch_upstream_key_to_control,
    put_upstream_profile_keys_to_control, upstream_key_view_from_control,
};
use axum::Json;
use crab_admin_types::upstream::UpstreamKeyView;
use crab_control::{PutUpstreamProfileRequest, UpstreamProfileView, UpstreamProfilesResponse};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn profile_config_from_put(id: &str, req: &PutUpstreamProfileAdminRequest) -> PersistedUpstreamProfileConfig {
    PersistedUpstreamProfileConfig {
        profile_id: id.to_string(),
        provider: req.provider.trim().to_lowercase(),
        base_url: req.base_url.trim().to_string(),
        fallback_model: req.fallback_model.trim().to_string(),
        endpoints: req.endpoints.clone(),
        tls_sni: req.tls_sni.clone(),
        proxy_url: req.proxy_url.clone(),
        fallback_profile_id: req.fallback_profile_id.clone(),
        fallback_max_retries: req.fallback_max_retries,
    }
}

fn put_request_from_config(cfg: &PersistedUpstreamProfileConfig) -> PutUpstreamProfileRequest {
    PutUpstreamProfileRequest {
        provider: cfg.provider.clone(),
        base_url: cfg.base_url.clone(),
        fallback_model: cfg.fallback_model.clone(),
        endpoints: cfg.endpoints.clone(),
        tls_sni: cfg.tls_sni.clone(),
        default_weight: 1,
        proxy_url: cfg.proxy_url.clone(),
        fallback_profile_id: cfg.fallback_profile_id.clone(),
        fallback_max_retries: cfg.fallback_max_retries,
    }
}

pub async fn push_profile_config_to_gateway(
    state: &AppState,
    cfg: &PersistedUpstreamProfileConfig,
) -> Result<(), String> {
    let gw_req = put_request_from_config(cfg);
    state
        .gateway
        .put_upstream_profile(&cfg.profile_id, &gw_req)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn persist_profile_config_to_pg(state: &AppState, cfg: &PersistedUpstreamProfileConfig) {
    let pg = state.pg_store.read().clone();
    let Some(pg) = pg else {
        return;
    };
    let _guard = state.pg_write_lock.lock().await;
    if let Err(e) = pg.upsert_profile_config(cfg).await {
        tracing::warn!(
            error = %e,
            profile_id = %cfg.profile_id,
            "PG persist upstream profile config failed (non-fatal)"
        );
    }
}

fn map_profile(p: UpstreamProfileView) -> UpstreamProfileAdminView {
    UpstreamProfileAdminView {
        id: p.id,
        provider: p.provider,
        base_url: p.base_url,
        fallback_model: p.fallback_model,
        endpoints: p.endpoints,
        tls_sni: p.tls_sni,
        key_pool_count: p.key_pool_count,
        keys_available: p.keys_available,
        proxy_url: p.proxy_url,
        fallback_profile_id: p.fallback_profile_id,
        fallback_max_retries: p.fallback_max_retries,
    }
}

fn profile_view_from_config(cfg: &PersistedUpstreamProfileConfig) -> UpstreamProfileAdminView {
    UpstreamProfileAdminView {
        id: cfg.profile_id.clone(),
        provider: cfg.provider.clone(),
        base_url: cfg.base_url.clone(),
        fallback_model: cfg.fallback_model.clone(),
        endpoints: cfg.endpoints.clone(),
        tls_sni: cfg.tls_sni.clone().unwrap_or_default(),
        key_pool_count: 0,
        keys_available: 0,
        proxy_url: cfg.proxy_url.clone(),
        fallback_profile_id: cfg.fallback_profile_id.clone(),
        fallback_max_retries: cfg.fallback_max_retries.unwrap_or(2),
    }
}

fn overlay_config_metadata(view: &mut UpstreamProfileAdminView, cfg: &PersistedUpstreamProfileConfig) {
    view.provider = cfg.provider.clone();
    view.base_url = cfg.base_url.clone();
    view.fallback_model = cfg.fallback_model.clone();
    view.endpoints = cfg.endpoints.clone();
    view.tls_sni = cfg.tls_sni.clone().unwrap_or_default();
    view.proxy_url = cfg.proxy_url.clone();
    view.fallback_profile_id = cfg.fallback_profile_id.clone();
    view.fallback_max_retries = cfg.fallback_max_retries.unwrap_or(2);
}

fn normalize_profile_key_inputs(
    existing: &[UpstreamPoolSecret],
    keys: Vec<UpstreamKeyInput>,
    replace: bool,
) -> Vec<UpstreamKeyInput> {
    let mut normalized = Vec::new();

    if replace {
        let mut id_seen: HashSet<String> = HashSet::new();
        let mut next_id: usize = keys.len().saturating_add(1);
        for mut key in keys {
            if key.secret.trim().is_empty() {
                continue;
            }
            if key.id.is_empty() || id_seen.contains(&key.id) {
                loop {
                    let candidate = format!("key-{}", next_id);
                    next_id += 1;
                    if !id_seen.contains(&candidate) {
                        key.id = candidate;
                        break;
                    }
                }
            }
            id_seen.insert(key.id.clone());
            normalized.push(key);
        }
        return normalized;
    }

    let mut seen_secrets: HashSet<String> = existing.iter().map(|s| s.secret.clone()).collect();
    let mut id_seen: HashSet<String> = existing.iter().map(|s| s.id.clone()).collect();
    let mut next_id: usize = existing.len().saturating_add(1);
    for mut key in keys {
        let secret = key.secret.trim().to_string();
        if secret.is_empty() || seen_secrets.contains(&secret) {
            continue;
        }
        seen_secrets.insert(secret.clone());
        key.secret = secret;
        if key.id.is_empty() || id_seen.contains(&key.id) {
            loop {
                let candidate = format!("key-{}", next_id);
                next_id += 1;
                if !id_seen.contains(&candidate) {
                    key.id = candidate;
                    break;
                }
            }
        }
        id_seen.insert(key.id.clone());
        normalized.push(key);
    }
    normalized
}

fn merge_profiles_with_pg_configs(
    gateway: UpstreamProfilesResponse,
    configs: &HashMap<String, PersistedUpstreamProfileConfig>,
) -> Vec<UpstreamProfileAdminView> {
    let mut by_id: HashMap<String, UpstreamProfileAdminView> = gateway
        .profiles
        .into_iter()
        .map(|p| {
            let id = p.id.clone();
            (id, map_profile(p))
        })
        .collect();

    for (id, cfg) in configs {
        let entry = by_id
            .entry(id.clone())
            .or_insert_with(|| profile_view_from_config(cfg));
        overlay_config_metadata(entry, cfg);
    }

    let mut profiles: Vec<_> = by_id.into_values().collect();
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    profiles
}

pub async fn list_profiles(state: &Arc<AppState>) -> Result<UpstreamProfilesAdminResponse, String> {
    let resp: UpstreamProfilesResponse = state
        .gateway
        .list_upstream_profiles()
        .await
        .map_err(|e| e.to_string())?;
    let configs = state.upstream_profile_configs.read().clone();
    let profiles = merge_profiles_with_pg_configs(resp.clone(), &configs);
    state.refresh_profile_providers().await;
    Ok(UpstreamProfilesAdminResponse {
        default_profile_id: resp.default_profile_id,
        profiles,
    })
}

pub async fn put_profile(
    state: &Arc<AppState>,
    id: &str,
    req: PutUpstreamProfileAdminRequest,
) -> Result<UpstreamProfileAdminView, String> {
    let cfg = profile_config_from_put(id, &req);

    let view = state
        .gateway
        .put_upstream_profile(id, &put_request_from_config(&cfg))
        .await
        .map_err(|e| e.to_string())?;
    {
        let mut map = state.upstream_profile_configs.write();
        map.insert(id.to_string(), cfg.clone());
    }
    persist_profile_config_to_pg(state, &cfg).await;
    state.flush_persist();
    state.refresh_profile_providers().await;
    let mut merged = map_profile(view);
    overlay_config_metadata(&mut merged, &cfg);
    Ok(merged)
}

pub async fn delete_profile(state: &Arc<AppState>, id: &str) -> Result<(), String> {
    state
        .gateway
        .delete_upstream_profile(id)
        .await
        .map_err(|e| e.to_string())?;
    {
        let mut map = state.upstream_profile_configs.write();
        map.remove(id);
    }
    {
        let mut map = state.upstream_profile_secrets.write();
        map.remove(id);
    }
    let pg = state.pg_store.read().clone();
    if let Some(pg) = pg {
        let _guard = state.pg_write_lock.lock().await;
        if let Err(e) = pg.delete_profile_config(id).await {
            tracing::warn!(error = %e, profile_id = %id, "PG delete profile config failed");
        }
        if let Err(e) = pg.replace_profile_secrets(id, &[]).await {
            tracing::warn!(error = %e, profile_id = %id, "PG delete profile secrets failed");
        }
    }
    state.flush_persist();
    Ok(())
}

pub async fn get_profile_keys(
    state: &Arc<AppState>,
    id: &str,
) -> Result<UpstreamProfileKeysAdminView, String> {
    if crate::oauth_codex::profile_is_codex_like(state, id) {
        crate::oauth_codex::materialize_codex_profile_pool(state, id).await;
    }

    let admin_pool = state
        .upstream_profile_secrets
        .read()
        .get(id)
        .cloned()
        .unwrap_or_default();

    let view = state
        .gateway
        .get_upstream_profile_keys(id)
        .await
        .map_err(|e| e.to_string())?;
    let models_probe = crate::upstream::probe_profile_key_models_internal(state, id)
        .await
        .ok();

    let mut keys: Vec<_> = if !admin_pool.is_empty() {
        profile_keys_from_admin_pool(&admin_pool, &view.keys, models_probe.as_ref())
    } else {
        view.keys
            .into_iter()
            .map(|k| {
                let mut mapped = upstream_key_view_from_control(k);
                if let Some(probe) = &models_probe {
                    if let Some(entry) = probe.keys.iter().find(|e| e.key_id == mapped.id) {
                        mapped = crate::types::enrich_upstream_key_view(mapped, entry);
                    }
                }
                mapped
            })
            .collect()
    };
    if crate::oauth_codex::profile_is_codex_like(state, id) {
        crate::oauth_codex::enrich_upstream_keys_from_credentials(state, &mut keys).await;
        keys.retain(|k| !k.id.starts_with("codex-auto-") && !k.account_id.starts_with("auto-"));
    }
    Ok(UpstreamProfileKeysAdminView {
        profile_id: view.profile_id,
        keys,
    })
}

fn key_preview(secret: &str) -> String {
    if secret.len() <= 10 {
        secret.to_string()
    } else {
        format!("{}...", &secret[..8])
    }
}

fn profile_keys_from_admin_pool(
    admin_pool: &[UpstreamPoolSecret],
    gateway_keys: &[crab_control::UpstreamKeyView],
    models_probe: Option<&crab_control::UpstreamProfileKeysModelsView>,
) -> Vec<UpstreamKeyView> {
    use std::collections::HashMap;

    let gw_by_id: HashMap<String, crab_control::UpstreamKeyView> = gateway_keys
        .iter()
        .map(|k| (k.id.clone(), k.clone()))
        .collect();

    admin_pool
        .iter()
        .map(|slot| {
            let mut mapped = if let Some(gk) = gw_by_id.get(&slot.id) {
                let mut view = upstream_key_view_from_control(gk.clone());
                view.enabled = slot.enabled;
                if !slot.account_id.is_empty() {
                    view.account_id = slot.account_id.clone();
                }
                view
            } else {
                UpstreamKeyView {
                    id: slot.id.clone(),
                    preview: key_preview(&slot.secret),
                    account_id: slot.account_id.clone(),
                    enabled: slot.enabled,
                    inflight: 0,
                    cooldown_remaining_secs: 0,
                    email: None,
                    plan_type: None,
                    models: Vec::new(),
                    quota: None,
                    priority: 0,
                    model_cooldowns: Vec::new(),
                }
            };
            if let Some(probe) = models_probe {
                if let Some(entry) = probe.keys.iter().find(|e| e.key_id == mapped.id) {
                    mapped = crate::types::enrich_upstream_key_view(mapped, entry);
                }
            }
            mapped
        })
        .collect()
}

pub async fn put_profile_keys(
    state: &Arc<AppState>,
    id: &str,
    keys: Vec<UpstreamKeyInput>,
    replace: bool,
) -> Result<UpstreamProfileKeysAdminView, String> {
    let mode = if replace {
        UpstreamKeysPutMode::Replace
    } else {
        UpstreamKeysPutMode::Append
    };
    let existing = state
        .upstream_profile_secrets
        .read()
        .get(id)
        .cloned()
        .unwrap_or_default();
    let normalized_keys = normalize_profile_key_inputs(&existing, keys, replace);
    let secret_lookup: HashMap<String, String> = existing
        .iter()
        .map(|s| (s.id.clone(), s.secret.clone()))
        .chain(
            normalized_keys
                .iter()
                .map(|k| (k.id.clone(), k.secret.clone())),
        )
        .collect();

    let req = put_upstream_profile_keys_to_control(&normalized_keys, mode);
    let view = state
        .gateway
        .put_upstream_profile_keys(id, &req)
        .await
        .map_err(|e| e.to_string())?;

    let persisted: Vec<UpstreamPoolSecret> = view
        .keys
        .iter()
        .map(|k| UpstreamPoolSecret {
            id: k.id.clone(),
            secret: secret_lookup.get(&k.id).cloned().unwrap_or_default(),
            enabled: k.enabled,
            account_id: k.account_id.clone(),
            priority: k.priority,
        })
        .collect();
    {
        let mut map = state.upstream_profile_secrets.write();
        map.insert(id.to_string(), persisted.clone());
    }

    // Persist key pool to PostgreSQL (Admin DB) when available (best-effort).
    // Important: never hold a parking_lot lock guard across an `.await`.
    let pg = { state.pg_store.read().clone() };
    if let Some(pg) = pg {
        let _guard = state.pg_write_lock.lock().await;
        let persisted: Vec<PersistedUpstreamPoolSecret> = persisted
            .into_iter()
            .map(|s| PersistedUpstreamPoolSecret {
                id: s.id,
                secret: s.secret,
                enabled: s.enabled,
                account_id: s.account_id,
                priority: s.priority,
            })
            .collect();
        if let Err(e) = pg.replace_profile_secrets(id, &persisted).await {
            tracing::warn!(error = %e, profile_id = %id, "PG persist upstream keys failed (non-fatal)");
        }
    }

    state.flush_persist();
    if crate::oauth_codex::profile_is_codex_like(state, id) {
        crate::oauth_codex::reconcile_codex_credentials_with_pool(state, id).await;
    }
    Ok(UpstreamProfileKeysAdminView {
        profile_id: view.profile_id,
        keys: view
            .keys
            .into_iter()
            .map(upstream_key_view_from_control)
            .collect(),
    })
}

pub async fn patch_profile_key(
    state: &Arc<AppState>,
    profile_id: &str,
    key_id: &str,
    req: PatchUpstreamKeyRequest,
) -> Result<UpstreamKeyPoolEntry, String> {
    let view = state
        .gateway
        .patch_upstream_profile_key(profile_id, key_id, &patch_upstream_key_to_control(&req))
        .await
        .map_err(|e| e.to_string())?;

    let current = state
        .upstream_profile_secrets
        .read()
        .get(profile_id)
        .cloned()
        .unwrap_or_default();
    let secret_lookup: HashMap<String, UpstreamPoolSecret> = current
        .iter()
        .map(|s| (s.id.clone(), s.clone()))
        .collect();
    let updated: Vec<UpstreamPoolSecret> = current
        .into_iter()
        .map(|s| {
            if s.id == key_id {
                UpstreamPoolSecret {
                    id: view.id.clone(),
                    secret: s.secret,
                    enabled: view.enabled,
                    account_id: view.account_id.clone(),
                    priority: view.priority,
                }
            } else {
                s
            }
        })
        .collect();
    if updated.iter().any(|s| s.id == view.id) {
        {
            let mut map = state.upstream_profile_secrets.write();
            map.insert(profile_id.to_string(), updated.clone());
        }
        let pg = { state.pg_store.read().clone() };
        if let Some(pg) = pg {
            let _guard = state.pg_write_lock.lock().await;
            let persisted: Vec<PersistedUpstreamPoolSecret> = updated
                .into_iter()
                .map(|s| {
                    let id = s.id.clone();
                    PersistedUpstreamPoolSecret {
                        id: id.clone(),
                        secret: secret_lookup
                            .get(&id)
                            .map(|slot| slot.secret.clone())
                            .unwrap_or_default(),
                        enabled: s.enabled,
                        account_id: s.account_id,
                        priority: s.priority,
                    }
                })
                .collect();
            if let Err(e) = pg.replace_profile_secrets(profile_id, &persisted).await {
                tracing::warn!(error = %e, profile_id = %profile_id, "PG persist key enabled failed (non-fatal)");
            }
        }
    }

    state.flush_persist();
    Ok(upstream_key_view_from_control(view))
}

pub async fn delete_profile_key(
    state: &Arc<AppState>,
    profile_id: &str,
    key_id: &str,
) -> Result<(), String> {
    state
        .gateway
        .delete_upstream_profile_key(profile_id, key_id)
        .await
        .map_err(|e| e.to_string())?;

    {
        let mut map = state.upstream_profile_secrets.write();
        if let Some(pool) = map.get_mut(profile_id) {
            pool.retain(|k| k.id != key_id);
        }
    }

    // Always flush in-memory state to admin-state.json (non-negotiable).
    state.flush_persist();

    // Best-effort PG sync (non-fatal on failure).
    let pg = { state.pg_store.read().clone() };
    if let Some(pg) = pg {
        let _guard = state.pg_write_lock.lock().await;
        let persisted: Vec<PersistedUpstreamPoolSecret> = state
            .upstream_profile_secrets
            .read()
            .get(profile_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|s| PersistedUpstreamPoolSecret {
                id: s.id,
                secret: s.secret,
                enabled: s.enabled,
                account_id: s.account_id,
                priority: s.priority,
            })
            .collect();
        if let Err(e) = pg.replace_profile_secrets(profile_id, &persisted).await {
            tracing::warn!(error = %e, profile_id = %profile_id, "PG persist key delete failed (non-fatal)");
        }
    }

    Ok(())
}

pub async fn test_profile(
    state: &Arc<AppState>,
    id: &str,
) -> Result<crate::types::UpstreamTestResult, String> {
    state
        .gateway
        .test_upstream_profile(id)
        .await
        .map(upstream_test_from_control)
        .map_err(|e| e.to_string())
}

pub async fn test_profile_key(
    state: &Arc<AppState>,
    profile_id: &str,
    key_id: &str,
) -> Result<crate::types::UpstreamTestResult, String> {
    state
        .gateway
        .test_upstream_profile_key(profile_id, key_id)
        .await
        .map(upstream_test_from_control)
        .map_err(|e| e.to_string())
}

/// Push Admin's cached profile key pool to the gateway (used after credential recovery).
pub async fn sync_profile_pool_to_gateway(
    state: &AppState,
    profile_id: &str,
    mode: UpstreamKeysPutMode,
) -> Result<(), String> {
    let secrets = state
        .upstream_profile_secrets
        .read()
        .get(profile_id)
        .cloned()
        .unwrap_or_default();
    if secrets.is_empty() {
        return Ok(());
    }
    let keys: Vec<UpstreamKeyInput> = secrets
        .iter()
        .map(|s| UpstreamKeyInput {
            id: s.id.clone(),
            secret: s.secret.clone(),
            enabled: s.enabled,
            account_id: s.account_id.clone(),
            priority: s.priority,
        })
        .collect();
    let req = put_upstream_profile_keys_to_control(&keys, mode);
    state
        .gateway
        .put_upstream_profile_keys(profile_id, &req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list_profiles_json(
    state: axum::extract::State<Arc<AppState>>,
) -> Result<Json<UpstreamProfilesAdminResponse>, (axum::http::StatusCode, String)> {
    list_profiles(&state)
        .await
        .map(Json)
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e))
}

/// Append or update a single API key in a profile's key pool (used by Codex OAuth import).
pub async fn put_profile_keys_upsert(
    state: &Arc<AppState>,
    profile_id: &str,
    secret: &str,
    account_id: &str,
    preferred_key_id: Option<&str>,
) -> Result<(), (axum::http::StatusCode, String)> {
    let normalized_account = normalize_pool_account_id(account_id);

    let view = state
        .gateway
        .get_upstream_profile_keys(profile_id)
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e.to_string()))?;

    let cached_secrets: HashMap<String, String> = state
        .upstream_profile_secrets
        .read()
        .get(profile_id)
        .map(|pool| {
            pool.iter()
                .map(|s| (s.id.clone(), s.secret.clone()))
                .collect()
        })
        .unwrap_or_default();

    let export_secrets: HashMap<String, String> = state
        .gateway
        .export_upstream_profile_keys(profile_id)
        .await
        .map(|export| export.keys.into_iter().map(|k| (k.id, k.secret)).collect())
        .unwrap_or_default();

    let mut matched_key_id: Option<String> = None;
    for key in &view.keys {
        let key_account = normalize_pool_account_id(&key.account_id);
        if key_account == normalized_account {
            matched_key_id = Some(key.id.clone());
            break;
        }
    }

    if let Some(key_id) = matched_key_id {
        let keys: Vec<UpstreamKeyInput> = view
            .keys
            .into_iter()
            .map(|k| {
                let secret_val = if k.id == key_id {
                    secret.to_string()
                } else {
                    cached_secrets
                        .get(&k.id)
                        .filter(|s| !s.trim().is_empty())
                        .cloned()
                        .or_else(|| {
                            export_secrets
                                .get(&k.id)
                                .filter(|s| !s.trim().is_empty())
                                .cloned()
                        })
                        .unwrap_or_default()
                };
                UpstreamKeyInput {
                    id: k.id,
                    secret: secret_val,
                    enabled: k.enabled,
                    account_id: if k.account_id.is_empty() {
                        "default".to_string()
                    } else {
                        k.account_id
                    },
                    priority: k.priority,
                }
            })
            .collect();

        if keys.iter().any(|k| k.id == key_id && k.secret.is_empty()) {
            return put_profile_keys_append(
                state,
                profile_id,
                secret,
                account_id,
                preferred_key_id,
            )
            .await;
        }

        put_profile_keys(state, profile_id, keys, true)
            .await
            .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e))?;
        return Ok(());
    }

    put_profile_keys_append(state, profile_id, secret, account_id, preferred_key_id).await
}

fn normalize_pool_account_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Append a single API key to a profile's key pool (used by Codex OAuth import).
pub async fn put_profile_keys_append(
    state: &Arc<AppState>,
    profile_id: &str,
    secret: &str,
    account_id: &str,
    preferred_key_id: Option<&str>,
) -> Result<(), (axum::http::StatusCode, String)> {
    let key_input = UpstreamKeyInput {
        id: preferred_key_id.unwrap_or_default().to_string(),
        secret: secret.to_string(),
        enabled: true,
        account_id: account_id.to_string(),
        priority: 0,
    };
    put_profile_keys(state, profile_id, vec![key_input], false)
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_append_keys_uses_stable_key_ids_and_dedupes_secrets() {
        let existing = vec![UpstreamPoolSecret {
            id: "key-1".to_string(),
            secret: "sk-existing".to_string(),
            enabled: true,
            account_id: "acct-1".to_string(),
            priority: 0,
        }];
        let keys = vec![
            UpstreamKeyInput {
                id: String::new(),
                secret: "sk-existing".to_string(),
                enabled: true,
                account_id: String::new(),
                priority: 0,
            },
            UpstreamKeyInput {
                id: String::new(),
                secret: "sk-new".to_string(),
                enabled: true,
                account_id: String::new(),
                priority: 0,
            },
        ];

        let normalized = normalize_profile_key_inputs(&existing, keys, false);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].id, "key-2");
        assert_eq!(normalized[0].secret, "sk-new");
    }

    #[test]
    fn normalize_replace_keys_assigns_key_ids_without_touching_existing_state() {
        let existing = vec![UpstreamPoolSecret {
            id: "uuid-1".to_string(),
            secret: "sk-existing".to_string(),
            enabled: true,
            account_id: "acct-1".to_string(),
            priority: 0,
        }];
        let keys = vec![
            UpstreamKeyInput {
                id: String::new(),
                secret: "sk-one".to_string(),
                enabled: true,
                account_id: String::new(),
                priority: 0,
            },
            UpstreamKeyInput {
                id: String::new(),
                secret: "sk-two".to_string(),
                enabled: true,
                account_id: String::new(),
                priority: 0,
            },
        ];

        let normalized = normalize_profile_key_inputs(&existing, keys, true);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].id, "key-3");
        assert_eq!(normalized[1].id, "key-4");
    }
}
