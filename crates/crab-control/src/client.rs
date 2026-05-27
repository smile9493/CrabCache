use crate::UpstreamTestResult;
use crate::error::ControlError;
use crate::types::*;
use reqwest::Client;

#[derive(Clone)]
pub struct GatewayAdminClient {
    base_url: String,
    admin_key: String,
    http: Client,
}

impl GatewayAdminClient {
    pub fn new(base_url: impl Into<String>, admin_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            admin_key: admin_key.into(),
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("CRABCACHE_GATEWAY_CONTROL_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:9080".to_string());
        let admin_key = std::env::var("CRABCACHE_GATEWAY_ADMIN_KEY")
            .unwrap_or_else(|_| "change-me-in-production".to_string());
        Self::new(base_url, admin_key)
    }

    fn authed(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base_url, path))
            .header(GATEWAY_ADMIN_KEY_HEADER, &self.admin_key)
    }

    async fn check(resp: reqwest::Response) -> Result<reqwest::Response, ControlError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        Err(ControlError::Http {
            status: status.as_u16(),
            body,
        })
    }

    pub async fn health(&self) -> Result<(), ControlError> {
        let resp = self
            .http
            .get(format!("{}/v1/health", self.base_url))
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ControlError::Http {
                status: resp.status().as_u16(),
                body: resp.text().await.unwrap_or_default(),
            })
        }
    }

    /// Readiness probe: checks Redis via the gateway management API.
    pub async fn ready(&self) -> Result<(), ControlError> {
        let resp = self
            .http
            .get(format!("{}/v1/ready", self.base_url))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(ControlError::Http { status, body })
    }

    /// Readiness probe with detailed subsystem health (Redis, L2).
    pub async fn ready_detail(&self) -> Result<GatewayReadyDetail, ControlError> {
        let resp = self
            .http
            .get(format!("{}/v1/ready", self.base_url))
            .send()
            .await?;
        if resp.status().is_success() || resp.status().as_u16() == 503 {
            return resp.json().await.map_err(ControlError::from);
        }
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Err(ControlError::Http { status, body })
    }

    pub async fn status(&self) -> Result<GatewayStatus, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/status")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn list_keys(&self) -> Result<Vec<ApiKeySpec>, ControlError> {
        let resp = self.authed(reqwest::Method::GET, "/v1/keys").send().await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn create_key(
        &self,
        req: &CreateGatewayKeyRequest,
    ) -> Result<CreateGatewayKeyResponse, ControlError> {
        let resp = self
            .authed(reqwest::Method::POST, "/v1/keys")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn revoke_key(&self, token: &str) -> Result<(), ControlError> {
        let path = format!("/v1/keys/{}", urlencoding::encode(token));
        let resp = self.authed(reqwest::Method::DELETE, &path).send().await?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn patch_key(
        &self,
        token: &str,
        req: &PatchGatewayKeyRequest,
    ) -> Result<ApiKeySpec, ControlError> {
        let path = format!("/v1/keys/{}", urlencoding::encode(token));
        let resp = self
            .authed(reqwest::Method::PATCH, &path)
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    /// Patch by gateway `StoredKey.id` (Dashboard / Admin BFF use this, not the sk-cc-* token).
    pub async fn patch_key_by_id(
        &self,
        id: &str,
        req: &PatchGatewayKeyRequest,
    ) -> Result<ApiKeySpec, ControlError> {
        let path = format!("/v1/keys/by-id/{}", urlencoding::encode(id));
        let resp = self
            .authed(reqwest::Method::PATCH, &path)
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn revoke_key_by_id(&self, id: &str) -> Result<(), ControlError> {
        let path = format!("/v1/keys/by-id/{}", urlencoding::encode(id));
        let resp = self.authed(reqwest::Method::DELETE, &path).send().await?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn get_ttl(&self) -> Result<TtlConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/cache/ttl")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn list_domain_policies(&self) -> Result<Vec<DomainPolicySpec>, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/domains/policies")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_domain_policies(
        &self,
        req: &PutDomainPoliciesRequest,
    ) -> Result<Vec<DomainPolicySpec>, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/domains/policies")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn delete_domain_policy(&self, domain: &str) -> Result<(), ControlError> {
        let path = format!("/v1/domains/policies/{domain}");
        let resp = self.authed(reqwest::Method::DELETE, &path).send().await?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn get_domain_usage(&self) -> Result<DomainUsageResponse, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/domains/usage")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_domain_usage(
        &self,
        req: &PutDomainUsageRequest,
    ) -> Result<(), ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/domains/usage")
            .json(req)
            .send()
            .await?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn put_ttl(&self, req: &PutTtlConfigRequest) -> Result<TtlConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/cache/ttl")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_stream_cache(&self) -> Result<StreamCacheConfig, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/runtime/stream_cache")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_stream_cache(
        &self,
        req: &StreamCacheConfig,
    ) -> Result<StreamCacheConfig, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/runtime/stream_cache")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_invalidate_status(&self) -> Result<InvalidateCacheStatus, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/cache/invalidate/status")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn invalidate_cache(
        &self,
        req: &InvalidateCacheRequest,
    ) -> Result<InvalidateCacheResponse, ControlError> {
        let mut builder = self
            .authed(reqwest::Method::POST, "/v1/cache/invalidate")
            .json(req);
        if req.scope.trim() == "all" {
            builder = builder.header(
                CACHE_INVALIDATE_CONFIRM_HEADER,
                CACHE_INVALIDATE_CONFIRM_ALL,
            );
        }
        let resp = builder.send().await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_fingerprint(&self) -> Result<FingerprintConfigRequest, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/cache/fingerprint")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_fingerprint(
        &self,
        req: &FingerprintConfigRequest,
    ) -> Result<FingerprintConfigRequest, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/cache/fingerprint")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_backends(&self) -> Result<RoutingBackendsView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/routing/backends")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_backends(
        &self,
        req: &PutBackendsRequest,
    ) -> Result<RoutingBackendsView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/routing/backends")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_upstream_keys(&self) -> Result<UpstreamKeysView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/upstream/keys")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_upstream_keys(
        &self,
        req: &PutUpstreamKeysRequest,
    ) -> Result<UpstreamKeysView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/upstream/keys")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn patch_upstream_key(
        &self,
        id: &str,
        req: &PatchUpstreamKeyRequest,
    ) -> Result<UpstreamKeyView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PATCH, &format!("/v1/upstream/keys/{id}"))
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_upstream_relay(&self) -> Result<UpstreamRelayConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/upstream/relay")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_upstream_relay(
        &self,
        req: &PutUpstreamRelayConfigRequest,
    ) -> Result<UpstreamRelayConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/upstream/relay")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_reasoning_runtime(&self) -> Result<ReasoningRuntimeConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/runtime/reasoning")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_reasoning_runtime(
        &self,
        req: &ReasoningRuntimeConfigView,
    ) -> Result<ReasoningRuntimeConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/runtime/reasoning")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn clear_reasoning_cache(&self) -> Result<ClearReasoningCacheResponse, ControlError> {
        let resp = self
            .authed(reqwest::Method::DELETE, "/v1/reasoning/cache")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_cursor_models(&self) -> Result<CursorModelsConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/cursor/models")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_cursor_models(
        &self,
        req: &CursorModelsConfigView,
    ) -> Result<CursorModelsConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/cursor/models")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_pipeline_runtime(&self) -> Result<PipelineRuntimeConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/runtime/pipeline")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_pipeline_runtime(
        &self,
        req: &PipelineRuntimeConfigView,
    ) -> Result<PipelineRuntimeConfigView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/v1/runtime/pipeline")
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn list_upstream_profiles(&self) -> Result<UpstreamProfilesResponse, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/upstream/profiles")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_upstream_profile(
        &self,
        id: &str,
        req: &PutUpstreamProfileRequest,
    ) -> Result<UpstreamProfileView, ControlError> {
        let resp = self
            .authed(reqwest::Method::PUT, &format!("/v1/upstream/profiles/{id}"))
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn delete_upstream_profile(&self, id: &str) -> Result<(), ControlError> {
        let resp = self
            .authed(
                reqwest::Method::DELETE,
                &format!("/v1/upstream/profiles/{id}"),
            )
            .send()
            .await?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn get_upstream_profile_keys(
        &self,
        id: &str,
    ) -> Result<UpstreamProfileKeysView, ControlError> {
        let resp = self
            .authed(
                reqwest::Method::GET,
                &format!("/v1/upstream/profiles/{id}/keys"),
            )
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn put_upstream_profile_keys(
        &self,
        id: &str,
        req: &PutUpstreamProfileKeysRequest,
    ) -> Result<UpstreamProfileKeysView, ControlError> {
        let resp = self
            .authed(
                reqwest::Method::PUT,
                &format!("/v1/upstream/profiles/{id}/keys"),
            )
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn patch_upstream_profile_key(
        &self,
        profile_id: &str,
        key_id: &str,
        req: &PatchUpstreamKeyRequest,
    ) -> Result<UpstreamKeyView, ControlError> {
        let resp = self
            .authed(
                reqwest::Method::PATCH,
                &format!("/v1/upstream/profiles/{profile_id}/keys/{key_id}"),
            )
            .json(req)
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn test_upstream_profile(
        &self,
        id: &str,
    ) -> Result<UpstreamTestResult, ControlError> {
        let resp = self
            .authed(
                reqwest::Method::POST,
                &format!("/v1/upstream/profiles/{id}/test"),
            )
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn test_upstream_profile_key(
        &self,
        profile_id: &str,
        key_id: &str,
    ) -> Result<UpstreamTestResult, ControlError> {
        let resp = self
            .authed(
                reqwest::Method::POST,
                &format!("/v1/upstream/profiles/{profile_id}/keys/{key_id}/test"),
            )
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_profile_routing(
        &self,
        profile_id: &str,
    ) -> Result<ProfileRoutingView, ControlError> {
        let resp = self
            .authed(
                reqwest::Method::GET,
                &format!("/v1/upstream/profiles/{profile_id}/routing"),
            )
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }

    pub async fn get_routing_summary(&self) -> Result<RoutingSummaryView, ControlError> {
        let resp = self
            .authed(reqwest::Method::GET, "/v1/routing/summary")
            .send()
            .await?;
        let resp = Self::check(resp).await?;
        resp.json().await.map_err(ControlError::from)
    }
}
