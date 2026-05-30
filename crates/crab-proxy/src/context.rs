use crate::TraceLogger;
use crate::backend_state::{BackendLoadRegistry, BackendPermit};
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

/// Client-facing OpenAI wire protocol (Chat Completions vs Responses API).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientWireApi {
    #[default]
    ChatCompletions,
    Responses,
}

impl ClientWireApi {
    pub fn from_request_path(path: &str) -> Self {
        if is_client_responses_path(path) {
            Self::Responses
        } else {
            Self::ChatCompletions
        }
    }

    /// Stable label stored in raw capture / trace (`responses` | `chat_completions`).
    pub fn as_wire_api_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
        }
    }
}

/// True for `POST /v1/responses` (Codex CLI and OpenAI Responses clients).
pub fn is_client_responses_path(path: &str) -> bool {
    path == "/v1/responses" || path.ends_with("/v1/responses")
}

/// Paths that accept LLM POST bodies (`model` in JSON).
pub fn is_llm_completion_path(path: &str) -> bool {
    path == "/v1/chat/completions"
        || path == "/chat/completions"
        || is_client_responses_path(path)
}
use crab_reasoning::{
    CursorReasoningDisplayAdapter, PreparedRequest, ReasoningBackend, StreamAccumulator,
};
use crab_semantic::SemanticCache;
use pingora_core::connectors::http::Connector;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendRouteStrategy {
    Ketama,
    P2c,
    LeastUsed,
    CostOptimized,
    /// Multi-factor weighted Ketama: score all ready backends on health, latency,
    /// load, affinity-hit-rate, and 429-rate; pick the best.
    WeightedKetama,
}

impl Default for BackendRouteStrategy {
    fn default() -> Self {
        Self::Ketama
    }
}

impl BackendRouteStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "p2c" => Self::P2c,
            "least_used" | "least-used" => Self::LeastUsed,
            "cost_optimized" | "cost-optimized" | "eco" => Self::CostOptimized,
            "weighted_ketama" | "weighted-ketama" | "weighted" => Self::WeightedKetama,
            _ => Self::Ketama,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ketama => "ketama",
            Self::P2c => "p2c",
            Self::LeastUsed => "least_used",
            Self::CostOptimized => "cost_optimized",
            Self::WeightedKetama => "weighted_ketama",
        }
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
    /// Runtime overload classification for the selected backend.
    pub backend_overload_state: Option<String>,
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
    /// Saved prepared upstream body for 429 retry — restored in `response_filter` before
    /// returning a retryable error so that `request_body_filter` can resend the full body.
    pub prepared_body_for_retry: Option<bytes::Bytes>,
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
            backend_overload_state: None,
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
            prepared_body_for_retry: None,
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
    /// Chat Completions → Responses API SSE translator for non-Codex `/v1/responses` clients.
    pub(crate) responses_translator: Option<crate::responses_wire::ChatToResponsesSseTranslator>,
    /// `response.created` + `response.in_progress` bytes queued at upstream headers (prefill keepalive).
    pub(crate) responses_wire_bootstrap: Option<Vec<u8>>,
    /// Bootstrap already merged into the first downstream body chunk.
    pub(crate) responses_wire_bootstrap_sent: bool,
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
    /// Extra bytes added to the outbound prefix (e.g. `stream_options` injection).
    pub outbound_extra_bytes: usize,
}

pub struct GatewayContext {
    pub request_id: String,
    pub cache_key: Option<String>,
    pub cache_hit: Option<CacheEntry>,
    pub cache_tier: Option<CacheTier>,
    pub is_streaming: bool,
    pub is_models_list: bool,
    /// Downstream wire API inferred from request path.
    pub client_wire_api: ClientWireApi,
    pub model: String,
    pub consumer: Option<String>,
    pub domain: Option<String>,
    /// Resolved downstream client IP (X-Forwarded-For / X-Real-IP / peer).
    pub client_ip: Option<String>,
    /// Direct TCP peer seen by Pingora (often the reverse proxy hop).
    pub client_peer_addr: Option<String>,
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
    pub backend_permit: Option<BackendPermit>,
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
    pub guardrail_hits: Vec<String>,
    pub guardrail_blocked: bool,
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
            client_wire_api: ClientWireApi::ChatCompletions,
            model: String::new(),
            consumer: None,
            domain: None,
            client_ip: None,
            client_peer_addr: None,
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
            backend_permit: None,
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
            guardrail_hits: Vec::new(),
            guardrail_blocked: false,
        }
    }
}

