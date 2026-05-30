//! Admin API proxy for gateway upstream profiles.

use crate::persist::PersistedUpstreamPoolSecret;
use crate::state::{AppState, UpstreamPoolSecret};
use crate::types::upstream_test_from_control;
use crate::types::{
    PatchUpstreamKeyRequest, PutUpstreamProfileAdminRequest, UpstreamKeyInput,
    UpstreamKeyPoolEntry, UpstreamKeysPutMode, UpstreamProfileAdminView,
    UpstreamProfileKeysAdminView, UpstreamProfilesAdminResponse, patch_upstream_key_to_control,
    put_upstream_profile_keys_to_control, upstream_key_view_from_control,
};
use axum::Json;
use crab_control::{PutUpstreamProfileRequest, UpstreamProfileView, UpstreamProfilesResponse};
use std::sync::Arc;

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
    }
}

pub async fn list_profiles(state: &Arc<AppState>) -> Result<UpstreamProfilesAdminResponse, String> {
    let resp: UpstreamProfilesResponse = state
        .gateway
        .list_upstream_profiles()
        .await
        .map_err(|e| e.to_string())?;
    state.refresh_profile_providers().await;
    Ok(UpstreamProfilesAdminResponse {
        default_profile_id: resp.default_profile_id,
        profiles: resp.profiles.into_iter().map(map_profile).collect(),
    })
}

pub async fn put_profile(
    state: &Arc<AppState>,
    id: &str,
    req: PutUpstreamProfileAdminRequest,
) -> Result<UpstreamProfileAdminView, String> {
    let gw_req = PutUpstreamProfileRequest {
        provider: req.provider,
        base_url: req.base_url,
        fallback_model: req.fallback_model,
        endpoints: req.endpoints,
        tls_sni: req.tls_sni,
        default_weight: 1,
        proxy_url: req.proxy_url,
        fallback_profile_id: req.fallback_profile_id,
        fallback_max_retries: req.fallback_max_retries,
    };
    let view = state
        .gateway
        .put_upstream_profile(id, &gw_req)
        .await
        .map_err(|e| e.to_string())?;
    state.refresh_profile_providers().await;
    Ok(map_profile(view))
}

pub async fn delete_profile(state: &Arc<AppState>, id: &str) -> Result<(), String> {
    state
        .gateway
        .delete_upstream_profile(id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn get_profile_keys(
    state: &Arc<AppState>,
    id: &str,
) -> Result<UpstreamProfileKeysAdminView, String> {
    let view = state
        .gateway
        .get_upstream_profile_keys(id)
        .await
        .map_err(|e| e.to_string())?;
    let models_probe = crate::upstream::probe_profile_key_models_internal(state, id).await.ok();
    Ok(UpstreamProfileKeysAdminView {
        profile_id: view.profile_id,
        keys: view
            .keys
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
            .collect(),
    })
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
        let secrets: Vec<UpstreamPoolSecret> = keys
            .iter()
            .filter(|k| !k.secret.is_empty())
            .map(|k| UpstreamPoolSecret {
                id: if k.id.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    k.id.clone()
                },
                secret: k.secret.clone(),
                enabled: k.enabled,
                account_id: k.account_id.clone(),
            })
            .collect();
    {
        let mut map = state.upstream_profile_secrets.write();
        map.insert(id.to_string(), secrets);
    }


    // Persist key pool to PostgreSQL (Admin DB) when available.
    // Important: never hold a parking_lot lock guard across an `.await`.
    let pg = { state.pg_store.read().clone() };
    if let Some(pg) = pg {
        let _guard = state.pg_write_lock.lock().await;
        let persisted: Vec<PersistedUpstreamPoolSecret> = state
            .upstream_profile_secrets
            .read()
            .get(id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|s| PersistedUpstreamPoolSecret {
                id: s.id,
                secret: s.secret,
                enabled: s.enabled,
                account_id: s.account_id,
            })
            .collect();
        pg.replace_profile_secrets(id, &persisted)
            .await
            .map_err(|e| format!("persist upstream keys to postgres: {e}"))?;
    }

    let req = put_upstream_profile_keys_to_control(&keys, mode);
    let view = state
        .gateway
        .put_upstream_profile_keys(id, &req)
        .await
        .map_err(|e| e.to_string())?;
    state.flush_persist();
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

    // Best-effort: reflect enabled toggle into Admin PG persistence when available.
    if let Some(enabled) = req.enabled {
        let pg = { state.pg_store.read().clone() };
        if let Some(pg) = pg {
            let _guard = state.pg_write_lock.lock().await;
            let mut updated: Vec<PersistedUpstreamPoolSecret> = state
                .upstream_profile_secrets
                .read()
                .get(profile_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|s| {
                    let id = s.id.clone();
                    PersistedUpstreamPoolSecret {
                        enabled: if id == key_id { enabled } else { s.enabled },
                        id: s.id,
                        secret: s.secret,
                        account_id: s.account_id,
                    }
                })
                .collect();
            // If admin did not have this key cached, don't attempt to synthesize a secret.
            if updated.iter().any(|s| s.id == key_id) {
                pg.replace_profile_secrets(profile_id, &updated)
                    .await
                    .map_err(|e| format!("persist upstream key enabled to postgres: {e}"))?;
                // keep in-memory cache aligned
                {
                    let mut map = state.upstream_profile_secrets.write();
                    map.insert(
                        profile_id.to_string(),
                        updated
                            .drain(..)
                            .map(|p| UpstreamPoolSecret {
                                id: p.id,
                                secret: p.secret,
                                enabled: p.enabled,
                                account_id: p.account_id,
                            })
                            .collect(),
                    );
                }
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
            })
            .collect();
        pg.replace_profile_secrets(profile_id, &persisted)
            .await
            .map_err(|e| format!("persist upstream key delete to postgres: {e}"))?;
    }

    state.flush_persist();
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
) -> Result<(), (axum::http::StatusCode, String)> {
    let normalized_account = normalize_pool_account_id(account_id);

    let view = state
        .gateway
        .get_upstream_profile_keys(profile_id)
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e.to_string()))?;

    let cached_secrets = state
        .upstream_profile_secrets
        .read()
        .get(profile_id)
        .cloned()
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
                        .iter()
                        .find(|s| s.id == k.id)
                        .map(|s| s.secret.clone())
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
                }
            })
            .collect();

        if keys.iter().any(|k| k.id == key_id && k.secret.is_empty()) {
            return put_profile_keys_append(state, profile_id, secret, account_id).await;
        }

        put_profile_keys(state, profile_id, keys, true)
            .await
            .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e))?;
        return Ok(());
    }

    put_profile_keys_append(state, profile_id, secret, account_id).await
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
) -> Result<(), (axum::http::StatusCode, String)> {
    let key_input = UpstreamKeyInput {
        id: String::new(),
        secret: secret.to_string(),
        enabled: true,
        account_id: account_id.to_string(),
    };
    put_profile_keys(state, profile_id, vec![key_input], false)
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e))?;
    Ok(())
}
