//! Upstream test and model sync helpers.

use crate::state::AppState;
use crate::types::{SyncResult, UpstreamModelsResponse};
use crab_control::{parse_upstream_base_url, validate_deepseek_key, UpstreamTestResult};
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
        };
    }
    if let Err(e) = validate_deepseek_key(api_key) {
        return UpstreamTestResult {
            ok: false,
            status_code: 0,
            latency_ms: 0,
            model_count: None,
            error: Some(e),
        };
    }

    let url = format!(
        "{}/v1/models",
        base_url.trim().trim_end_matches('/')
    );
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
            };
        }
    };

    UpstreamTestResult {
        ok: true,
        status_code,
        latency_ms,
        model_count: Some(upstream.data.len()),
        error: None,
    }
}

pub async fn sync_models_internal(state: &Arc<AppState>) -> Result<SyncResult, String> {
    let upstream_config = state.upstream_config.read().clone();
    let upstream_url = format!(
        "{}/v1/models",
        upstream_config.base_url.trim_end_matches('/')
    );
    let api_key = state.pick_sync_api_key().ok_or_else(|| {
        "Configure upstream key pool or set a sync API key.".to_string()
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let resp = client
        .get(&upstream_url)
        .header("Authorization", format!("Bearer {}", &api_key))
        .send()
        .await
        .map_err(|e| format!("Cannot reach upstream: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Upstream returned {}: {}", status.as_u16(), body.chars().take(200).collect::<String>()));
    }

    let upstream: UpstreamModelsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse upstream response: {e}"))?;

    let upstream_ids: Vec<String> = upstream.data.iter().map(|m| m.id.clone()).collect();

    let mut stored = state.models.write();
    let existing_ids: Vec<String> = stored.models.iter().map(|m| m.id.clone()).collect();

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

    let upstream_models: Vec<crate::state::StoredModel> = upstream
        .data
        .into_iter()
        .map(|m| {
            let existing = stored.models.iter().find(|e| e.id == m.id);
            crate::state::StoredModel {
                id: m.id,
                owned_by: m.owned_by,
                context_length: existing.and_then(|e| e.context_length),
                input_price_per_mtok: existing.and_then(|e| e.input_price_per_mtok),
                output_price_per_mtok: existing.and_then(|e| e.output_price_per_mtok),
                available: true,
            }
        })
        .collect();

    let total = upstream_models.len();
    stored.models = upstream_models;
    stored.synced_at = Some(
        chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string(),
    );

    drop(stored);
    state.flush_persist();

    Ok(SyncResult {
        added,
        removed,
        unchanged,
        total,
    })
}

pub async fn detect_models_internal(state: &Arc<AppState>) -> Result<crate::types::ModelDetectResponse, String> {
    let upstream_config = state.upstream_config.read().clone();
    let upstream_url = format!(
        "{}/v1/models",
        upstream_config.base_url.trim_end_matches('/')
    );
    let api_key = state.pick_sync_api_key().ok_or_else(|| {
        "Configure upstream key pool before detecting models.".to_string()
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let resp = client
        .get(&upstream_url)
        .header("Authorization", format!("Bearer {}", &api_key))
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

    let upstream: UpstreamModelsResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse upstream response: {e}"))?;

    let upstream_ids: Vec<String> = upstream.data.iter().map(|m| m.id.clone()).collect();
    let stored = state.models.read();
    let existing_ids: Vec<String> = stored.models.iter().map(|m| m.id.clone()).collect();

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
    add: Vec<String>,
    remove: Vec<String>,
) -> SyncResult {
    let mut stored = state.models.write();
    let mut existing_ids: Vec<String> = stored.models.iter().map(|m| m.id.clone()).collect();

    for id in &remove {
        stored.models.retain(|m| &m.id != id);
        existing_ids.retain(|e| e != id);
    }

    let mut added = Vec::new();
    for id in add {
        if !existing_ids.contains(&id) {
            stored.models.push(crate::state::StoredModel {
                id: id.clone(),
                owned_by: "deepseek".to_string(),
                context_length: None,
                input_price_per_mtok: None,
                output_price_per_mtok: None,
                available: true,
            });
            existing_ids.push(id.clone());
            added.push(id);
        }
    }

    let total = stored.models.len();
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