// `Instant` is Send but not Sync; Pingora's `HttpServerApp` requires `CTX: Send + Sync`.
// Each `GatewayContext` is owned by one worker for the request lifetime.
unsafe impl Sync for GatewayContext {}

/// Experimental feature flags — each gate is independent and default off.
#[derive(Debug, Deserialize, Clone)]
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
    /// Persist Responses `previous_response_id` chains to Redis (Moka L0 + Redis L1).
    #[serde(default = "default_responses_chain_redis")]
    pub responses_chain_redis: bool,
    #[serde(default = "default_responses_chain_ttl_secs")]
    pub responses_chain_ttl_secs: u64,
    #[serde(default = "default_responses_chain_max_capacity")]
    pub responses_chain_max_capacity: u64,
    #[serde(default = "default_responses_chain_max_value_bytes")]
    pub responses_chain_max_value_bytes: usize,
    #[serde(default = "default_responses_chain_max_output_items")]
    pub responses_chain_max_output_items: usize,
    /// Minimum bytes of request body prefix to trigger MiMo passthrough (overlap connect + upload).
    #[serde(default = "default_passthrough_prefix_bytes")]
    pub passthrough_prefix_bytes: usize,
    /// Enable per-backend runtime load checks when selecting upstream peers.
    #[serde(default)]
    pub backend_load_aware_routing_enabled: bool,
    /// Route policy used when choosing among healthy upstream backends.
    #[serde(default)]
    pub backend_route_strategy: BackendRouteStrategy,
    /// Enable per-backend in-flight concurrency limits.
    #[serde(default)]
    pub backend_concurrency_limit_enabled: bool,
    /// Default max in-flight requests per backend when backend concurrency limiting is enabled.
    #[serde(default = "default_max_inflight_per_backend")]
    pub default_max_inflight_per_backend: usize,
    /// Mark a backend overloaded when observed prefill exceeds this threshold (0 disables).
    #[serde(default = "default_backend_prefill_overload_threshold_ms")]
    pub backend_prefill_overload_threshold_ms: u64,
    /// Cooldown window after a backend crosses the prefill threshold.
    #[serde(default = "default_backend_overload_cooldown_ms")]
    pub backend_overload_cooldown_ms: u64,
    /// Reserve switch for future pipeline overload gates.
    #[serde(default)]
    pub pipeline_overload_degrade_enabled: bool,
    /// Reserve switch for future MiMo overload degradation.
    #[serde(default)]
    pub mimo_overload_degrade_to_generic: bool,
    /// Enable conversation-level upstream key binding for MiMo pipeline.
    /// Once a conversation binds to a key, all subsequent requests use that key
    /// (no rotation) until the binding expires after `mimo_key_binding_ttl_secs`
    /// of idle time, or the key's concurrency exceeds `mimo_key_max_inflight`.
    #[serde(default)]
    pub mimo_key_binding: bool,
    /// Idle TTL (seconds) for MiMo key bindings. A binding is released after
    /// this duration of no requests on the same conversation.
    #[serde(default = "default_mimo_key_binding_ttl_secs")]
    pub mimo_key_binding_ttl_secs: u64,
    /// Max concurrent requests per upstream key in MiMo key-binding mode.
    /// When exceeded, the conversation waits up to `mimo_key_overflow_wait_ms`
    /// before spilling to another key (binding unchanged). Set to 0 to disable.
    #[serde(default = "default_mimo_key_max_inflight")]
    pub mimo_key_max_inflight: usize,
    /// Milliseconds to wait on the bound key's semaphore before spilling to
    /// another key. Protects prefix-cache affinity while bounding tail latency.
    #[serde(default = "default_mimo_key_overflow_wait_ms")]
    pub mimo_key_overflow_wait_ms: u64,
    /// Weights for multi-factor weighted Ketama routing (used when backend_route_strategy = weighted_ketama).
    #[serde(default)]
    pub score_weights: ScoreWeightsConfig,
    /// Pre-flight backend health checks before upstream (quota preflight).
    #[serde(default)]
    pub preflight: PreflightConfig,
}

