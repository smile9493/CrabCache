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
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body)
        && let Some(msg) = value.get("error").and_then(|v| v.as_str())
    {
        return msg.to_string();
    }
    if !body.is_empty() {
        return body;
    }

    format!("HTTP {}", status)
}

/// Check admin key against the server before entering the authenticated shell.
pub async fn verify_admin_key(key: &str) -> Result<(), String> {
    let url = format!("{}/overview/core", API_BASE);
    let resp = Request::get(&url)
        .header(ADMIN_KEY_HEADER, key)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    match resp.status() {
        401 => Err("invalid_admin_key".to_string()),
        304 => Ok(()),
        status if !resp.ok() => Err(format!("HTTP {}", status)),
        _ => Ok(()),
    }
}

pub async fn fetch_json<T: for<'de> serde::Deserialize<'de>>(url: &str) -> Result<T, String> {
    let (builder, epoch) = apply_admin_auth(Request::get(url));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap_or_default()
        .to_string();
    let body = resp.text().await.unwrap_or_default();
    serde_json::from_str::<T>(&body).map_err(|e| {
        let prefix: String = body.chars().take(200).collect();
        let hint = if prefix.trim_start().starts_with("<!doctype")
            || prefix.trim_start().starts_with("<html")
            || prefix.trim_start().starts_with("<")
        {
            " (looks like HTML; route may be missing and fell through to index.html)"
        } else {
            ""
        };
        format!(
            "Parse error: {} (HTTP {}, content-type: {}){}; url={}; body_prefix={:?}",
            e, status, ct, hint, url, prefix
        )
    })
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

    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap_or_default()
        .to_string();
    let body = resp.text().await.unwrap_or_default();
    serde_json::from_str::<T>(&body).map_err(|e| {
        let prefix: String = body.chars().take(200).collect();
        format!(
            "Parse error: {} (HTTP {}, content-type: {}); body_prefix={:?}",
            e, status, ct, prefix
        )
    })
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

    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap_or_default()
        .to_string();
    let body = resp.text().await.unwrap_or_default();
    serde_json::from_str::<T>(&body).map_err(|e| {
        let prefix: String = body.chars().take(200).collect();
        format!(
            "Parse error: {} (HTTP {}, content-type: {}); body_prefix={:?}",
            e, status, ct, prefix
        )
    })
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

    let status = resp.status();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap_or_default()
        .to_string();
    let body = resp.text().await.unwrap_or_default();
    serde_json::from_str::<T>(&body).map_err(|e| {
        let prefix: String = body.chars().take(200).collect();
        format!(
            "Parse error: {} (HTTP {}, content-type: {}); body_prefix={:?}",
            e, status, ct, prefix
        )
    })
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

/// Result of an ETag-aware overview/core fetch.
pub struct OverviewCoreResult {
    pub core: Option<crate::types::OverviewCore>,
    pub etag: String,
}

/// Fetch overview/core with If-None-Match for 304 support.
/// When the server returns 304, `core` is `None` and the caller should not update signals.
pub async fn fetch_overview_core(current_etag: &str) -> Result<OverviewCoreResult, String> {
    let url = format!("{}/overview/core", API_BASE);
    let (builder, epoch) = apply_admin_auth(Request::get(&url));
    let builder = if !current_etag.is_empty() {
        builder.header("If-None-Match", current_etag)
    } else {
        builder
    };
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let new_etag = resp.headers().get("etag").unwrap_or_default();

    if resp.status() == 304 {
        return Ok(OverviewCoreResult {
            core: None,
            etag: new_etag,
        });
    }

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    let core: crate::types::OverviewCore = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    Ok(OverviewCoreResult {
        core: Some(core),
        etag: new_etag,
    })
}

pub async fn fetch_overview_timeseries(
    window: &str,
) -> Result<crate::types::OverviewTimeseriesResponse, String> {
    fetch_json(&format!(
        "{}/overview/timeseries?window={}",
        API_BASE, window
    ))
    .await
}

/// Result of an ETag-aware timeseries fetch.
pub struct TimeseriesResult {
    pub points: Option<Vec<crate::types::TimeSeriesPoint>>,
    pub etag: String,
}

/// Fetch overview/timeseries with If-None-Match for 304 support.
pub async fn fetch_overview_timeseries_etag(
    window: &str,
    current_etag: &str,
) -> Result<TimeseriesResult, String> {
    let url = format!("{}/overview/timeseries?window={}", API_BASE, window);
    let (builder, epoch) = apply_admin_auth(Request::get(&url));
    let builder = if !current_etag.is_empty() {
        builder.header("If-None-Match", current_etag)
    } else {
        builder
    };
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let new_etag = resp.headers().get("etag").unwrap_or_default();

    if resp.status() == 304 {
        return Ok(TimeseriesResult {
            points: None,
            etag: new_etag,
        });
    }

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    let data: crate::types::OverviewTimeseriesResponse = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    Ok(TimeseriesResult {
        points: Some(data.points),
        etag: new_etag,
    })
}

pub async fn fetch_overview_trace() -> Result<crate::types::TraceSummary, String> {
    fetch_json(&format!("{}/overview/trace", API_BASE)).await
}

pub async fn fetch_model_peak_hours(
    days: u32,
) -> Result<crate::types::ModelPeakHoursResponse, String> {
    fetch_json(&format!(
        "{}/analytics/model-peak-hours?days={}",
        API_BASE, days
    ))
    .await
}

pub async fn delete_model_peak_hour(
    model: &str,
    hour_bucket: i64,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/analytics/model-peak-hours?model={}&hour_bucket={}",
        API_BASE,
        urlencoding::encode(model),
        hour_bucket
    );
    let (builder, epoch) = apply_admin_auth(Request::delete(&url));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Parse error: {}", e))
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

/// Upsert a single domain policy via read-modify-write.
///
/// **Note**: This is not atomic — concurrent calls may lose updates.
/// The UI disables the save button while a request is in-flight to mitigate this.
pub async fn upsert_domain_policy(policy: DomainPolicy) -> Result<Vec<DomainPolicy>, String> {
    let mut policies = fetch_domain_policies().await?;
    if let Some(existing) = policies.iter_mut().find(|p| p.domain == policy.domain) {
        *existing = policy;
    } else {
        policies.push(policy);
    }
    put_domain_policies(policies).await
}

/// Delete a single domain policy via read-modify-write.
///
/// **Note**: Same concurrency caveat as `upsert_domain_policy`.
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

/// All gateway keys including sync/reconcile artifacts (for live monitoring dropdowns).
pub async fn fetch_keys_include_synced() -> Result<Vec<ApiKey>, String> {
    fetch_json(&format!("{}/keys?include_synced=true", API_BASE)).await
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
    fetch_live_metrics_v2(consumer, window_secs, &[], "", "").await
}

/// v2 fetch with group_by, key_id, and session_fingerprint support.
pub async fn fetch_live_metrics_v2(
    consumer: &str,
    window_secs: u32,
    group_by: &[&str],
    key_id: &str,
    session_fingerprint: &str,
) -> Result<crate::types::LiveMetricsResponse, String> {
    let bucket_secs = if window_secs <= 3600 {
        5
    } else if window_secs <= 12 * 3600 {
        60
    } else if window_secs <= 24 * 3600 {
        300
    } else if window_secs <= 3 * 24 * 3600 {
        900
    } else if window_secs <= 7 * 24 * 3600 {
        1800
    } else {
        3600
    };
    let mut url = format!(
        "{}/live-metrics?consumer={}&window_secs={}&bucket_secs={}",
        API_BASE,
        percent_encode_query(consumer),
        window_secs,
        bucket_secs
    );
    for gb in group_by {
        url.push_str("&group_by=");
        url.push_str(&percent_encode_query(gb));
    }
    if !key_id.is_empty() {
        url.push_str("&key_id=");
        url.push_str(&percent_encode_query(key_id));
    }
    if !session_fingerprint.is_empty() {
        url.push_str("&session_fingerprint=");
        url.push_str(&percent_encode_query(session_fingerprint));
    }
    fetch_json(&url).await
}

/// Fetch available consumers from the lightweight live-metrics/consumers endpoint.
pub async fn fetch_live_consumers(window_secs: u32) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/live-metrics/consumers?window_secs={}",
        API_BASE, window_secs
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

pub async fn prune_duplicate_keys(
    dry_run: bool,
) -> Result<crab_admin_types::PruneDuplicateKeysResponse, String> {
    post_json(
        &format!("{}/keys/prune-duplicates", API_BASE),
        &serde_json::json!({ "dry_run": dry_run }),
    )
    .await
}

pub async fn patch_key(id: &str, req: &PatchKeyRequest) -> Result<ApiKey, String> {
    patch_json(&format!("{}/keys/{}", API_BASE, id), req).await
}

pub async fn fetch_key_concurrency(
    id: &str,
    window_secs: u32,
) -> Result<crate::types::KeyConcurrencyResponse, String> {
    fetch_json(&format!(
        "{}/keys/{}/concurrency?window_secs={}",
        API_BASE, id, window_secs
    ))
    .await
}

pub async fn fetch_key_routing(
    id: &str,
    window_secs: u32,
) -> Result<crate::types::KeyRoutingResponse, String> {
    fetch_json(&format!(
        "{}/keys/{}/routing?window_secs={}",
        API_BASE, id, window_secs
    ))
    .await
}

pub async fn fetch_session_timeline(
    fingerprint: &str,
) -> Result<crate::types::SessionTimelineResponse, String> {
    fetch_json(&format!("{}/sessions/{}", API_BASE, fingerprint)).await
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

pub async fn fetch_limits_config() -> Result<LimitsConfig, String> {
    fetch_json(&format!("{}/config/limits", API_BASE)).await
}

pub async fn update_limits_config(req: &LimitsConfig) -> Result<LimitsConfig, String> {
    put_json(&format!("{}/config/limits", API_BASE), req).await
}

pub async fn fetch_cache_pricing_config() -> Result<PricingConfigView, String> {
    fetch_json(&format!("{}/cache/pricing", API_BASE)).await
}

pub async fn update_cache_pricing_config(
    req: &PricingConfigView,
) -> Result<PricingConfigView, String> {
    put_json(&format!("{}/cache/pricing", API_BASE), req).await
}

pub async fn fetch_features_config() -> Result<FeaturesConfigView, String> {
    fetch_json(&format!("{}/config/features", API_BASE)).await
}

pub async fn update_features_config(
    req: &FeaturesConfigView,
) -> Result<FeaturesConfigView, String> {
    put_json(&format!("{}/config/features", API_BASE), req).await
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

pub async fn detect_models(profile_id: &str) -> Result<ModelDetectResponse, String> {
    #[derive(serde::Serialize)]
    struct EmptyBody {}
    post_json(
        &format!(
            "{}/models/detect?profile_id={}",
            API_BASE,
            urlencoding::encode(profile_id)
        ),
        &EmptyBody {},
    )
    .await
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

pub async fn fetch_models(profile_id: Option<&str>) -> Result<ModelListResponse, String> {
    let url = match profile_id {
        Some(id) => format!("{}/models?profile_id={}", API_BASE, urlencoding::encode(id)),
        None => format!("{}/models", API_BASE),
    };
    fetch_json(&url).await
}

pub async fn fetch_upstream_profiles() -> Result<crate::types::UpstreamProfilesAdminResponse, String>
{
    fetch_json(&format!("{}/upstream/profiles", API_BASE)).await
}

pub async fn put_upstream_profile(
    id: &str,
    req: &crate::types::PutUpstreamProfileAdminRequest,
) -> Result<crate::types::UpstreamProfileAdminView, String> {
    put_json(&format!("{}/upstream/profiles/{id}", API_BASE), req).await
}

pub async fn delete_upstream_profile(id: &str) -> Result<(), String> {
    delete_json(&format!("{}/upstream/profiles/{id}", API_BASE)).await
}

pub async fn test_upstream_profile(id: &str) -> Result<UpstreamTestResult, String> {
    #[derive(serde::Serialize)]
    struct EmptyBody {}
    post_json(
        &format!("{}/upstream/profiles/{id}/test", API_BASE),
        &EmptyBody {},
    )
    .await
}

pub async fn test_upstream_profile_key(
    profile_id: &str,
    key_id: &str,
) -> Result<UpstreamTestResult, String> {
    #[derive(serde::Serialize)]
    struct EmptyBody {}
    post_json(
        &format!(
            "{}/upstream/profiles/{profile_id}/keys/{key_id}/test",
            API_BASE
        ),
        &EmptyBody {},
    )
    .await
}

pub async fn fetch_upstream_profile_keys(
    id: &str,
) -> Result<crate::types::UpstreamProfileKeysAdminView, String> {
    fetch_json(&format!("{}/upstream/profiles/{id}/keys", API_BASE)).await
}

pub async fn put_upstream_profile_keys(
    id: &str,
    req: &PutUpstreamKeysRequest,
) -> Result<crate::types::UpstreamProfileKeysAdminView, String> {
    put_json(&format!("{}/upstream/profiles/{id}/keys", API_BASE), req).await
}

pub async fn patch_upstream_profile_key(
    profile_id: &str,
    key_id: &str,
    req: &PatchUpstreamKeyRequest,
) -> Result<UpstreamKeyView, String> {
    patch_json(
        &format!("{}/upstream/profiles/{profile_id}/keys/{key_id}", API_BASE),
        req,
    )
    .await
}

pub async fn delete_upstream_profile_key(profile_id: &str, key_id: &str) -> Result<(), String> {
    delete_json(&format!(
        "{}/upstream/profiles/{profile_id}/keys/{key_id}",
        API_BASE
    ))
    .await
}

pub async fn delete_upstream_key(key_id: &str) -> Result<(), String> {
    delete_json(&format!("{}/upstream/keys/{key_id}", API_BASE)).await
}

pub async fn fetch_profile_routing(profile_id: &str) -> Result<ProfileRoutingView, String> {
    fetch_json(&format!(
        "{}/upstream/profiles/{}/routing",
        API_BASE,
        urlencoding::encode(profile_id)
    ))
    .await
}

pub async fn fetch_routing_summary() -> Result<RoutingSummaryView, String> {
    // NOTE: `/routing/summary` no longer exists on admin BFF; unknown routes can
    // fall through to index.html (HTTP 200), which causes JSON parse failures.
    // Build lightweight summary from profile routing. Try `default` first for
    // backward compatibility, then fall back to configured default/first profile.
    let profile = match fetch_profile_routing("default").await {
        Ok(profile) => profile,
        Err(_) => {
            let profiles = fetch_upstream_profiles().await?;
            let fallback_profile_id = if !profiles.default_profile_id.is_empty() {
                profiles.default_profile_id
            } else {
                profiles
                    .profiles
                    .first()
                    .map(|p| p.id.clone())
                    .ok_or_else(|| "No upstream profile available".to_string())?
            };
            fetch_profile_routing(&fallback_profile_id).await?
        }
    };
    let backends_total = profile.backends.len();
    let backends_healthy = profile.backends.iter().filter(|b| b.healthy).count();

    Ok(RoutingSummaryView {
        circuit_open_count: 0,
        backends_healthy,
        backends_total,
        upstream_keys_available: profile.key_pool.available,
        upstream_keys_total: profile.key_pool.total,
        profile_id: profile.profile_id,
    })
}

pub async fn fetch_routing_profiles() -> Result<Vec<ProfileRoutingView>, String> {
    let profiles_resp = fetch_upstream_profiles().await?;
    let mut ids: Vec<String> = profiles_resp.profiles.into_iter().map(|p| p.id).collect();
    if ids.is_empty() {
        return Err("No upstream profile available".to_string());
    }
    if !profiles_resp.default_profile_id.is_empty() {
        ids.sort();
        ids.dedup();
        if let Some(pos) = ids
            .iter()
            .position(|id| id == &profiles_resp.default_profile_id)
        {
            let default = ids.remove(pos);
            ids.insert(0, default);
        }
    }

    let mut views = Vec::with_capacity(ids.len());
    let mut last_err: Option<String> = None;
    for id in ids {
        match fetch_profile_routing(&id).await {
            Ok(view) => views.push(view),
            Err(e) => last_err = Some(format!("{id}: {e}")),
        }
    }
    if views.is_empty() {
        return Err(last_err.unwrap_or_else(|| "No routing data available".to_string()));
    }
    Ok(views)
}

pub async fn sync_models(profile_id: &str) -> Result<SyncResult, String> {
    let (builder, epoch) = apply_admin_auth(Request::post(&format!(
        "{}/models?profile_id={}",
        API_BASE,
        urlencoding::encode(profile_id)
    )));
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

pub async fn fetch_logs(query: &crate::types::LogsFilterQuery) -> Result<LogsPageResponse, String> {
    let mut path = format!("{}/logs", API_BASE);
    let mut params = Vec::new();
    if let Some(l) = query.limit {
        params.push(format!("limit={}", l));
    }
    if let Some(c) = query.cursor.as_deref().filter(|s| !s.is_empty()) {
        params.push(format!("cursor={}", percent_encode_query(c)));
    }
    if let Some(m) = query.model.as_deref().filter(|s| !s.is_empty()) {
        params.push(format!("model={}", percent_encode_query(m)));
    }
    if let Some(c) = query.consumer.as_deref().filter(|s| !s.is_empty()) {
        params.push(format!("consumer={}", percent_encode_query(c)));
    }
    if let Some(t) = query.cache_tier.as_deref().filter(|s| !s.is_empty()) {
        params.push(format!("cache_tier={}", percent_encode_query(t)));
    }
    if let Some(h) = query.request_hash.as_deref().filter(|s| !s.is_empty()) {
        params.push(format!("request_hash={}", percent_encode_query(h)));
    }
    if let Some(v) = query.latency_min {
        params.push(format!("latency_min={}", v));
    }
    if let Some(v) = query.latency_max {
        params.push(format!("latency_max={}", v));
    }
    if let Some(v) = query.token_min {
        params.push(format!("token_min={}", v));
    }
    if let Some(v) = query.token_max {
        params.push(format!("token_max={}", v));
    }
    if let Some(v) = query.from_ms {
        params.push(format!("from_ms={}", v));
    }
    if let Some(v) = query.to_ms {
        params.push(format!("to_ms={}", v));
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

pub struct TraceAnalysisResult {
    pub analysis: Option<TraceAnalysis>,
    pub etag: String,
}

pub async fn fetch_trace_analysis_etag(
    hours: u32,
    current_etag: &str,
) -> Result<TraceAnalysisResult, String> {
    let url = format!("{}/trace/analysis?hours={}", API_BASE, hours);
    let (builder, epoch) = apply_admin_auth(Request::get(&url));
    let builder = if !current_etag.is_empty() {
        builder.header("If-None-Match", current_etag)
    } else {
        builder
    };
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let new_etag = resp.headers().get("etag").unwrap_or_default();

    if resp.status() == 304 {
        return Ok(TraceAnalysisResult {
            analysis: None,
            etag: new_etag,
        });
    }

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    let analysis: TraceAnalysis = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    Ok(TraceAnalysisResult {
        analysis: Some(analysis),
        etag: new_etag,
    })
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
    post_json(
        &format!("{}/system/check-update", API_BASE),
        &serde_json::json!({}),
    )
    .await
}

pub async fn trigger_system_update() -> Result<SystemUpdateResult, String> {
    post_json(
        &format!("{}/system/update", API_BASE),
        &serde_json::json!({}),
    )
    .await
}

pub async fn change_admin_key(old_key: &str, new_key: &str) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "old_key": old_key,
        "new_key": new_key,
    });
    put_json(&format!("{}/system/admin-key", API_BASE), &body).await
}

// ── Composition API ──────────────────────────────────────────────

pub async fn fetch_composition_summary(
    hours: u32,
    project_id: Option<&str>,
    consumer: Option<&str>,
) -> Result<crate::types::CompositionSummaryResponse, String> {
    let mut path = format!("{}/composition/summary?hours={}", API_BASE, hours);
    if let Some(pid) = project_id.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&project_id={}", percent_encode_query(pid)));
    }
    if let Some(c) = consumer.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&consumer={}", percent_encode_query(c)));
    }
    fetch_json(&path).await
}

pub async fn fetch_composition_trends() -> Result<crate::types::CompositionTrendsResponse, String> {
    fetch_json(&format!("{}/composition/trends", API_BASE)).await
}

pub async fn fetch_composition_debug(
    hours: u32,
    limit: Option<usize>,
    request_hash: Option<&str>,
    consumer: Option<&str>,
) -> Result<crate::types::CompositionDebugResponse, String> {
    let mut path = format!("{}/composition/debug?hours={}", API_BASE, hours);
    if let Some(rh) = request_hash.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&request_hash={}", percent_encode_query(rh)));
    }
    if let Some(c) = consumer.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&consumer={}", percent_encode_query(c)));
    }
    if let Some(l) = limit {
        path.push_str(&format!("&limit={}", l));
    }
    fetch_json(&path).await
}

