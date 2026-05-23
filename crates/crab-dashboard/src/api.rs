use crate::auth::{admin_key_header_value, handle_unauthorized};
use crate::types::*;
use gloo_net::http::{Request, RequestBuilder, Response};

const API_BASE: &str = "/api/admin";
const ADMIN_KEY_HEADER: &str = "x-admin-key";

fn apply_admin_auth(mut builder: RequestBuilder) -> (RequestBuilder, u64) {
    let epoch = crate::auth::auth_epoch();
    if let Some(key) = admin_key_header_value() {
        builder = builder.header(ADMIN_KEY_HEADER, &key);
    }
    (builder, epoch)
}

async fn http_error(resp: Response, request_epoch: u64) -> String {
    let status = resp.status();
    if status == 401 {
        if admin_key_header_value().is_some() {
            handle_unauthorized(request_epoch);
        }
        return "unauthorized".to_string();
    }

    let body = resp.text().await.unwrap_or_default();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(msg) = value.get("error").and_then(|v| v.as_str()) {
            return msg.to_string();
        }
    }
    if !body.is_empty() {
        return body;
    }

    format!("HTTP {}", status)
}

/// Check admin key against the server before entering the authenticated shell.
pub async fn verify_admin_key(key: &str) -> Result<(), String> {
    let url = format!("{}/overview", API_BASE);
    let resp = Request::get(&url)
        .header(ADMIN_KEY_HEADER, key)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    match resp.status() {
        401 => Err("invalid_admin_key".to_string()),
        status if !resp.ok() => Err(format!("HTTP {}", status)),
        _ => Ok(()),
    }
}

