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
}