// ── Infra API ───────────────────────────────────────────────────

pub async fn fetch_infra_snapshot() -> Result<crate::types::InfraSnapshot, String> {
    fetch_json(&format!("{}/infra/snapshot", API_BASE)).await
}

pub async fn fetch_infra_status() -> Result<crate::types::InfraStatus, String> {
    fetch_json(&format!("{}/infra/status", API_BASE)).await
}

pub async fn fetch_infra_timeseries(
    window: &str,
    container_id: Option<&str>,
) -> Result<crate::types::InfraTimeseriesResponse, String> {
    let mut url = format!("{}/infra/timeseries?window={}", API_BASE, window);
    if let Some(id) = container_id.filter(|s| !s.is_empty()) {
        url.push_str(&format!("&container_id={}", percent_encode_query(id)));
    }
    fetch_json(&url).await
}

pub async fn post_infra_speed_test(
    direction: &str,
) -> Result<crate::types::SpeedTestAccepted, String> {
    post_json(
        &format!("{}/infra/speed-test", API_BASE),
        &serde_json::json!({ "direction": direction }),
    )
    .await
}

pub async fn fetch_infra_speed_test_job(
    job_id: &str,
) -> Result<crate::types::SpeedTestJobView, String> {
    fetch_json(&format!("{}/infra/speed-test/{}", API_BASE, job_id)).await
}