async fn fetch_json<T: for<'de> serde::Deserialize<'de>>(url: &str) -> Result<T, String> {
    let (builder, epoch) = apply_admin_auth(Request::get(url));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

async fn post_json<T: for<'de> serde::Deserialize<'de>, B: serde::Serialize>(
    url: &str,
    body: &B,
) -> Result<T, String> {
    let (builder, epoch) = apply_admin_auth(Request::post(url));
    let resp = builder
        .json(body)
        .map_err(|e| format!("Serialization error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

async fn delete_json(url: &str) -> Result<(), String> {
    let (builder, epoch) = apply_admin_auth(Request::delete(url));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    Ok(())
}

async fn put_json<T: for<'de> serde::Deserialize<'de>, B: serde::Serialize>(
    url: &str,
    body: &B,
) -> Result<T, String> {
    let (builder, epoch) = apply_admin_auth(Request::put(url));
    let resp = builder
        .json(body)
        .map_err(|e| format!("Serialization error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

async fn patch_json<T: for<'de> serde::Deserialize<'de>, B: serde::Serialize>(
    url: &str,
    body: &B,
) -> Result<T, String> {
    let (builder, epoch) = apply_admin_auth(Request::patch(url));
    let resp = builder
        .json(body)
        .map_err(|e| format!("Serialization error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

pub async fn fetch_prefix_cache_metrics() -> Result<PrefixCacheMetricsSnapshot, String> {
    fetch_json(&format!("{API_BASE}/metrics/prefix-cache")).await
}

pub async fn fetch_metrics() -> Result<MetricsSnapshot, String> {
    fetch_json(&format!("{}/metrics", API_BASE)).await
}

pub async fn fetch_overview() -> Result<crate::types::OverviewBundle, String> {
    fetch_json(&format!("{}/overview", API_BASE)).await
}

pub async fn fetch_domains() -> Result<Vec<DomainMetricsBucket>, String> {
    fetch_json(&format!("{}/domains", API_BASE)).await
}

pub async fn fetch_domain_detail(domain: &str) -> Result<DomainDetailBundle, String> {
    fetch_json(&format!("{}/domains/{}", API_BASE, domain)).await
}

pub async fn fetch_domain_policies() -> Result<Vec<DomainPolicy>, String> {
    fetch_json(&format!("{}/domains/policies", API_BASE)).await
}

pub async fn put_domain_policies(policies: Vec<DomainPolicy>) -> Result<Vec<DomainPolicy>, String> {
    put_json(&format!("{}/domains/policies", API_BASE), &policies).await
}

pub async fn upsert_domain_policy(policy: DomainPolicy) -> Result<Vec<DomainPolicy>, String> {
    let mut policies = fetch_domain_policies().await?;
    if let Some(existing) = policies.iter_mut().find(|p| p.domain == policy.domain) {
        *existing = policy;
    } else {
        policies.push(policy);
    }
    put_domain_policies(policies).await
}

pub async fn delete_domain_policy(domain: &str) -> Result<Vec<DomainPolicy>, String> {
    let policies = fetch_domain_policies()
        .await?
        .into_iter()
        .filter(|p| p.domain != domain)
        .collect();
    put_domain_policies(policies).await
}

pub async fn fetch_gateway_health() -> Result<GatewayHealth, String> {
    fetch_json(&format!("{}/gateway/health", API_BASE)).await
}

pub async fn fetch_network_info() -> Result<NetworkInfo, String> {
    fetch_json(&format!("{}/network/info", API_BASE)).await
}

pub async fn fetch_keys() -> Result<Vec<ApiKey>, String> {
    fetch_json(&format!("{}/keys", API_BASE)).await
}

fn percent_encode_query(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", b);
            }
        }
    }
    out
}

pub async fn fetch_live_metrics(
    consumer: &str,
    window_secs: u32,
) -> Result<crate::types::LiveMetricsResponse, String> {
    let url = format!(
        "{}/live-metrics?consumer={}&window_secs={}&bucket_secs=5",
        API_BASE,
        percent_encode_query(consumer),
        window_secs
    );
    fetch_json(&url).await
}

/// Fetch available consumers from the lightweight live-metrics/consumers endpoint.
pub async fn fetch_live_consumers(
    window_secs: u32,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/live-metrics/consumers?window_secs={}",
        API_BASE,
        window_secs
    );
    fetch_json(&url).await
}

pub async fn create_key(req: &CreateKeyRequest) -> Result<ApiKey, String> {
    post_json(&format!("{}/keys", API_BASE), req).await
}

pub async fn revoke_key(id: &str) -> Result<(), String> {
    delete_json(&format!("{}/keys/{}", API_BASE, id)).await
}

pub async fn batch_revoke_keys(ids: &[String]) -> Result<serde_json::Value, String> {
    post_json(
        &format!("{}/keys/batch-revoke", API_BASE),
        &serde_json::json!({ "ids": ids }),
    )
    .await
}

pub async fn patch_key(id: &str, req: &PatchKeyRequest) -> Result<ApiKey, String> {
    patch_json(&format!("{}/keys/{}", API_BASE, id), req).await
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

pub async fn fetch_upstream_config() -> Result<UpstreamConfig, String> {
    fetch_json(&format!("{}/upstream/config", API_BASE)).await
}

pub async fn update_upstream_config(
    req: &UpdateUpstreamConfigRequest,
) -> Result<UpdateUpstreamConfigResponse, String> {
    put_json(&format!("{}/upstream/config", API_BASE), req).await
}

pub async fn test_upstream_connection(
    body: &UpstreamTestBody,
) -> Result<UpstreamTestResult, String> {
    post_json(&format!("{}/upstream/test", API_BASE), body).await
}

pub async fn detect_models() -> Result<ModelDetectResponse, String> {
    #[derive(serde::Serialize)]
    struct EmptyBody {}
    post_json(&format!("{}/models/detect", API_BASE), &EmptyBody {}).await
}

pub async fn apply_models(body: &ModelApplyBody) -> Result<SyncResult, String> {
    post_json(&format!("{}/models/apply", API_BASE), body).await
}

pub async fn fetch_upstream_keys() -> Result<UpstreamKeysView, String> {
    fetch_json(&format!("{}/upstream/keys", API_BASE)).await
}

pub async fn put_upstream_keys(req: &PutUpstreamKeysRequest) -> Result<UpstreamKeysView, String> {
    put_json(&format!("{}/upstream/keys", API_BASE), req).await
}

pub async fn patch_upstream_key(
    id: &str,
    req: &PatchUpstreamKeyRequest,
) -> Result<UpstreamKeyView, String> {
    patch_json(&format!("{}/upstream/keys/{id}", API_BASE), req).await
}

pub async fn fetch_models() -> Result<ModelListResponse, String> {
    fetch_json(&format!("{}/models", API_BASE)).await
}

pub async fn sync_models() -> Result<SyncResult, String> {
    let (builder, epoch) = apply_admin_auth(Request::post(&format!("{}/models", API_BASE)));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    resp.json::<SyncResult>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
}

pub async fn fetch_routing_status() -> Result<RoutingStatus, String> {
    fetch_json(&format!("{}/routing/status", API_BASE)).await
}

pub async fn fetch_logs(
    limit: Option<usize>,
    cursor: Option<&str>,
) -> Result<LogsPageResponse, String> {
    let mut path = format!("{}/logs", API_BASE);
    let mut params = Vec::new();
    if let Some(l) = limit {
        params.push(format!("limit={}", l));
    }
    if let Some(c) = cursor.filter(|s| !s.is_empty()) {
        params.push(format!("cursor={}", c));
    }
    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }
    fetch_json(&path).await
}

pub async fn fetch_log_detail(id: &str) -> Result<RequestDetail, String> {
    fetch_json(&format!("{}/logs/{}", API_BASE, id)).await
}

pub async fn fetch_trace_analysis(hours: u32) -> Result<TraceAnalysis, String> {
    fetch_json(&format!("{}/trace/analysis?hours={}", API_BASE, hours)).await
}

pub async fn fetch_cache_ops() -> Result<CacheOpsView, String> {
    fetch_json(&format!("{}/cache/ops", API_BASE)).await
}

pub async fn invalidate_cache(req: &InvalidateCacheBody) -> Result<InvalidateCacheResult, String> {
    post_json(&format!("{}/cache/invalidate", API_BASE), req).await
}

pub async fn update_fingerprint(
    req: &FingerprintConfigBody,
) -> Result<FingerprintConfigBody, String> {
    put_json(&format!("{}/cache/fingerprint", API_BASE), req).await
}

pub async fn update_stream_cache(req: &StreamCacheToggle) -> Result<StreamCacheToggle, String> {
    put_json(&format!("{}/cache/stream_cache", API_BASE), req).await
}

pub async fn fetch_pipeline_runtime() -> Result<PipelineRuntimeConfig, String> {
    fetch_json(&format!("{}/runtime/pipeline", API_BASE)).await
}

pub async fn update_pipeline_runtime(
    req: &PipelineRuntimeConfig,
) -> Result<PipelineRuntimeConfig, String> {
    put_json(&format!("{}/runtime/pipeline", API_BASE), req).await
}

pub async fn fetch_cursor_models() -> Result<CursorModelsConfig, String> {
    fetch_json(&format!("{}/cursor/models", API_BASE)).await
}

pub async fn update_cursor_models(req: &CursorModelsConfig) -> Result<CursorModelsConfig, String> {
    put_json(&format!("{}/cursor/models", API_BASE), req).await
}

pub async fn update_routing_backends(req: &PutBackendsRequest) -> Result<RoutingStatus, String> {
    put_json(&format!("{}/routing/backends", API_BASE), req).await
}

pub async fn fetch_reasoning_config() -> Result<ReasoningConfig, String> {
    fetch_json(&format!("{}/reasoning/config", API_BASE)).await
}

pub async fn update_reasoning_config(req: &ReasoningConfig) -> Result<ReasoningConfig, String> {
    put_json(&format!("{}/reasoning/config", API_BASE), req).await
}

// ── System / Update ──────────────────────────────────────────────

pub async fn fetch_system_version() -> Result<SystemVersion, String> {
    fetch_json(&format!("{}/system/version", API_BASE)).await
}

pub async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    post_json(&format!("{}/system/check-update", API_BASE), &serde_json::json!({})).await
}

pub async fn trigger_system_update() -> Result<SystemUpdateResult, String> {
    post_json(&format!("{}/system/update", API_BASE), &serde_json::json!({})).await
}

pub async fn change_admin_key(
    old_key: &str,
    new_key: &str,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "old_key": old_key,
        "new_key": new_key,
    });
    put_json(&format!("{}/system/admin-key", API_BASE), &body).await
}
