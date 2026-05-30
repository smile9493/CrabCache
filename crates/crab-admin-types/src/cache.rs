use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastInvalidateView {
    pub scope: String,
    pub status: String,
    pub at_secs: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidateJobView {
    pub scope: String,
    pub phase: String,
    pub error: Option<String>,
    pub started_at_secs: u64,
    pub completed_at_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheOpsView {
    pub fingerprint_version: u32,
    pub fingerprint_normalize: bool,
    pub stream_cache_enabled: bool,
    pub last_invalidate: Option<LastInvalidateView>,
    #[serde(default)]
    pub invalidate_all_in_progress: bool,
    #[serde(default)]
    pub invalidate_job: Option<InvalidateJobView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheBody {
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidateCacheResult {
    pub scope: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfigBody {
    pub version: u32,
    pub normalize_content: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingConfigView {
    pub default_input_price_per_million: f64,
    pub default_output_price_per_million: f64,
    #[serde(default)]
    pub model_overrides: Vec<(String, ModelPricingView)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricingView {
    pub input: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfigView {
    pub prefix_aware_cache: bool,
    pub streaming_body_forward: bool,
    pub connection_prewarm: bool,
    pub affinity_prompt_cache_feedback: bool,
    pub delta_cache: bool,
    pub io_uring_backend: bool,
    pub wasm_filters: bool,
    pub mimo_context_compression: bool,
    pub mimo_compression_threshold: usize,
    pub upstream_request_gzip: bool,
    pub upstream_request_gzip_min_bytes: usize,
    pub mimo_retire_prefix_messages: bool,
    pub mimo_keep_recent_turns: usize,
    pub mimo_session_store: bool,
    pub mimo_session_store_ttl_secs: u64,
    pub mimo_session_store_max_messages: usize,
    pub passthrough_prefix_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceLoggingConfigView {
    pub max_lines: u64,
    pub max_files: u64,
    pub max_payload_bytes: usize,
    pub max_response_preview_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCaptureConfigView {
    pub enabled: bool,
    pub sample_rate: f64,
    pub mask_api_keys: bool,
    pub sample_always_on_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCacheToggle {
    pub enabled: bool,
}

/// Admin API name for [`StreamCacheToggle`].
pub type StreamCacheConfig = StreamCacheToggle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    pub default_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: Vec<(String, u64)>,
    #[serde(default)]
    pub consumer_overrides: Vec<(String, u64)>,
    /// Combined overrides keyed by `"consumer:model"`.
    #[serde(default)]
    pub consumer_model_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCacheConfigRequest {
    pub l0_ttl_secs: u64,
    pub l1_ttl_secs: u64,
    #[serde(default)]
    pub model_overrides: Vec<(String, u64)>,
    #[serde(default)]
    pub consumer_overrides: Vec<(String, u64)>,
    /// Combined overrides keyed by `"consumer:model"`.
    #[serde(default)]
    pub consumer_model_overrides: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSemanticConfigRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    pub similarity_threshold: f64,
    #[serde(default)]
    pub ttl_secs: u64,
    #[serde(default)]
    pub min_query_chars: usize,
    #[serde(default)]
    pub max_query_chars: usize,
    #[serde(default)]
    pub max_concurrent_embeds: usize,
}
