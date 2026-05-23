mod client_key_limiter;
mod context;
mod debug_log;
mod error;
mod proxy;
mod runtime;
mod sse;
mod tenant;
mod trace_logger;
mod upstream_body;
mod upstream_headers;
mod upstream_pool;
mod upstream_profile;

// Extracted helper modules from proxy.rs
mod helper_fns;
mod error_jsons;
mod response_helpers;
mod cache_helpers;
mod metrics_helpers;
mod sse_rewrite;
mod connection_helpers;

pub use client_key_limiter::{ClientKeyGuard, ClientKeyLimiter, ClientKeyLimitError};
pub use context::{
    ConnectionConfig, GatewayContext, GatewayState, ModelPricing, PricingConfig, ReasoningConfig,
    StoredKey,
};
pub use error::ProxyError;
pub use proxy::GatewayProxy;
pub use sse_rewrite::flush_streaming_reasoning;
pub use cache_helpers::should_store_sse_body;
pub use runtime::{DomainPolicy, RuntimeConfig};
pub use trace_logger::{SanitizedLogEntry, TraceConfig, TraceLogger};
pub use debug_log::debug_agent_log;
pub use upstream_pool::{
    REASONING_NAMESPACE_AUTH, UpstreamKeyGuard, UpstreamKeyPool, UpstreamKeySpec,
    UpstreamKeyStatus, key_preview,
};
pub use tenant::{effective_cache_namespace, resolve_project_id, sanitize_user_id, ProjectResolveError};
pub use upstream_profile::UpstreamProfileRuntime;
