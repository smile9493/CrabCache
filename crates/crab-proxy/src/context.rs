use crate::runtime::RuntimeConfig;
use crab_cache::{CacheEntry, CoalesceGuard, RequestCoalescer, TieredCache};
use crab_reasoning::{
    CursorReasoningDisplayAdapter, PreparedRequest, ReasoningStore, StreamAccumulator,
};
use crab_semantic::SemanticCache;
use crab_metrics::CacheTier;
use crate::TraceLogger;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct StoredKey {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConnectionConfig {
    pub tcp_keepalive_idle_secs: Option<u64>,
    pub tcp_keepalive_interval_secs: Option<u64>,
    pub tcp_keepalive_count: Option<usize>,
    pub idle_timeout_secs: Option<u64>,
    pub h2_ping_interval_secs: Option<u64>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            tcp_keepalive_idle_secs: Some(60),
            tcp_keepalive_interval_secs: Some(10),
            tcp_keepalive_count: Some(3),
            idle_timeout_secs: Some(90),
            h2_ping_interval_secs: Some(30),
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
    pub cache_db_path: String,
    pub cache_max_age_secs: Option<u64>,
    pub cache_max_rows: Option<usize>,
}

impl Default for ReasoningConfig {
    fn default() -> Self {
        Self {
            thinking_mode: "enabled".to_string(),
            reasoning_effort: "max".to_string(),
            missing_reasoning_strategy: "recover".to_string(),
            display_reasoning: true,
            collapsible_reasoning: true,
            cache_db_path: ":memory:".to_string(),
            cache_max_age_secs: Some(30 * 24 * 3600),
            cache_max_rows: Some(100_000),
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
    pub request_start: Instant,
    pub upstream_start: Option<Instant>,
    pub ttft: Option<std::time::Duration>,
    pub accumulated_body: Vec<u8>,
    pub is_coalesced_follower: bool,
    pub coalesce_guard: Option<CoalesceGuard>,
    pub original_request_body: Option<Vec<u8>>,
    pub prepared_request: Option<PreparedRequest>,
    pub new_request_body: Option<Vec<u8>>,
    pub stream_accumulator: Option<StreamAccumulator>,
    pub display_adapter: Option<CursorReasoningDisplayAdapter>,
    pub pending_recovery_notice: Option<String>,
    pub authorization: Option<String>,
    pub req_hash: Option<String>,
    pub content_length: usize,
    pub total_tokens: u64,
    pub conversation_id: Option<String>,
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
            request_start: Instant::now(),
            upstream_start: None,
            ttft: None,
            accumulated_body: Vec::new(),
            is_coalesced_follower: false,
            coalesce_guard: None,
            original_request_body: None,
            prepared_request: None,
            new_request_body: None,
            stream_accumulator: None,
            display_adapter: None,
            pending_recovery_notice: None,
            authorization: None,
            req_hash: None,
            content_length: 0,
            total_tokens: 0,
            conversation_id: None,
        }
    }
}

pub struct GatewayState {
    pub runtime: Arc<RuntimeConfig>,
    pub tiered_cache: Arc<TieredCache>,
    pub semantic_cache: Option<Arc<SemanticCache>>,
    pub coalescer: Arc<RequestCoalescer>,
    pub reasoning_store: Arc<ReasoningStore>,
    pub reasoning_config: ReasoningConfig,
    pub trace_logger: Option<Arc<TraceLogger>>,
    pub cache_key_namespace: Option<String>,
}
