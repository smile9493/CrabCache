use crate::types::*;
use gloo_net::http::Request;

const API_BASE: &str = "http://127.0.0.1:3000/api/admin";

async fn fetch_json<T: for<'de> serde::Deserialize<'de>>(url: &str) -> Result<T, String> {
    let resp = Request::get(url)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

async fn post_json<T: for<'de> serde::Deserialize<'de>, B: serde::Serialize>(
    url: &str,
    body: &B,
) -> Result<T, String> {
    let resp = Request::post(url)
        .json(body)
        .map_err(|e| format!("Serialization error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

async fn delete_json(url: &str) -> Result<(), String> {
    let resp = Request::delete(url)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    Ok(())
}

async fn put_json<T: for<'de> serde::Deserialize<'de>, B: serde::Serialize>(
    url: &str,
    body: &B,
) -> Result<T, String> {
    let resp = Request::put(url)
        .json(body)
        .map_err(|e| format!("Serialization error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

pub async fn fetch_metrics() -> Result<MetricsSnapshot, String> {
    fetch_json(&format!("{}/metrics", API_BASE)).await
}

pub async fn fetch_keys() -> Result<Vec<ApiKey>, String> {
    fetch_json(&format!("{}/keys", API_BASE)).await
}

pub async fn create_key(req: &CreateKeyRequest) -> Result<ApiKey, String> {
    post_json(&format!("{}/keys", API_BASE), req).await
}

pub async fn revoke_key(id: &str) -> Result<(), String> {
    delete_json(&format!("{}/keys/{}", API_BASE, id)).await
}

pub async fn fetch_cache_config() -> Result<CacheConfig, String> {
    fetch_json(&format!("{}/cache/config", API_BASE)).await
}

pub async fn update_cache_config(req: &UpdateCacheConfigRequest) -> Result<CacheConfig, String> {
    put_json(&format!("{}/cache/config", API_BASE), req).await
}

pub async fn fetch_semantic_config() -> Result<SemanticConfig, String> {
    fetch_json(&format!("{}/semantic/config", API_BASE)).await
}

pub async fn update_semantic_config(
    req: &UpdateSemanticConfigRequest,
) -> Result<SemanticConfig, String> {
    put_json(&format!("{}/semantic/config", API_BASE), req).await
}

pub async fn fetch_connection_config() -> Result<ConnectionConfig, String> {
    fetch_json(&format!("{}/connection/config", API_BASE)).await
}

pub async fn update_connection_config(
    req: &UpdateConnectionConfigRequest,
) -> Result<ConnectionConfig, String> {
    put_json(&format!("{}/connection/config", API_BASE), req).await
}

pub async fn fetch_models() -> Result<ModelListResponse, String> {
    fetch_json(&format!("{}/models", API_BASE)).await
}

pub async fn sync_models() -> Result<SyncResult, String> {
    let resp = Request::post(&format!("{}/models", API_BASE))
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json::<SyncResult>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

pub async fn fetch_routing_status() -> Result<RoutingStatus, String> {
    fetch_json(&format!("{}/routing/status", API_BASE)).await
}

pub async fn fetch_logs() -> Result<Vec<RequestLog>, String> {
    fetch_json(&format!("{}/logs", API_BASE)).await
}

pub async fn fetch_log_detail(id: &str) -> Result<RequestDetail, String> {
    fetch_json(&format!("{}/logs/{}", API_BASE, id)).await
}