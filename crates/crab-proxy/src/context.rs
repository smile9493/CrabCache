use crate::TraceLogger;
use crate::client_key_limiter::{ClientKeyGuard, ClientKeyLimiter};
use crate::client_key_rate_limiter::ClientKeyRateLimiter;
use crate::raw_capture::RawCaptureLogger;
use crate::runtime::RuntimeConfig;
use crate::semantic_runtime::SharedSemanticRuntime;
use crate::upstream_pool::UpstreamKeyGuard;
use crate::upstream_user_id_limiter::{UpstreamUserIdGuard, UpstreamUserIdLimiter};
use bytes::Bytes;
use crab_cache::{CacheEntry, CoalesceGuard, RequestCoalescer, TieredCache};
use crab_client_endpoint::ClientEndpointSnapshot;
use crab_composition::RequestComposition;
use crab_metrics::CacheTier;
use crab_pipeline::{PipelineSelectionReason, RequestPipeline};
use crab_reasoning::{
    CursorReasoningDisplayAdapter, PreparedRequest, ReasoningBackend, StreamAccumulator,
};
use crab_semantic::SemanticCache;
use pingora_core::connectors::http::Connector;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
    /// Force HTTP/1.1 ALPN to upstream. Default `false` negotiates HTTP/2 (MiMo passthrough uses H2 DATA frames).
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
    false
}

fn default_upstream_force_http1() -> bool {
    false
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
            idle_timeout_secs: Some(120),
            h2_ping_interval_secs: Some(30),
            upstream_request_timeout_secs: default_upstream_request_timeout_secs(),
            upstream_force_http1: false,
            upstream_write_timeout_secs: default_upstream_write_timeout_secs(),
            upstream_connection_timeout_secs: default_upstream_connection_timeout_secs(),
            upstream_disable_keepalive: false,
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
    /// `sqlite` (default), `redis`, or `pg` (PostgreSQL).
    #[serde(default = "default_reasoning_backend")]
    pub backend: String,
    pub cache_db_path: String,
    #[serde(default)]
    pub redis_url: Option<String>,
    /// PostgreSQL URL for `backend = "pg"`.
    #[serde(default)]
    pub pg_url: Option<String>,
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
            pg_url: None,
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

/// Per-request lifecycle watermarks for phase latency histograms.
#[derive(Default)]
pub struct RequestTimeline {
    pub body_read_start: Option<Instant>,
    pub body_read_done: Option<Instant>,
    pub json_parse_done: Option<Instant>,
    pub pipeline_select_done: Option<Instant>,
    pub cache_lookup_done: Option<Instant>,
    pub upstream_connect_done: Option<Instant>,
    pub upstream_headers_sent: Option<Instant>,
    pub upstream_body_sent: Option<Instant>,
    /// Upstream response headers received (`response_filter` on 2xx).
    pub upstream_response_headers: Option<Instant>,
    /// First upstream response body chunk (SSE TTFT after headers).
    pub ttft: Option<Instant>,
    /// Request start → upstream response headers (MiMo prefill SLO).
    pub prefill_done: Option<Instant>,
    pub upstream_body_done: Option<Instant>,
    pub cache_write_done: Option<Instant>,
    pub logging_done: Option<Instant>,
}

/// Upstream connection, retry, and error state.
pub struct UpstreamState {
    /// HTTP `Host` / TLS SNI for the selected upstream peer.
    pub host: Option<String>,
    /// Ketama affinity key used for backend selection (also on cache-hit paths).
    pub affinity_key: Option<String>,
    /// Backend name for circuit breaker tracking.
    pub backend_name: Option<String>,
    /// TCP/TLS connect completed (`upstream_peer`); not reset at response headers.
    pub start: Option<Instant>,
    /// Upstream response headers received (`response_filter` on 2xx).
    pub headers_at: Option<Instant>,
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
    /// Prepared upstream JSON was written in `request_body_filter` (skip trailing empty H2 EOS).
    pub prepared_upstream_body_emitted: bool,
    /// Whether upstream 4xx/5xx error body was logged to debug NDJSON.
    pub error_body_logged: bool,
    /// Upstream returned 4xx/5xx while client requested SSE — pass JSON error through.
    pub error_passthrough: bool,
    pub sse_rate_limited: bool,
    /// Whether the first upstream body chunk was logged for debug.
    pub first_body_chunk_logged: bool,
    /// Upstream `Content-Encoding` (stripped from forwarded headers); drives R7 decompress.
    pub response_decompress: crate::upstream_response_decompress::UpstreamDecompressState,
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
            affinity_key: None,
            backend_name: None,
            start: None,
            headers_at: None,
            latency_ms: None,
            key_guard: None,
            miss: false,
            retry_budget: Self::default_retry_budget(),
            http_status: None,
            connection_close: false,
            retry_buffer_truncated: false,
            prepared_upstream_body_emitted: false,
            error_body_logged: false,
            error_passthrough: false,
            sse_rate_limited: false,
            first_body_chunk_logged: false,
            response_decompress:
                crate::upstream_response_decompress::UpstreamDecompressState::default(),
        }
    }
}

