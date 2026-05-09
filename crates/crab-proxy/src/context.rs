use crab_cache::{CacheEntry, RequestCoalescer, TieredCache};
use crab_route::AffinityRouter;
use crab_semantic::SemanticCache;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

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

pub struct GatewayContext {
    pub request_id: String,
    pub cache_key: Option<String>,
    pub cache_hit: Option<CacheEntry>,
    pub is_streaming: bool,
    pub is_models_list: bool,
    pub model: String,
    pub consumer: Option<String>,
    pub request_start: Instant,
    pub upstream_start: Option<Instant>,
    pub ttft: Option<std::time::Duration>,
    pub accumulated_body: Vec<u8>,
    pub is_coalesced_follower: bool,
}

impl GatewayContext {
    pub fn new(request_id: String) -> Self {
        Self {
            request_id,
            cache_key: None,
            cache_hit: None,
            is_streaming: false,
            is_models_list: false,
            model: String::new(),
            consumer: None,
            request_start: Instant::now(),
            upstream_start: None,
            ttft: None,
            accumulated_body: Vec::new(),
            is_coalesced_follower: false,
        }
    }
}

pub struct GatewayState {
    pub router: Arc<AffinityRouter>,
    pub tiered_cache: Arc<TieredCache>,
    pub semantic_cache: Arc<SemanticCache>,
    pub coalescer: Arc<RequestCoalescer>,
    pub api_key: String,
    pub conn_config: ConnectionConfig,
}
