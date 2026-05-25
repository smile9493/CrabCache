use crate::TraceLogger;
use crate::client_key_limiter::{ClientKeyGuard, ClientKeyLimiter};
use crate::client_key_rate_limiter::ClientKeyRateLimiter;
use crate::runtime::RuntimeConfig;
use crate::upstream_pool::UpstreamKeyGuard;
use crab_cache::{CacheEntry, CoalesceGuard, RequestCoalescer, TieredCache};
use crab_composition::RequestComposition;
use crab_metrics::CacheTier;
use crab_pipeline::{PipelineSelectionReason, RequestPipeline};
use crab_reasoning::{
    CursorReasoningDisplayAdapter, PreparedRequest, ReasoningBackend, StreamAccumulator,
};
use crab_semantic::{SemanticCache, SemanticGateConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConnectionConfig {
    pub tcp_keepalive_idle_secs: Option<u64>,
    pub tcp_keepalive_interval_secs: Option<u64>,
    pub tcp_keepalive_count: Option<usize>,
    pub idle_timeout_secs: Option<u64>,
    pub h2_ping_interval_secs: Option<u64>,
    /// Max seconds waiting for upstream response bytes (0 = no limit).
    #[serde(default = "default_upstream_request_timeout_secs")]
    pub upstream_request_timeout_secs: Option<u64>,
    /// Force HTTP/1.1 ALPN to upstream (recommended for DeepSeek; avoids H2 edge cases).
    #[serde(default = "default_upstream_force_http1")]
    pub upstream_force_http1: bool,
    /// Max seconds per write when sending large request bodies upstream.
    #[serde(default = "default_upstream_write_timeout_secs")]
    pub upstream_write_timeout_secs: Option<u64>,
    /// TCP+TLS connect timeout to upstream.
    #[serde(default = "default_upstream_connection_timeout_secs")]
    pub upstream_connection_timeout_secs: Option<u64>,
    /// Send `Connection: close` and avoid pooling idle upstream sockets (fixes stale H1 reuse).
    #[serde(default = "default_upstream_disable_keepalive")]
    pub upstream_disable_keepalive: bool,
    /// Override the ECDH curves advertised during TLS handshake (OpenSSL group list syntax).
    /// Defaults to Chrome-like order: `"X25519:P-256:P-384"`.
    /// Set to `""` to use the OpenSSL/Pingora defaults.
    #[serde(default = "default_upstream_tls_curves")]
    pub upstream_tls_curves: String,
}

fn default_upstream_disable_keepalive() -> bool {
    true
}

fn default_upstream_force_http1() -> bool {
    true
}

fn default_upstream_write_timeout_secs() -> Option<u64> {
    Some(300)
}

fn default_upstream_connection_timeout_secs() -> Option<u64> {
    Some(60)
}

fn default_upstream_request_timeout_secs() -> Option<u64> {
    Some(300)
}

fn default_upstream_tls_curves() -> String {
    "X25519:P-256:P-384".to_string()
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            tcp_keepalive_idle_secs: Some(60),
            tcp_keepalive_interval_secs: Some(10),
            tcp_keepalive_count: Some(3),
            idle_timeout_secs: Some(90),
            h2_ping_interval_secs: Some(30),
            upstream_request_timeout_secs: default_upstream_request_timeout_secs(),
            upstream_force_http1: true,
            upstream_write_timeout_secs: default_upstream_write_timeout_secs(),
            upstream_connection_timeout_secs: default_upstream_connection_timeout_secs(),
            upstream_disable_keepalive: true,
            upstream_tls_curves: default_upstream_tls_curves(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReasoningConfig {
    pub thinking_mode: String,
    pub reasoning_effort: String,
    pub missing_reasoning_strategy: String,
    pub display_reasoning: bool,
    pub collapsible_reasoning: bool,
    /// `sqlite` (default) or `redis` (required for multi-instance gateway).
    #[serde(default = "default_reasoning_backend")]
    pub backend: String,
    pub cache_db_path: String,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default = "default_max_reasoning_entry_bytes")]
    pub max_reasoning_entry_bytes: usize,
    pub cache_max_age_secs: Option<u64>,
    pub cache_max_rows: Option<usize>,
    /// Append a tail summary user message when message count exceeds this (0 = disabled).
    #[serde(default)]
    pub context_summary_message_threshold: usize,
    /// When true, log/metric non-append-only prefix changes (does not block requests).
    #[serde(default)]
    pub prefix_validate: bool,
}

fn default_reasoning_backend() -> String {
    "sqlite".to_string()
}

fn default_max_reasoning_entry_bytes() -> usize {
    512 * 1024
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            thinking_mode: "enabled".to_string(),
            reasoning_effort: "max".to_string(),
            missing_reasoning_strategy: "recover".to_string(),
            display_reasoning: true,
            collapsible_reasoning: true,
            backend: default_reasoning_backend(),
            cache_db_path: ":memory:".to_string(),
            redis_url: None,
            max_reasoning_entry_bytes: default_max_reasoning_entry_bytes(),
            cache_max_age_secs: Some(30 * 24 * 3600),
            cache_max_rows: Some(100_000),
            context_summary_message_threshold: 0,
            prefix_validate: false,
        }
    }
}

