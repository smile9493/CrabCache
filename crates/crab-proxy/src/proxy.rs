use crate::context::{ConnectionConfig, GatewayContext, GatewayState};
use crate::sse::{parse_sse_chunk, UsageData};
use crab_cache::{CacheEntry, UsageInfo};
use crab_metrics::global_metrics;
use crab_route::extract_affinity_key;
use http::HeaderMap;
use pingora_core::prelude::*;
use pingora_core::protocols::l4::ext::TcpKeepalive;
use pingora_core::upstreams::peer::PeerOptions;
use pingora_http::{RequestHeader, ResponseHeader};
use pingora_proxy::{ProxyHttp, Session};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

pub struct GatewayProxy {
    state: Arc<GatewayState>,
}

impl GatewayProxy {
    pub fn new(state: Arc<GatewayState>) -> Self {
        Self { state }
    }
}

#[async_trait::async_trait]
impl ProxyHttp for GatewayProxy {
    type CTX = GatewayContext;

    fn new_ctx(&self) -> Self::CTX {
        GatewayContext::new(uuid::Uuid::new_v4().to_string())
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let req_header = session.req_header();

        if req_header.uri.path() == "/health" {
            let _ = session.respond_error(200).await;
            return Ok(true);
        }

        if is_models_endpoint(req_header.uri.path(), &req_header.method) {
            ctx.is_models_list = true;
            return Ok(false);
        }

        if req_header.uri.path() != "/v1/chat/completions" {
            let _ = session.respond_error(404).await;
            return Ok(true);
        }

        if req_header.method != http::Method::POST {
            let _ = session.respond_error(405).await;
            return Ok(true);
        }

        let auth = req_header
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !auth.ends_with(&self.state.api_key) {
            let _ = session.respond_error(401).await;
            return Ok(true);
        }

        ctx.consumer = req_header
            .headers
            .get("x-consumer")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        if ctx.is_models_list {
            let backend = self
                .state
                .router
                .backends()
                .first()
                .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

            let mut peer = HttpPeer::new(backend.addr, true, backend.tls_sni.clone());
            apply_connection_options(&self.state.conn_config, &mut peer.options);
            return Ok(Box::new(peer));
        }

        let req_header = session.req_header();

        let client_ip = session
            .client_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();

        let headers = HeaderMap::from_iter(
            req_header
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );

        let affinity_key = extract_affinity_key(&headers, &client_ip);

        let backend = self
            .state
            .router
            .select(affinity_key.as_bytes())
            .ok_or_else(|| Error::new(ErrorType::ConnectProxyFailure))?;

        debug!(
            request_id = %ctx.request_id,
            backend = %backend.name,
            affinity_key = %affinity_key,
            "Selected upstream backend"
        );

        let mut peer = HttpPeer::new(
            backend.addr,
            true,
            backend.tls_sni.clone(),
        );
        apply_connection_options(&self.state.conn_config, &mut peer.options);

        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        upstream_request
            .insert_header("x-request-id", ctx.request_id.clone())
            .unwrap();

        Ok(())
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if ctx.is_models_list {
            return Ok(());
        }

        let status = upstream_response.status.as_u16();
        if status >= 400 {
            return Ok(());
        }

        upstream_response
            .insert_header("x-request-id", ctx.request_id.clone())
            .unwrap();

        upstream_response
            .insert_header("x-cache-status", "miss")
            .unwrap();

        Ok(())
    }

    fn upstream_response_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<Option<Duration>> {
        if ctx.is_models_list {
            return Ok(None);
        }

        if let Some(data) = body.as_ref() {
            ctx.accumulated_body.extend_from_slice(data);

            if ctx.is_streaming {
                if ctx.ttft.is_none() {
                    if let Some(upstream_start) = ctx.upstream_start {
                        ctx.ttft = Some(upstream_start.elapsed());
                        global_metrics().record_latency(
                            crab_metrics::LatencyKind::TTFT,
                            ctx.ttft.unwrap(),
                            None,
                        );
                    }
                }

                let events = parse_sse_chunk(data);
                for event in &events {
                    if let Some(usage) = event.parse_usage() {
                        record_usage(&usage, &ctx.model, ctx.consumer.as_deref());
                    }
                }
            }
        }

        if end_of_stream && !ctx.is_streaming {
            if let Some(upstream_start) = ctx.upstream_start {
                let latency = upstream_start.elapsed();
                global_metrics().record_latency(
                    crab_metrics::LatencyKind::Upstream,
                    latency,
                    None,
                );
            }

            if let Ok(body_value) = serde_json::from_slice::<serde_json::Value>(&ctx.accumulated_body) {
                if let Some(usage) = body_value.get("usage") {
                    let usage_data = UsageData {
                        prompt_tokens: usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        completion_tokens: usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        prompt_cache_hit_tokens: usage.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        prompt_cache_miss_tokens: usage.get("prompt_cache_miss_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                    };
                    record_usage(&usage_data, &ctx.model, ctx.consumer.as_deref());
                }

                if let Some(cache_key) = &ctx.cache_key {
                    let entry = CacheEntry {
                        response_body: ctx.accumulated_body.clone(),
                        model: ctx.model.clone(),
                        usage: UsageInfo::default(),
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        ttl_secs: 3600,
                    };

                    let tiered_cache = self.state.tiered_cache.clone();
                    let cache_key = cache_key.clone();
                    let model = ctx.model.clone();
                    let consumer = ctx.consumer.clone();
                    tokio::spawn(async move {
                        if let Err(e) = tiered_cache
                            .put(&cache_key, entry, &model, consumer.as_deref())
                            .await
                        {
                            warn!(error = %e, "Failed to cache response");
                        }
                    });
                }
            }
        }

        Ok(None)
    }
}

fn record_usage(usage: &UsageData, model: &str, consumer: Option<&str>) {
    global_metrics().record_upstream_usage(
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.prompt_cache_hit_tokens,
        usage.prompt_cache_miss_tokens,
        model,
        consumer,
    );
}

fn is_models_endpoint(path: &str, method: &http::Method) -> bool {
    *method == http::Method::GET && (path == "/models" || path == "/v1/models")
}

fn apply_connection_options(config: &ConnectionConfig, options: &mut PeerOptions) {
    if let (Some(idle), Some(interval), Some(count)) = (
        config.tcp_keepalive_idle_secs,
        config.tcp_keepalive_interval_secs,
        config.tcp_keepalive_count,
    ) {
        options.tcp_keepalive = Some(TcpKeepalive {
            idle: Duration::from_secs(idle),
            interval: Duration::from_secs(interval),
            count,
            user_timeout: Duration::from_secs(0),
        });
    }

    if let Some(idle_secs) = config.idle_timeout_secs {
        options.idle_timeout = Some(Duration::from_secs(idle_secs));
    }

    if let Some(ping_secs) = config.h2_ping_interval_secs
        && ping_secs > 0
    {
        options.h2_ping_interval = Some(Duration::from_secs(ping_secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_context_new() {
        let ctx = GatewayContext::new("test-id".to_string());
        assert_eq!(ctx.request_id, "test-id");
        assert!(!ctx.is_streaming);
        assert!(ctx.cache_key.is_none());
        assert!(ctx.cache_hit.is_none());
    }
}