pub async fn post_infra_speed_test_upload(
    job_id: &str,
    token: &str,
    body: Vec<u8>,
) -> Result<(), String> {
    let url = format!(
        "{}/infra/speed-test/upload?job_id={}&token={}",
        API_BASE,
        percent_encode_query(job_id),
        percent_encode_query(token),
    );
    let (builder, epoch) = apply_admin_auth(gloo_net::http::Request::post(&url));
    let resp = builder
        .body(body)
        .map_err(|e| format!("Request error: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;
    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }
    Ok(())
}

// ── Raw Capture API ──────────────────────────────────────────────

pub async fn fetch_capture_list(
    hours: u32,
    limit: Option<usize>,
    consumer: Option<&str>,
    project_id: Option<&str>,
    request_hash: Option<&str>,
    session_fingerprint: Option<&str>,
    backend_name: Option<&str>,
    client_wire_api: Option<&str>,
) -> Result<crate::types::CaptureListResponse, String> {
    let mut path = format!("{}/capture/list?hours={}", API_BASE, hours);
    if let Some(l) = limit {
        path.push_str(&format!("&limit={}", l));
    }
    if let Some(c) = consumer.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&consumer={}", percent_encode_query(c)));
    }
    if let Some(pid) = project_id.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&project_id={}", percent_encode_query(pid)));
    }
    if let Some(rh) = request_hash.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&request_hash={}", percent_encode_query(rh)));
    }
    if let Some(sf) = session_fingerprint.filter(|s| !s.is_empty()) {
        path.push_str(&format!(
            "&session_fingerprint={}",
            percent_encode_query(sf)
        ));
    }
    if let Some(bn) = backend_name.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&backend_name={}", percent_encode_query(bn)));
    }
    if let Some(wire) = client_wire_api.filter(|s| !s.is_empty()) {
        path.push_str(&format!("&client_wire_api={}", percent_encode_query(wire)));
    }
    fetch_json(&path).await
}