/// Per-model pricing for cost-saved calculation.
/// Prices are in USD per million tokens.
#[derive(Debug, Deserialize, Clone)]
pub struct ModelPricing {
    pub input_price_per_million: f64,
    pub output_price_per_million: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PricingConfig {
    pub default_input_price_per_million: f64,
    pub default_output_price_per_million: f64,
    #[serde(default)]
    pub model_overrides: HashMap<String, ModelPricing>,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            // Default DeepSeek v3 pricing (cache miss rates)
            default_input_price_per_million: 0.55,
            default_output_price_per_million: 2.19,
            model_overrides: HashMap::new(),
        }
    }
}

impl PricingConfig {
    /// Calculate cost saved for a cache hit in USD.
    /// Takes the input/output token counts from the cached entry.
    pub fn cost_saved_usd(&self, model: &str, prompt_tokens: u64, completion_tokens: u64) -> f64 {
        let (input_price, output_price) = self
            .model_overrides
            .get(model)
            .map(|p| (p.input_price_per_million, p.output_price_per_million))
            .unwrap_or((
                self.default_input_price_per_million,
                self.default_output_price_per_million,
            ));

        let input_cost = (prompt_tokens as f64) / 1_000_000.0 * input_price;
        let output_cost = (completion_tokens as f64) / 1_000_000.0 * output_price;
        input_cost + output_cost
    }
}

/// Token usage statistics collected during upstream response processing.
#[derive(Debug, Default)]
pub struct TokenStats {
    pub total: u64,
    pub last_input: u64,
    pub last_output: u64,
    pub last_prompt_cache_hit: u64,
    pub last_prompt_cache_miss: u64,
}

/// Upstream connection, retry, and error state.
pub struct UpstreamState {
    /// HTTP `Host` / TLS SNI for the selected upstream peer.
    pub host: Option<String>,
    /// Backend name for circuit breaker tracking.
    pub backend_name: Option<String>,
    pub start: Option<Instant>,
    /// Upstream body completion latency (response headers → EOS), miss paths only.
    pub latency_ms: Option<f64>,
    pub key_guard: Option<UpstreamKeyGuard>,
    pub miss: bool,
    /// Remaining same-request upstream retries after 429 (non-streaming only).
    pub retry_budget: u8,
    /// Upstream HTTP status from `response_filter` (for error body correlation).
    pub http_status: Option<u16>,
    pub connection_close: bool,
    /// Downstream retry buffer exceeded 64KiB while reading in `request_filter`.
    pub retry_buffer_truncated: bool,
    /// Whether upstream 4xx/5xx error body was logged to debug NDJSON.
    pub error_body_logged: bool,
}

impl UpstreamState {
    fn default_retry_budget() -> u8 {
        1
    }
}

impl Default for UpstreamState {
    fn default() -> Self {
        Self {
            host: None,
            backend_name: None,
            start: None,
            latency_ms: None,
            key_guard: None,
            miss: false,
            retry_budget: Self::default_retry_budget(),
            http_status: None,
            connection_close: false,
            retry_buffer_truncated: false,
            error_body_logged: false,
        }
    }
}

/// Streaming response processing state (SSE rewriting, reasoning accumulation).
pub struct StreamState {
    pub accumulator: Option<StreamAccumulator>,
    pub display_adapter: Option<CursorReasoningDisplayAdapter>,
    /// Set when streaming SSE receives upstream `[DONE]` and reasoning was stored.
    pub reasoning_finalized: bool,
    /// Incomplete SSE line bytes spanning upstream body chunks.
    pub sse_remainder: Vec<u8>,
    /// Client-shaped SSE bytes accumulated for L0/L1 `sse_body` (not upstream raw).
    pub client_sse_body: Vec<u8>,
    pub pending_recovery_notice: Option<String>,
    /// One-shot warn when CursorDeepSeekV4 streams without `prepared_request`.
    pub reasoning_bypass_warned: bool,
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            accumulator: None,
            display_adapter: None,
            reasoning_finalized: false,
            sse_remainder: Vec::new(),
            client_sse_body: Vec::new(),
            pending_recovery_notice: None,
            reasoning_bypass_warned: false,
        }
    }
}

