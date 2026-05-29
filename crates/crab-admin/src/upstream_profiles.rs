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
    Ok(UpstreamProfileKeysAdminView {
        profile_id: view.profile_id,
        keys: view
            .keys
            .into_iter()
            .map(upstream_key_view_from_control)
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