/// Configuration for quota preflight / backend health gating.
#[derive(Debug, Clone, Deserialize)]
pub struct PreflightConfig {
    /// Enable quota preflight checks.
    pub enabled: bool,
    /// Cooldown (ms) after which a 429-backend can be retried.
    pub cooldown_ms: u64,
    /// Max consecutive 429s before marking a backend as skipped.
    pub max_consecutive_429: u32,
    /// Skip backend if 429-rate >= this threshold.
    pub skip_threshold: f64,
}

impl Default for PreflightConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cooldown_ms: 60_000,
            max_consecutive_429: 3,
            skip_threshold: 0.5,
        }
    }
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            prefix_aware_cache: false,
            streaming_body_forward: false,
            connection_prewarm: false,
            affinity_prompt_cache_feedback: false,
            delta_cache: false,
            io_uring_backend: false,
            wasm_filters: false,
            mimo_context_compression: false,
            mimo_compression_threshold: default_compression_threshold(),
            upstream_request_gzip: false,
            upstream_request_gzip_min_bytes: default_upstream_request_gzip_min_bytes(),
            mimo_retire_prefix_messages: false,
            mimo_keep_recent_turns: default_mimo_keep_recent_turns(),
            mimo_session_store: false,
            mimo_session_store_ttl_secs: default_mimo_session_store_ttl_secs(),
            mimo_session_store_max_messages: default_mimo_session_store_max_messages(),
            responses_chain_redis: default_responses_chain_redis(),
            responses_chain_ttl_secs: default_responses_chain_ttl_secs(),
            responses_chain_max_capacity: default_responses_chain_max_capacity(),
            responses_chain_max_value_bytes: default_responses_chain_max_value_bytes(),
            responses_chain_max_output_items: default_responses_chain_max_output_items(),
            passthrough_prefix_bytes: default_passthrough_prefix_bytes(),
            backend_load_aware_routing_enabled: false,
            backend_route_strategy: BackendRouteStrategy::default(),
            backend_concurrency_limit_enabled: false,
            default_max_inflight_per_backend: default_max_inflight_per_backend(),
            backend_prefill_overload_threshold_ms: default_backend_prefill_overload_threshold_ms(),
            backend_overload_cooldown_ms: default_backend_overload_cooldown_ms(),
            pipeline_overload_degrade_enabled: false,
            mimo_overload_degrade_to_generic: false,
            mimo_key_binding: false,
            mimo_key_binding_ttl_secs: default_mimo_key_binding_ttl_secs(),
            mimo_key_max_inflight: default_mimo_key_max_inflight(),
            mimo_key_overflow_wait_ms: default_mimo_key_overflow_wait_ms(),
            score_weights: ScoreWeightsConfig::default(),
            preflight: PreflightConfig::default(),
        }
    }
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

fn default_responses_chain_redis() -> bool {
    true
}

fn default_responses_chain_ttl_secs() -> u64 {
    3600
}

fn default_responses_chain_max_capacity() -> u64 {
    50_000
}

fn default_responses_chain_max_value_bytes() -> usize {
    256 * 1024
}

fn default_responses_chain_max_output_items() -> usize {
    64
}

fn default_passthrough_prefix_bytes() -> usize {
    1024
}

fn default_max_inflight_per_backend() -> usize {
    64
}

fn default_backend_prefill_overload_threshold_ms() -> u64 {
    30_000
}

fn default_backend_overload_cooldown_ms() -> u64 {
    60_000
}

fn default_upstream_request_gzip_min_bytes() -> usize {
    4096
}

fn default_compression_threshold() -> usize {
    40
}

fn default_mimo_key_binding_ttl_secs() -> u64 {
    300 // 5 minutes
}

fn default_mimo_key_max_inflight() -> usize {
    6
}

fn default_mimo_key_overflow_wait_ms() -> u64 {
    200
}

/// Serializable config for multi-factor weighted routing scores.
#[derive(Debug, Clone, Deserialize)]
pub struct ScoreWeightsConfig {
    pub health: f64,
    pub latency_inv: f64,
    pub load_inv: f64,
    pub affinity_hit: f64,
    pub rate_429_inv: f64,
}

impl Default for ScoreWeightsConfig {
    fn default() -> Self {
        crate::backend_state::DEFAULT_SCORE_WEIGHTS.into()
    }
}