pub struct GatewayContext {
    pub request_id: String,
    pub cache_key: Option<String>,
    pub cache_hit: Option<CacheEntry>,
    pub cache_tier: Option<CacheTier>,
    pub is_streaming: bool,
    pub is_models_list: bool,
    pub model: String,
    pub consumer: Option<String>,
    pub domain: Option<String>,
    pub request_pipeline: Option<RequestPipeline>,
    pub pipeline_reason: Option<PipelineSelectionReason>,
    pub upstream_profile_id: Option<String>,
    /// Model name after pipeline prepare (upstream-bound).
    pub upstream_model: Option<String>,
    pub request_start: Instant,
    pub ttft: Option<std::time::Duration>,
    pub accumulated_body: Vec<u8>,
    pub is_coalesced_follower: bool,
    pub coalesce_guard: Option<CoalesceGuard>,
    pub original_request_body: Option<Vec<u8>>,
    pub prepared_request: Option<PreparedRequest>,
    pub new_request_body: Option<Vec<u8>>,
    pub authorization: Option<String>,
    pub req_hash: Option<String>,
    pub content_length: usize,
    pub conversation_id: Option<String>,
    /// Resolved tenant id for upstream `user_id` and cache namespaces.
    pub project_id: Option<String>,
    /// OpenAI-style `prompt_cache_key` from request body (affinity + L3 stickiness).
    pub prompt_cache_key: Option<String>,
    pub request_permit: Option<OwnedSemaphorePermit>,
    pub client_key_guard: Option<ClientKeyGuard>,
    /// Serialized upstream JSON body length after reasoning prepare (for diagnostics).
    pub upstream_outbound_body_len: usize,
    /// Set in `upstream_request_filter` before Pingora writes upstream headers.
    pub upstream_headers_prepared_at: Option<Instant>,
    /// Request composition fingerprint (extracted in request_filter after pipeline prepare).
    pub request_composition: Option<RequestComposition>,
    /// Token usage statistics.
    pub tokens: TokenStats,
    /// Upstream connection and retry state.
    pub upstream: UpstreamState,
    /// Streaming response processing state.
    pub stream: StreamState,
    /// Accumulated response body for trace logging (non-streaming / streaming).
    /// Only populated when `trace_logging.max_response_preview_bytes > 0`.
    pub response_body_preview: Vec<u8>,
}

impl GatewayContext {
    pub fn new(request_id: String) -> Self {
        Self {
            request_id,
            cache_key: None,
            cache_hit: None,
            cache_tier: None,
            is_streaming: false,
            is_models_list: false,
            model: String::new(),
            consumer: None,
            domain: None,
            request_pipeline: None,
            pipeline_reason: None,
            upstream_profile_id: None,
            upstream_model: None,
            request_start: Instant::now(),
            ttft: None,
            accumulated_body: Vec::new(),
            is_coalesced_follower: false,
            coalesce_guard: None,
            original_request_body: None,
            prepared_request: None,
            new_request_body: None,
            authorization: None,
            req_hash: None,
            content_length: 0,
            conversation_id: None,
            project_id: None,
            prompt_cache_key: None,
            request_permit: None,
            client_key_guard: None,
            upstream_outbound_body_len: 0,
            upstream_headers_prepared_at: None,
            request_composition: None,
            tokens: TokenStats::default(),
            upstream: UpstreamState::default(),
            stream: StreamState::default(),
            response_body_preview: Vec::new(),
        }
    }
}

pub struct GatewayState {
    pub runtime: Arc<RuntimeConfig>,
    pub tiered_cache: Arc<TieredCache>,
    pub semantic_cache: Option<Arc<SemanticCache>>,
    pub semantic_gate: SemanticGateConfig,
    pub coalescer: Arc<RequestCoalescer>,
    pub reasoning_store: Arc<ReasoningBackend>,
    pub reasoning_config: Arc<RwLock<ReasoningConfig>>,
    pub cors_enabled: bool,
    pub trace_logger: Option<Arc<TraceLogger>>,
    pub cache_key_namespace: Option<String>,
    pub pricing: PricingConfig,
    /// Max raw SSE bytes stored per stream cache entry (`0` = never store `sse_body`).
    pub max_sse_cache_bytes: usize,
    pub max_request_body_bytes: usize,
    pub request_semaphore: Arc<Semaphore>,
    pub client_key_limiter: Arc<ClientKeyLimiter>,
    pub client_key_rate_limiter: Arc<ClientKeyRateLimiter>,
}