/// Streaming response processing state (SSE rewriting, reasoning accumulation).
#[derive(Default)]
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
    /// Pipeline-specific SSE processing handler (created once per streaming request).
    pub(crate) stream_pipeline: Option<crate::sse_pipeline::StreamPipeline>,
}

/// Early-connect passthrough: overlap upstream TCP/TLS while the client body uploads.
///
/// Armed prefix from `request_filter`; tail chunks relay incrementally in `request_body_filter`.
#[derive(Default)]
pub struct RequestPassthroughState {
    pub active: bool,
    /// Prefix sniffed in `request_filter` (drained on first upstream emit).
    pub buffer: Vec<u8>,
    pub finalized: bool,
    /// Prefix length at arm time (trace only).
    pub armed_prefix_len: usize,
    /// First upstream body chunk (armed prefix) has been forwarded.
    pub prefix_emitted: bool,
    /// Client body tail chunks captured via `Bytes::clone()` for Raw Capture (zero-copy).
    pub captured_client_chunks: Vec<Bytes>,
    /// Upstream response body chunks captured via `Bytes::clone()` for Raw Capture (zero-copy).
    pub captured_upstream_chunks: Vec<Bytes>,
    /// Incremental SHA-256 over the full client body (trace / req_hash without buffering).
    pub(crate) body_hasher: Option<sha2::Sha256>,
    /// Inbound `Content-Length` when present (passthrough content-length tracking).
    pub inbound_content_length: Option<usize>,
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
    pub original_request_body: Option<Bytes>,
    /// Parsed client JSON payload reused across pipeline/composition/raw-capture to avoid re-parse.
    pub parsed_request_payload: Option<Arc<serde_json::Value>>,
    pub prepared_request: Option<PreparedRequest>,
    /// Copied from `PreparedRequest` for trace/capture after streaming moves `prepared_request` into SSE pipeline.
    pub retired_prefix_messages: Option<usize>,
    pub new_request_body: Option<Bytes>,
    /// Snapshot of the upstream JSON body for raw capture (survives `new_request_body.take()`).
    pub upstream_body_for_capture: Option<Bytes>,
    /// Parsed upstream JSON payload reused by raw-capture.
    pub parsed_upstream_payload: Option<Arc<serde_json::Value>>,
    pub authorization: Option<String>,
    /// SHA-256 prefix of client Bearer token (for capture: same key grouping).
    pub client_key_fingerprint: Option<String>,
    pub req_hash: Option<String>,
    pub content_length: usize,
    pub conversation_id: Option<String>,
    /// Resolved tenant id for upstream `user_id` and cache namespaces.
    pub project_id: Option<String>,
    /// OpenAI-style `prompt_cache_key` from request body (affinity + L3 stickiness).
    pub prompt_cache_key: Option<String>,
    pub request_permit: Option<OwnedSemaphorePermit>,
    pub client_key_guard: Option<ClientKeyGuard>,
    pub deepseek_user_id_guard: Option<UpstreamUserIdGuard>,
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
    /// Per-request cached reasoning config snapshot (avoids repeated RwLock reads).
    pub cached_reasoning_config: ReasoningConfig,
    /// Session fingerprint derived from the first user message (SHA-256 prefix).
    /// Computed once in `request_filter` and shared by trace logger + raw capture.
    pub session_fingerprint: Option<String>,
    /// Stable session source for reasoning/session-store diagnostics.
    pub stable_session_kind: Option<String>,
    pub affinity_prompt_cache_hits: u64,
    /// Accumulated upstream prompt-cache miss tokens (affinity hint finalized in `logging`).
    pub affinity_prompt_cache_misses: u64,
    /// Consecutive usage chunks with pure prompt-cache miss (no hit tokens).
    pub affinity_pure_miss_streak: u32,
    /// Exact tiered cache lookup already attempted (e.g. before full JSON parse).
    pub exact_cache_probed: bool,
    /// Lifecycle watermarks for `gateway_request_phase_latency_seconds`.
    pub timeline: RequestTimeline,
    pub request_passthrough: RequestPassthroughState,
    /// Session store merge outcome: `hit` | `miss` | `break`.
    pub session_store_outcome: Option<String>,
    pub session_store_redis_key: Option<String>,
    /// Canonical messages to persist after successful upstream response.
    pub session_persist_base: Option<Vec<serde_json::Value>>,
    pub session_upstream_messages_len: Option<usize>,
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
            parsed_request_payload: None,
            prepared_request: None,
            retired_prefix_messages: None,
            new_request_body: None,
            upstream_body_for_capture: None,
            parsed_upstream_payload: None,
            authorization: None,
            client_key_fingerprint: None,
            req_hash: None,
            content_length: 0,
            conversation_id: None,
            project_id: None,
            prompt_cache_key: None,
            request_permit: None,
            client_key_guard: None,
            deepseek_user_id_guard: None,
            upstream_outbound_body_len: 0,
            upstream_headers_prepared_at: None,
            request_composition: None,
            tokens: TokenStats::default(),
            upstream: UpstreamState::default(),
            stream: StreamState::default(),
            response_body_preview: Vec::new(),
            cached_reasoning_config: ReasoningConfig::default(),
            session_fingerprint: None,
            stable_session_kind: None,
            affinity_prompt_cache_hits: 0,
            affinity_prompt_cache_misses: 0,
            affinity_pure_miss_streak: 0,
            exact_cache_probed: false,
            timeline: RequestTimeline::default(),
            request_passthrough: RequestPassthroughState::default(),
            session_store_outcome: None,
            session_store_redis_key: None,
            session_persist_base: None,
            session_upstream_messages_len: None,
        }
    }
}

