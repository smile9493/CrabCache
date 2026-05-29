mod client_key_limiter;
mod client_key_rate_limiter;
mod context;
mod debug_log;
mod error;
mod masking;
mod phases;
mod profile_build;
mod proxy;
pub mod raw_capture;
mod runtime;
mod sse;
mod stored_key;
mod tenant;
mod trace_logger;
pub mod upstream_body;
mod upstream_body_compress;
mod upstream_headers;
mod upstream_pool;
mod upstream_profile;
mod upstream_response_decompress;
mod upstream_user_id_limiter;
mod user_id_audit;

// Extracted helper modules from proxy.rs
mod body_quick_parse;
mod cache_helpers;
mod cache_response;
mod cache_revalidate;
mod connection_helpers;
pub mod connection_prewarm;
mod error_jsons;
mod helper_fns;
mod metrics_helpers;
mod semantic_runtime;
mod send_helpers;
mod session_store;
mod sse_pipeline;
mod sse_rewrite;
mod streaming_body_forward;

pub use cache_helpers::{
    build_cache_entry, build_cache_entry_with_sse, build_semantic_query_text,
    cache_entry_matches_stream_mode, prepare_response_body_for_cache, should_store_sse_body,
};
pub use cache_response::{
    cached_sse_has_nonempty_content, completion_json_has_visible_client_content,
    json_to_sse_stream, send_cached_response,
};
pub use client_key_limiter::{ClientKeyGuard, ClientKeyLimitError, ClientKeyLimiter};
pub use client_key_rate_limiter::ClientKeyRateLimiter;
pub use context::{
    ConnectionConfig, FeaturesConfig, GatewayContext, GatewayState, ModelPricing, PricingConfig,
    ReasoningConfig,
};
pub use debug_log::{debug_agent_log, init_debug_log, is_debug_agent_log_enabled};
pub use error::ProxyError;
pub use profile_build::{
    ProfileBuildInput, build_profile_runtime, parse_profile_backends, resolve_profile_key_specs,
};
pub use proxy::{
    GatewayProxy, should_defer_upstream_request_body, should_skip_upstream_trailing_empty_eos,
};
pub use raw_capture::{RawCaptureConfig, RawCaptureLogger};
pub use runtime::{DomainPolicy, DomainUsage, RuntimeConfig};
pub use semantic_runtime::{SemanticRuntimeState, SharedSemanticRuntime};
pub use session_store::SessionStore;
pub use sse_rewrite::flush_streaming_reasoning;
pub use stored_key::StoredKey;
pub use streaming_body_forward::{
    StreamingBodyState, StreamingDeferCircuitBreaker, defer_body_incomplete,
    defer_partial_ready_for_arm, hash_body_chunks,
};
pub use tenant::{
    ProjectResolveError, effective_cache_namespace, resolve_project_id, sanitize_user_id,
};
pub use trace_logger::{
    CompositionDebugConfig, SanitizedLogEntry, TraceConfig, TraceLogger, composition_debug_tx,
    set_composition_debug_tx,
};
pub use upstream_pool::{
    REASONING_NAMESPACE_AUTH, UpstreamKeyGuard, UpstreamKeyPool, UpstreamKeySpec,
    UpstreamKeyStatus, key_preview,
};
pub use upstream_profile::UpstreamProfileRuntime;
pub use upstream_user_id_limiter::{
    DeepSeekConcurrencyTier, DeepSeekUserConcurrencyConfig, DeepSeekUserIdLimitError,
    UpstreamUserIdGuard, UpstreamUserIdLimiter, classify_deepseek_v4_tier,
};
pub use user_id_audit::{
    UserIdAuditStatus, apply_user_id_audit_to_entry, compute_user_id_audit, is_deepseek_pipeline,
    parse_user_id_from_json,
};
