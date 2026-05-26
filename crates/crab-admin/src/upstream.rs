//! Upstream test and model sync helpers.

use crate::state::AppState;
use crate::types::{SyncResult, UpstreamModelsResponse, UpstreamTestResult};
use crab_control::{parse_upstream_base_url, validate_upstream_key};
use std::sync::Arc;
use std::time::Instant;

pub async fn test_upstream_connection(base_url: &str, api_key: &str) -> UpstreamTestResult {
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
    // #region agent log
    {
        use std::io::Write;
        let key_preview = if api_key.len() > 12 {
            format!("{}...{}", &api_key[..4], &api_key[api_key.len() - 4..])
        } else {
            "***".to_string()
        };
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/opt/projct/CrabCache/.cursor/debug-ec43cd.log")
        {
            let _ = writeln!(
                f,
                "{{\"sessionId\":\"ec43cd\",\"location\":\"upstream.rs:32\",\"message\":\"admin test_upstream_connection: sending request\",\"data\":{{\"base_url\":\"{}\",\"url\":\"{}\",\"key_preview\":\"{}\",\"key_len\":{}}},\"timestamp\":{}}}",
                base_url,
                url,
                key_preview,
                api_key.len(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
        }
    }
    // #endregion
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
    // #region agent log
    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/opt/projct/CrabCache/.cursor/debug-ec43cd.log")
        {
            let _ = writeln!(
                f,
                "{{\"sessionId\":\"ec43cd\",\"location\":\"upstream.rs:70\",\"message\":\"admin test_upstream_connection: response received\",\"data\":{{\"status_code\":{},\"latency_ms\":{}}},\"timestamp\":{}}}",
                status_code,
                latency_ms,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
        }
    }
    // #endregion
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

pub async fn profile_base_url_async(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<String, String> {
    if profile_id == "deepseek" {
        let cfg = state.upstream_config.read();
        return Ok(cfg.base_url.clone());
    }
    let list = state
        .gateway
        .list_upstream_profiles()
        .await
        .map_err(|e| e.to_string())?;
    list.profiles
        .into_iter()
        .find(|p| p.id == profile_id)
        .map(|p| p.base_url)
        .ok_or_else(|| format!("unknown upstream profile '{profile_id}'"))
}

pub async fn sync_models_internal(
    state: &Arc<AppState>,
    profile_id: &str,
) -> Result<SyncResult, String> {
    let base_url = profile_base_url_async(state, profile_id).await?;
    let api_key = state.pick_sync_api_key(profile_id).ok_or_else(|| {
        format!("Configure API keys for profile '{profile_id}' before syncing models.")
    })?;

    let upstream = fetch_upstream_models(&base_url, &api_key).await?;
    let upstream_ids: Vec<String> = upstream.data.iter().map(|m| m.id.clone()).collect();
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

    for m in upstream.data {
        let existing = existing_for_profile.iter().find(|e| e.id == m.id);
        stored.models.push(crate::state::StoredModel {
            profile_id: profile_id.to_string(),
            id: m.id,
            owned_by: if m.owned_by.is_empty() {
                owned_by_default.clone()
            } else {
                m.owned_by
            },
            context_length: existing.and_then(|e| e.context_length),
            input_price_per_mtok: existing.and_then(|e| e.input_price_per_mtok),
            output_price_per_mtok: existing.and_then(|e| e.output_price_per_mtok),
            available: true,
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

    drop(stored);
    state.flush_persist();

    Ok(SyncResult {
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
    let base_url = profile_base_url_async(state, profile_id).await?;
    let api_key = state
        .pick_sync_api_key(profile_id)
        .ok_or_else(|| format!("Configure API keys for profile '{profile_id}' first."))?;

    let upstream = fetch_upstream_models(&base_url, &api_key).await?;
    let upstream_ids: Vec<String> = upstream.data.iter().map(|m| m.id.clone()).collect();
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