pub async fn fetch_capture_detail(
    request_id: &str,
) -> Result<crate::types::CaptureDetailResponse, String> {
    fetch_json(&format!("{}/capture/{}", API_BASE, request_id)).await
}

pub async fn fetch_capture_stats(hours: u32) -> Result<crate::types::CaptureStatsResponse, String> {
    fetch_json(&format!("{}/capture/stats?hours={}", API_BASE, hours)).await
}

// ── Log Management ──

pub async fn fetch_log_disk_usage() -> Result<crate::types::LogDiskUsage, String> {
    fetch_json(&format!("{API_BASE}/logs/usage")).await
}

pub async fn clear_logs(
    target: &str,
    older_than_hours: Option<u32>,
) -> Result<crate::types::ClearLogsResponse, String> {
    post_json(
        &format!("{API_BASE}/logs/clear"),
        &serde_json::json!({
            "target": target,
            "older_than_hours": older_than_hours,
        }),
    )
    .await
}

pub async fn fetch_retention_policy() -> Result<crate::types::RetentionPolicy, String> {
    fetch_json(&format!("{API_BASE}/logs/retention")).await
}

pub async fn update_retention_policy(
    policy: &crate::types::RetentionPolicy,
) -> Result<crate::types::RetentionPolicy, String> {
    put_json(&format!("{API_BASE}/logs/retention"), policy).await
}