/// Experimental feature flags — each gate is independent and default off.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct FeaturesConfig {
    /// Enable prefix-aware L0 cache key (Moka prefix trie for shared message prefixes).
    #[serde(default)]
    pub prefix_aware_cache: bool,
    /// Enable zero-buffer streaming body forwarding in `request_body_filter`.
    #[serde(default)]
    pub streaming_body_forward: bool,
    /// Pre-warm upstream connections on new session fingerprints.
    #[serde(default)]
    pub connection_prewarm: bool,
    /// Prefer upstream backends that recently returned prompt_cache_hit_tokens > 0 for the same affinity key.
    #[serde(default)]
    pub affinity_prompt_cache_feedback: bool,
    /// Experimental: delta (differential) response cache for long sessions.
    #[serde(default)]
    pub delta_cache: bool,
    /// Reserved: io_uring network backend (requires Pingora support).
    #[serde(default)]
    pub io_uring_backend: bool,
    /// Reserved: WASM filter plugins for body/SSE transforms.
    #[serde(default)]
    pub wasm_filters: bool,
    /// Enable MiMo context compression (auto-summarize old messages).
    #[serde(default)]
    pub mimo_context_compression: bool,
    /// MiMo context compression: message count threshold to trigger compression.
    #[serde(default = "default_compression_threshold")]
    pub mimo_compression_threshold: usize,
    /// Gzip upstream request bodies when size >= `upstream_request_gzip_min_bytes`.
    #[serde(default)]
    pub upstream_request_gzip: bool,
    /// Minimum raw JSON body size before `upstream_request_gzip` applies.
    #[serde(default = "default_upstream_request_gzip_min_bytes")]
    pub upstream_request_gzip_min_bytes: usize,
    /// Retire old MiMo `messages` turns before upstream (does not change cache keys).
    #[serde(default)]
    pub mimo_retire_prefix_messages: bool,
    /// User/assistant turn pairs to keep when `mimo_retire_prefix_messages` is on.
    #[serde(default = "default_mimo_keep_recent_turns")]
    pub mimo_keep_recent_turns: usize,
    /// Redis-backed canonical MiMo messages per stable session (upstream only).
    #[serde(default)]
    pub mimo_session_store: bool,
    #[serde(default = "default_mimo_session_store_ttl_secs")]
    pub mimo_session_store_ttl_secs: u64,
    #[serde(default = "default_mimo_session_store_max_messages")]
    pub mimo_session_store_max_messages: usize,
}