impl From<crate::backend_state::ScoreWeights> for ScoreWeightsConfig {
    fn from(w: crate::backend_state::ScoreWeights) -> Self {
        Self { health: w.health, latency_inv: w.latency_inv, load_inv: w.load_inv, affinity_hit: w.affinity_hit, rate_429_inv: w.rate_429_inv }
    }
}

impl From<&ScoreWeightsConfig> for crate::backend_state::ScoreWeights {
    fn from(c: &ScoreWeightsConfig) -> Self {
        crate::backend_state::ScoreWeights::from_config(c.health, c.latency_inv, c.load_inv, c.affinity_hit, c.rate_429_inv)
    }
}

pub struct GatewayState {
    pub runtime: Arc<RuntimeConfig>,
    pub tiered_cache: Arc<TieredCache>,
    pub semantic_cache: Option<Arc<SemanticCache>>,
    pub semantic_runtime: SharedSemanticRuntime,
    pub coalescer: Arc<RequestCoalescer>,
    pub reasoning_store: Arc<ReasoningBackend>,
    pub reasoning_config: Arc<parking_lot::RwLock<ReasoningConfig>>,
    pub cors_enabled: Arc<AtomicBool>,
    pub trace_logger: Option<Arc<TraceLogger>>,
    pub raw_capture_logger: Option<Arc<RawCaptureLogger>>,
    pub cache_key_namespace: Option<String>,
    pub pricing: Arc<parking_lot::RwLock<PricingConfig>>,
    /// Max raw SSE bytes stored per stream cache entry (`0` = never store `sse_body`).
    pub max_sse_cache_bytes: usize,
    pub max_request_body_bytes: Arc<AtomicUsize>,
    pub request_semaphore: Arc<Semaphore>,
    pub client_key_limiter: Arc<ClientKeyLimiter>,
    pub client_key_rate_limiter: Arc<ClientKeyRateLimiter>,
    pub deepseek_user_id_limiter: Arc<UpstreamUserIdLimiter>,
    pub features: Arc<parking_lot::RwLock<FeaturesConfig>>,
    /// Tracks session fingerprints that have already been seen (for connection pre-warm).
    /// Bounded to 10K entries with LRU eviction and 1-hour TTL.
    pub seen_session_fingerprints: moka::sync::Cache<String, ()>,
    /// Shared Pingora upstream connector (TCP/TLS connection pool).
    /// Injected from `HttpProxy::connector_arc()` at startup; used for direct pool pre-warm.
    pub upstream_connector: parking_lot::RwLock<Option<Arc<Connector<()>>>>,
    /// affinity_key → backend_name when upstream prompt cache hits were observed (L3 stickiness).
    pub affinity_backend_hints: moka::sync::Cache<String, String>,
    /// Runtime per-backend load, latency and concurrency state.
    pub backend_load: Arc<BackendLoadRegistry>,
    /// Limits concurrent direct pool pre-warm requests.
    pub prewarm_semaphore: Arc<Semaphore>,
    /// Global RPS estimator using pingora-limits::Rate (1-second double-buffered Count-Min Sketch).
    pub global_rate: Arc<pingora_limits::rate::Rate>,
    /// Client Base URL discovery (FRP / OpenResty / observed request headers).
    pub client_endpoint: Arc<parking_lot::RwLock<ClientEndpointSnapshot>>,
    /// MiMo transparent session store (Redis `crab:session:*`).
    pub session_store: Option<Arc<crate::session_store::SessionStore>>,
    /// MiMo conversation-level key binding store (stable_session → key_id).
    pub key_binding_store: Option<Arc<crate::key_binding::KeyBindingStore>>,
    /// Codex Responses API `previous_response_id` chain (response_id → prior output[]).
    pub responses_chain_store: Arc<crate::responses_chain_store::ResponsesChainStore>,
    /// Backend-level circuit breaker registry (4-state machine).
    pub circuit_breakers: std::sync::Arc<crate::circuit_breaker::CircuitBreakerRegistry>,
    /// Model-level lockout registry (per-profile/backend/model).
    pub model_lockouts: std::sync::Arc<crate::model_lockout::ModelLockoutRegistry>,
    /// Client-level lockout registry (brute-force protection).
    pub client_lockouts: std::sync::Arc<crate::client_lockout::ClientLockoutRegistry>,
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