// ── Trace Logging Config ──

pub async fn fetch_trace_logging_config() -> Result<crate::types::TraceLoggingConfigView, String> {
    fetch_json(&format!("{API_BASE}/config/trace-logging")).await
}

pub async fn update_trace_logging_config(
    req: &crate::types::TraceLoggingConfigView,
) -> Result<crate::types::TraceLoggingConfigView, String> {
    put_json(&format!("{API_BASE}/config/trace-logging"), req).await
}

// ── Raw Capture Config ──

pub async fn fetch_raw_capture_config() -> Result<crate::types::RawCaptureConfigView, String> {
    fetch_json(&format!("{API_BASE}/config/raw-capture")).await
}

pub async fn update_raw_capture_config(
    req: &crate::types::RawCaptureConfigView,
) -> Result<crate::types::RawCaptureConfigView, String> {
    put_json(&format!("{API_BASE}/config/raw-capture"), req).await
}

// ── Audit Log ──

/// The backend returns `{ entries: [...] }`. We unwrap and return the inner Vec.
pub async fn fetch_audit_logs<T: for<'de> serde::Deserialize<'de>>(
    limit: i64,
    offset: i64,
    action: Option<String>,
) -> Result<Vec<T>, String> {
    let mut url = format!("{API_BASE}/audit-log?limit={limit}&offset={offset}");
    if let Some(a) = action {
        url.push_str(&format!("&action={}", a));
    }
    let resp: serde_json::Value = fetch_json(&url).await?;
    let entries = resp
        .get("entries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "audit-log response missing 'entries'".to_string())?;
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        result.push(
            serde_json::from_value(entry.clone())
                .map_err(|e| format!("audit entry parse error: {}", e))?,
        );
    }
    Ok(result)
}