fn default_mimo_keep_recent_turns() -> usize {
    6
}

fn default_mimo_session_store_ttl_secs() -> u64 {
    86_400
}

fn default_mimo_session_store_max_messages() -> usize {
    200
}

fn default_upstream_request_gzip_min_bytes() -> usize {
    4096
}

fn default_compression_threshold() -> usize {
    40
}

pub struct GatewayState {
    pub runtime: Arc<RuntimeConfig>,
    pub tiered_cache: Arc<TieredCache>,
    pub semantic_cache: Option<Arc<SemanticCache>>,
    pub semantic_runtime: SharedSemanticRuntime,
    pub coalescer: Arc<RequestCoalescer>,
    pub reasoning_store: Arc<ReasoningBackend>,
    pub reasoning_config: Arc<parking_lot::RwLock<ReasoningConfig>>,
    pub cors_enabled: bool,
    pub trace_logger: Option<Arc<TraceLogger>>,
    pub raw_capture_logger: Option<Arc<RawCaptureLogger>>,
    pub cache_key_namespace: Option<String>,
    pub pricing: PricingConfig,
    /// Max raw SSE bytes stored per stream cache entry (`0` = never store `sse_body`).
    pub max_sse_cache_bytes: usize,
    pub max_request_body_bytes: usize,
    pub request_semaphore: Arc<Semaphore>,
    pub client_key_limiter: Arc<ClientKeyLimiter>,
    pub client_key_rate_limiter: Arc<ClientKeyRateLimiter>,
    pub deepseek_user_id_limiter: Arc<UpstreamUserIdLimiter>,
    pub features: FeaturesConfig,
    /// Tracks session fingerprints that have already been seen (for connection pre-warm).
    /// Bounded to 10K entries with LRU eviction and 1-hour TTL.
    pub seen_session_fingerprints: moka::sync::Cache<String, ()>,
    /// Shared Pingora upstream connector (TCP/TLS connection pool).
    /// Injected from `HttpProxy::connector_arc()` at startup; used for direct pool pre-warm.
    pub upstream_connector: parking_lot::RwLock<Option<Arc<Connector<()>>>>,
    /// affinity_key → backend_name when upstream prompt cache hits were observed (L3 stickiness).
    pub affinity_backend_hints: moka::sync::Cache<String, String>,
    /// Limits concurrent direct pool pre-warm requests.
    pub prewarm_semaphore: Arc<Semaphore>,
    /// Global RPS estimator using pingora-limits::Rate (1-second double-buffered Count-Min Sketch).
    pub global_rate: Arc<pingora_limits::rate::Rate>,
    /// Client Base URL discovery (FRP / OpenResty / observed request headers).
    pub client_endpoint: Arc<parking_lot::RwLock<ClientEndpointSnapshot>>,
    /// MiMo transparent session store (Redis `crab:session:*`).
    pub session_store: Option<Arc<crate::session_store::SessionStore>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_context_new_defaults() {
        let ctx = GatewayContext::new("test-id".to_string());
        assert_eq!(ctx.request_id, "test-id");
        assert!(!ctx.is_streaming);
        assert!(ctx.cache_key.is_none());
        assert!(ctx.cache_hit.is_none());
        assert!(ctx.cache_tier.is_none());
        assert!(ctx.original_request_body.is_none());
        assert!(ctx.prepared_request.is_none());
    }
}