// ── SSE Connection ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct SseTokenResponse {
    token: String,
}

/// Exchange `x-admin-key` for a short-lived, one-time SSE token.
///
/// EventSource cannot set custom headers, so the server issues a token via this
/// authenticated endpoint and the client connects with `?token=`.
pub async fn fetch_sse_token() -> Result<String, String> {
    let (builder, epoch) = apply_admin_auth(Request::get(&format!("{API_BASE}/events/token")));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }

    let body: SseTokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("Invalid SSE token response: {}", e))?;

    if body.token.trim().is_empty() {
        return Err("empty SSE token".to_string());
    }

    Ok(body.token)
}

/// Open an SSE stream using a one-time token from [`fetch_sse_token`].
pub fn open_sse_with_token(token: &str) -> Result<web_sys::EventSource, String> {
    let url = format!(
        "{API_BASE}/events?token={}",
        urlencoding::encode(token.trim())
    );
    web_sys::EventSource::new(&url).map_err(|e| format!("SSE connect error: {:?}", e))
}

/// Fetch a short-lived token and connect to the SSE metrics stream.
pub async fn connect_sse() -> Result<web_sys::EventSource, String> {
    let token = fetch_sse_token().await?;
    open_sse_with_token(&token)
}

// ── Codex OAuth Device Login ──

use crab_admin_types::oauth::{
    CodexBulkImportResponse, CodexCredentialListResponse, CodexDeviceStartResponse,
    CodexDeviceStatusResponse, CodexImportRequest, CodexImportResponse,
};

pub async fn start_codex_device_login(
    profile_id: &str,
) -> Result<CodexDeviceStartResponse, String> {
    post_json(
        &format!(
            "{}/upstream/profiles/{}/oauth/codex/device/start",
            API_BASE,
            urlencoding::encode(profile_id)
        ),
        &serde_json::json!({}),
    )
    .await
}

pub async fn poll_codex_device_login(
    profile_id: &str,
    session_id: &str,
) -> Result<CodexDeviceStatusResponse, String> {
    fetch_json(&format!(
        "{}/upstream/profiles/{}/oauth/codex/device/{}",
        API_BASE,
        urlencoding::encode(profile_id),
        urlencoding::encode(session_id)
    ))
    .await
}

pub async fn cancel_codex_device_login(
    profile_id: &str,
    session_id: &str,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/upstream/profiles/{}/oauth/codex/device/{}",
        API_BASE,
        urlencoding::encode(profile_id),
        urlencoding::encode(session_id)
    );
    let (builder, epoch) = apply_admin_auth(Request::delete(&url));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }
    resp.json()
        .await
        .map_err(|e| format!("Invalid cancel response: {e}"))
}

pub async fn list_codex_credentials(
    profile_id: &str,
) -> Result<CodexCredentialListResponse, String> {
    fetch_json(&format!(
        "{}/upstream/profiles/{}/oauth/codex/credentials",
        API_BASE,
        urlencoding::encode(profile_id)
    ))
    .await
}

pub async fn import_codex_credential(
    profile_id: &str,
    credential_id: &str,
) -> Result<CodexImportResponse, String> {
    post_json(
        &format!(
            "{}/upstream/profiles/{}/oauth/codex/import",
            API_BASE,
            urlencoding::encode(profile_id)
        ),
        &CodexImportRequest {
            credential_id: credential_id.to_string(),
        },
    )
    .await
}

pub async fn import_codex_bulk_json(
    profile_id: &str,
    body: &serde_json::Value,
) -> Result<CodexBulkImportResponse, String> {
    post_json(
        &format!(
            "{}/upstream/profiles/{}/oauth/codex/import/bulk",
            API_BASE,
            urlencoding::encode(profile_id)
        ),
        body,
    )
    .await
}

// ── PKCE OAuth API ───────────────────────────────────────────────────────────

pub async fn start_codex_pkce_login(profile_id: &str) -> Result<CodexPkceStartResponse, String> {
    post_json(
        &format!(
            "{}/upstream/profiles/{}/oauth/codex/pkce/start",
            API_BASE,
            urlencoding::encode(profile_id)
        ),
        &serde_json::json!({}),
    )
    .await
}

pub async fn poll_codex_pkce_login(
    profile_id: &str,
    session_id: &str,
) -> Result<CodexPkceExchangeResponse, String> {
    fetch_json(&format!(
        "{}/upstream/profiles/{}/oauth/codex/pkce/{}",
        API_BASE,
        urlencoding::encode(profile_id),
        urlencoding::encode(session_id)
    ))
    .await
}

pub async fn exchange_codex_pkce(
    profile_id: &str,
    session_id: &str,
    callback_url: &str,
) -> Result<CodexPkceExchangeResponse, String> {
    post_json(
        &format!(
            "{}/upstream/profiles/{}/oauth/codex/pkce/{}/exchange",
            API_BASE,
            urlencoding::encode(profile_id),
            urlencoding::encode(session_id)
        ),
        &CodexPkceExchangeRequest {
            callback_url: callback_url.to_string(),
        },
    )
    .await
}

pub async fn cancel_codex_pkce_login(
    profile_id: &str,
    session_id: &str,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/upstream/profiles/{}/oauth/codex/pkce/{}",
        API_BASE,
        urlencoding::encode(profile_id),
        urlencoding::encode(session_id)
    );
    let (builder, epoch) = apply_admin_auth(Request::delete(&url));
    let resp = builder
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    if !resp.ok() {
        return Err(http_error(resp, epoch).await);
    }
    resp.json()
        .await
        .map_err(|e| format!("Invalid cancel response: {e}"))
}
