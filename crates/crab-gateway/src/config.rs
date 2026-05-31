use crab_control::parse_upstream_base_url;
use crab_pipeline::{
    CursorModelEntry, CursorModelsConfig, PipelineGlobals, PipelineMode, PipelineOverride,
    UpstreamProvider, validate_cursor_models,
};
use crab_proxy::{FeaturesConfig, RawCaptureConfig, UpstreamKeyPool, UpstreamProfileRuntime};
use crab_route::LbRouter;
use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;

pub use crab_proxy::{ConnectionConfig, PricingConfig, ReasoningConfig, TraceConfig};
pub use crab_state::StateBackendConfig;

/// A wrapper around `String` that redacts its value in `Debug` output
/// and `Display` output, preventing accidental leakage of secrets
/// in logs and error messages.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn inner(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    /// Returns the value for use in Authorization headers etc.
    /// Named explicitly to audit every call site.
    pub fn as_authorization(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretString(****)")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "****")
    }
}

impl<'de> serde::Deserialize<'de> for SecretString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(SecretString(s))
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        SecretString(s)
    }
}

impl From<SecretString> for String {
    fn from(s: SecretString) -> Self {
        s.0
    }
}

pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len() - 4..])
}

fn default_empty_api_key() -> SecretString {
    SecretString::new(String::new())
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    pub listen_addr: String,
    pub metrics_addr: String,
    #[serde(default = "default_empty_api_key")]
    pub api_key: SecretString,
    pub upstream: UpstreamConfig,
    pub cache: CacheConfig,
    pub semantic: SemanticConfig,
    pub connection: Option<ConnectionConfig>,
    pub reasoning: Option<ReasoningConfig>,
    pub trace_logging: Option<TraceConfig>,
    pub raw_capture: Option<RawCaptureConfig>,
    pub management: Option<ManagementConfig>,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub gateway: GatewaySection,
    #[serde(default)]
    pub state: StateBackendConfig,
    #[serde(default)]
    pub features: FeaturesConfig,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct GatewaySection {
    /// When true, `api_key` may still be used as a client Bearer (not recommended in production).
    #[serde(default)]
    pub legacy_api_key_as_client_auth: bool,
    /// When true, respond to CORS preflight and allow cross-origin API calls.
    #[serde(default)]
    pub cors_enabled: bool,
    /// Default upstream profile id when model/key do not specify one.
    #[serde(default = "default_upstream_profile_id")]
    pub default_upstream_profile: String,
    /// `auto` or `force_cursor_v4` (emergency: all chat uses Cursor DeepSeek V4 pipeline).
    #[serde(default = "default_pipeline_mode")]
    pub pipeline_mode: String,
    /// Cursor-visible model aliases (e.g. `gpt-4o` → `deepseek-v4-pro`).
    #[serde(default)]
    pub cursor_models: GatewayCursorModelsConfig,
    /// When true, client keys without `project_id` get a stable derived DeepSeek `user_id` (`client:{hash}`).
    #[serde(default)]
    pub auto_project_id_from_client_key: bool,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct GatewayCursorModelsConfig {
    #[serde(default)]
    pub force_deepseek_profile_for_aliases: bool,
    #[serde(default)]
    pub synthetic_models_enabled: bool,
    #[serde(default)]
    pub aliases: HashMap<String, GatewayCursorModelAlias>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayCursorModelAlias {
    pub upstream: String,
    #[serde(default = "default_cursor_alias_pipeline")]
    pub pipeline: String,
}

fn default_cursor_alias_pipeline() -> String {
    "cursor_deepseek_v4".to_string()
}

impl GatewayConfig {
    pub fn cursor_models_config(&self) -> CursorModelsConfig {
        let aliases = self
            .gateway
            .cursor_models
            .aliases
            .iter()
            .map(|(id, a)| {
                (
                    id.clone(),
                    CursorModelEntry {
                        upstream: a.upstream.clone(),
                        pipeline: PipelineOverride::from_str(&a.pipeline),
                    },
                )
            })
            .collect();
        CursorModelsConfig {
            aliases,
            force_deepseek_profile_for_aliases: self
                .gateway
                .cursor_models
                .force_deepseek_profile_for_aliases,
            synthetic_models_enabled: self.gateway.cursor_models.synthetic_models_enabled,
        }
    }
}

fn default_upstream_profile_id() -> String {
    "deepseek".to_string()
}

fn default_pipeline_mode() -> String {
    "auto".to_string()
}

fn default_max_request_body_bytes() -> usize {
    4_194_304
}

fn default_max_concurrent_requests() -> usize {
    512
}

#[derive(Debug, Deserialize, Clone)]
pub struct LimitsConfig {
    #[serde(default = "default_max_request_body_bytes")]
    pub max_request_body_bytes: usize,
    #[serde(default = "default_max_concurrent_requests")]
    pub max_concurrent_requests: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_request_body_bytes: default_max_request_body_bytes(),
            max_concurrent_requests: default_max_concurrent_requests(),
        }
    }
}

const MAX_REQUEST_BODY_BYTES_CAP: usize = 64 * 1024 * 1024;

fn default_management_admin_key() -> SecretString {
    SecretString::new("change-me-in-production".to_string())
}

fn default_invalidate_scan_timeout_secs() -> u64 {
    300
}

pub const WEAK_ADMIN_KEYS: &[&str] = &["change-me-in-production", "dev-only-gateway-admin-secret"];

pub const WEAK_API_KEY_PREFIXES: &[&str] = &["sk-your-"];

#[derive(Debug, Deserialize, Clone)]
pub struct ManagementConfig {
    pub listen_addr: String,
    #[serde(default = "default_management_admin_key")]
    pub admin_key: SecretString,
    /// Max seconds for a background Redis SCAN during cache invalidation.
    #[serde(default = "default_invalidate_scan_timeout_secs")]
    pub invalidate_scan_timeout_secs: u64,
}

impl Default for ManagementConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:9080".to_string(),
            admin_key: default_management_admin_key(),
            invalidate_scan_timeout_secs: default_invalidate_scan_timeout_secs(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct UpstreamProfileConfig {
    pub id: String,
    #[serde(default = "default_profile_provider")]
    pub provider: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<String>,
    pub model: Option<String>,
    pub tls_sni: Option<String>,
    #[serde(default)]
    pub keys: Vec<SecretString>,
}

fn default_profile_provider() -> String {
    "deepseek".to_string()
}

#[derive(Debug, Deserialize)]
pub struct UpstreamConfig {
    pub deepseek_endpoints: Vec<String>,
    pub default_weight: Option<u32>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub tls_sni: Option<String>,
    /// DeepSeek upstream API keys (quota pool). If empty, falls back to top-level `api_key`.
    #[serde(default)]
    pub keys: Vec<SecretString>,
    /// Cooldown after HTTP 429 before reusing a key (seconds).
    #[serde(default = "default_upstream_key_cooldown_secs")]
    pub key_cooldown_secs: u64,
    #[serde(default = "default_health_check_interval_secs")]
    pub health_check_interval_secs: u64,
    #[serde(default = "default_max_coalesce_inflight")]
    pub max_coalesce_inflight: Option<usize>,
    #[serde(default = "default_coalesce_timeout_secs")]
    pub coalesce_timeout_secs: Option<u64>,
    /// Multi-vendor upstream profiles. When empty, a single `deepseek` profile is synthesized from legacy fields.
    #[serde(default)]
    pub profiles: Vec<UpstreamProfileConfig>,
    #[serde(default)]
    pub deepseek_user_concurrency: crab_proxy::DeepSeekUserConcurrencyConfig,
}

fn default_upstream_key_cooldown_secs() -> u64 {
    60
}

fn default_health_check_interval_secs() -> u64 {
    30
}
fn default_max_coalesce_inflight() -> Option<usize> {
    None
}
fn default_coalesce_timeout_secs() -> Option<u64> {
    None
}

#[derive(Debug, Deserialize)]
pub struct CacheConfig {
    pub l0_max_capacity: Option<u64>,
    pub l0_ttl_secs: Option<u64>,
    pub l1_redis_url: String,
    pub l1_pool_size: Option<u32>,
    pub l1_connection_timeout_secs: Option<u64>,
    pub default_ttl_secs: Option<u64>,
    pub model_ttl_overrides: Option<HashMap<String, u64>>,
    pub consumer_ttl_overrides: Option<HashMap<String, u64>>,
    /// When false, streaming responses are not written to L0/L1 cache.
    /// Defaults to true (preserving current behavior).
    #[serde(default = "default_stream_cache_enabled")]
    pub stream_cache_enabled: bool,
    /// Optional namespace prefix for cache keys. When set, enables multi-tenant
    /// isolation by prepending "{namespace}:" to all cache key hashes.
    /// Defaults to empty (no namespace).
    #[serde(default)]
    pub cache_key_namespace: Option<String>,
    /// Fingerprint normalization version. Bump this when normalization rules
    /// change to invalidate old cache entries. Defaults to 1.
    #[serde(default = "default_fingerprint_version")]
    pub fingerprint_version: u32,
    /// When true (default), message content is normalized before hashing
    /// (whitespace, line endings, Unicode NFC). Set to false for emergency rollback.
    #[serde(default = "default_fingerprint_normalize_content")]
    pub fingerprint_normalize_content: bool,
    /// Optional cost-saved pricing configuration for Prometheus metrics.
    /// When omitted, default DeepSeek v3 pricing is used.
    #[serde(default)]
    pub pricing: Option<PricingConfig>,
    /// Max raw SSE bytes stored per stream cache entry. `0` disables storing `sse_body`.
    #[serde(default = "default_max_sse_cache_bytes")]
    pub max_sse_cache_bytes: usize,
}

fn default_max_sse_cache_bytes() -> usize {
    4_194_304
}

const MAX_SSE_CACHE_BYTES_CAP: usize = 64 * 1024 * 1024;

fn default_fingerprint_version() -> u32 {
    1
}

fn default_fingerprint_normalize_content() -> bool {
    true
}

fn default_stream_cache_enabled() -> bool {
    false
}

#[derive(Debug, Deserialize)]
pub struct SemanticConfig {
    pub enabled: bool,
    pub model_path: String,
    pub tokenizer_path: String,
    /// Expected SHA-256 hex digest of the ONNX model file.
    /// When set, the model is verified before loading (production safety).
    pub model_sha256: Option<String>,
    pub qdrant_url: String,
    pub collection_name: String,
    pub vector_size: Option<u64>,
    pub similarity_threshold: Option<f32>,
    pub ttl_secs: Option<u64>,
    #[serde(default = "default_semantic_min_query_chars")]
    pub min_query_chars: usize,
    #[serde(default = "default_semantic_max_query_chars")]
    pub max_query_chars: usize,
    #[serde(default = "default_semantic_max_concurrent_embeds")]
    pub max_concurrent_embeds: usize,
    #[serde(default = "default_semantic_embed_only_on_exact_miss")]
    pub embed_only_on_exact_miss: bool,
}

fn default_semantic_min_query_chars() -> usize {
    32
}
fn default_semantic_max_query_chars() -> usize {
    8192
}
fn default_semantic_max_concurrent_embeds() -> usize {
    4
}
fn default_semantic_embed_only_on_exact_miss() -> bool {
    true
}

impl GatewayConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&content)?;
        // Environment variable overrides api_key from config file
        if let Ok(key) = std::env::var("CRABCACHE_API_KEY")
            && !key.is_empty()
        {
            config.api_key = SecretString::new(key);
        }
        if let Ok(url) = std::env::var("CRABCACHE_L1_REDIS_URL")
            && !url.is_empty()
        {
            config.cache.l1_redis_url = url;
        }
        if let Ok(keys_csv) = std::env::var("CRABCACHE_UPSTREAM_KEYS")
            && !keys_csv.is_empty()
        {
            config.upstream.keys = keys_csv
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| SecretString::new(s.to_string()))
                .collect();
        }
        if config.management.is_none() {
            config.management = Some(ManagementConfig::default());
        }
        if let Some(mgmt) = &mut config.management {
            if let Ok(key) = std::env::var("CRABCACHE_GATEWAY_ADMIN_KEY")
                && !key.is_empty()
            {
                mgmt.admin_key = SecretString::new(key);
            }
            if let Ok(addr) = std::env::var("CRABCACHE_MANAGEMENT_LISTEN")
                && !addr.is_empty()
            {
                mgmt.listen_addr = addr;
            }
        }
        if let Ok(url) = std::env::var("CRABCACHE_UPSTREAM_BASE_URL")
            && !url.is_empty()
        {
            config.upstream.base_url = Some(url);
        }
        if let Ok(model) = std::env::var("CRABCACHE_UPSTREAM_MODEL")
            && !model.is_empty()
        {
            config.upstream.model = Some(model);
        }
        if let Ok(url) = std::env::var("CRABCACHE_TRACE_PG_URL")
            && !url.is_empty()
        {
            let tc = config.trace_logging.get_or_insert_with(Default::default);
            tc.pg_url = Some(url);
        }
        config.apply_upstream_defaults()?;
        Ok(config)
    }

    /// Derive routing peers and TLS SNI from `[upstream].base_url` when omitted (new-api style).
    pub fn apply_upstream_defaults(&mut self) -> anyhow::Result<()> {
        let parsed =
            parse_upstream_base_url(self.upstream_base_url()).map_err(|e| anyhow::anyhow!(e))?;
        self.upstream.base_url = Some(parsed.normalized.clone());
        if self.upstream.deepseek_endpoints.is_empty() {
            self.upstream
                .deepseek_endpoints
                .push(parsed.endpoint.clone());
        }
        if self.upstream.tls_sni.is_none() {
            self.upstream.tls_sni = Some(parsed.tls_sni.clone());
        }
        Ok(())
    }

    pub fn resolved_tls_sni(&self) -> String {
        self.upstream.tls_sni.clone().unwrap_or_else(|| {
            parse_upstream_base_url(self.upstream_base_url())
                .map(|p| p.tls_sni)
                .unwrap_or_else(|_| "api.deepseek.com".to_string())
        })
    }

    pub fn parse_endpoints(&self) -> Vec<crab_route::Backend> {
        let tls_sni = self.resolved_tls_sni();
        crab_control::parse_backend_endpoints(
            &self.upstream.deepseek_endpoints,
            self.upstream.default_weight.unwrap_or(1),
            &tls_sni,
        )
        .unwrap_or_else(|errors| {
            for e in errors {
                tracing::error!(error = %e, "Invalid upstream endpoint");
            }
            Vec::new()
        })
    }

    /// Legacy single-profile list, or explicit `[[upstream.profiles]]`.
    pub fn resolved_upstream_profiles(&self) -> Vec<UpstreamProfileConfig> {
        if !self.upstream.profiles.is_empty() {
            return self.upstream.profiles.clone();
        }
        vec![UpstreamProfileConfig {
            id: "deepseek".to_string(),
            provider: "deepseek".to_string(),
            base_url: self.upstream.base_url.clone(),
            endpoints: self.upstream.deepseek_endpoints.clone(),
            model: self.upstream.model.clone(),
            tls_sni: self.upstream.tls_sni.clone(),
            keys: self.upstream.keys.clone(),
        }]
    }

    fn profile_key_secrets(&self, profile: &UpstreamProfileConfig) -> Vec<String> {
        let mut keys: Vec<String> = profile
            .keys
            .iter()
            .map(|k| k.inner().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        // Only DeepSeek profiles may fall back to the global `[upstream]` key list.
        if keys.is_empty() && profile.provider.eq_ignore_ascii_case("deepseek") {
            keys = self.upstream_key_secrets();
        }
        keys
    }

    fn parse_profile_endpoints(
        &self,
        profile: &UpstreamProfileConfig,
    ) -> anyhow::Result<Vec<crab_route::Backend>> {
        let base_url = profile
            .base_url
            .as_deref()
            .unwrap_or_else(|| self.upstream_base_url());
        let parsed = parse_upstream_base_url(base_url).map_err(|e| anyhow::anyhow!(e))?;
        let mut endpoints = profile.endpoints.clone();
        if endpoints.is_empty() {
            endpoints.push(parsed.endpoint);
        }
        let tls_sni = profile.tls_sni.clone().unwrap_or(parsed.tls_sni);
        let weight = self.upstream.default_weight.unwrap_or(1);
        let mut backends = Vec::new();
        let mut errors = Vec::new();
        for (i, endpoint) in endpoints.iter().enumerate() {
            if let Ok(addr) = endpoint.parse::<SocketAddr>() {
                backends.push(crab_route::Backend::new(
                    format!("{}-backend-{}", profile.id, i + 1),
                    addr,
                    weight,
                    tls_sni.clone(),
                ));
                continue;
            }
            match endpoint.to_socket_addrs() {
                Ok(mut addrs) => {
                    if let Some(addr) = addrs.next() {
                        backends.push(crab_route::Backend::new(
                            format!("{}-backend-{}", profile.id, i + 1),
                            addr,
                            weight,
                            tls_sni.clone(),
                        ));
                    } else {
                        errors.push(format!("No addresses for '{endpoint}'"));
                    }
                }
                Err(e) => errors.push(format!("Cannot resolve '{endpoint}': {e}")),
            }
        }
        if !errors.is_empty() {
            anyhow::bail!("{}", errors.join("; "));
        }
        if backends.is_empty() {
            anyhow::bail!("profile '{}' has no endpoints", profile.id);
        }
        Ok(backends)
    }

    pub fn pipeline_globals(&self) -> PipelineGlobals {
        let ids: Vec<String> = self
            .resolved_upstream_profiles()
            .into_iter()
            .map(|p| p.id)
            .collect();
        let mode = PipelineMode::from_str(&self.gateway.pipeline_mode);
        PipelineGlobals::with_profiles_mode_and_cursor_models(
            self.gateway.default_upstream_profile.clone(),
            ids,
            mode,
            self.cursor_models_config(),
        )
    }

    pub fn build_upstream_profile_runtimes(
        &self,
        rt: &tokio::runtime::Runtime,
    ) -> anyhow::Result<indexmap::IndexMap<String, Arc<UpstreamProfileRuntime>>> {
        let cooldown = self.upstream_key_cooldown_secs();
        let mut map = indexmap::IndexMap::new();
        for profile in self.resolved_upstream_profiles() {
            let backends = self.parse_profile_endpoints(&profile)?;
            let router = rt.block_on(async { LbRouter::new(&backends) })?;
            let base_url = profile
                .base_url
                .clone()
                .unwrap_or_else(|| self.upstream_base_url().to_string());
            let fallback_model = profile
                .model
                .clone()
                .unwrap_or_else(|| self.fallback_model().to_string());
            let tls_sni = profile
                .tls_sni
                .clone()
                .unwrap_or_else(|| self.resolved_tls_sni());
            let keys = self.profile_key_secrets(&profile);
            let pool = UpstreamKeyPool::from_secrets(keys, cooldown, 0);
            let pool_handle = Arc::new(RwLock::new(pool));
            map.insert(
                profile.id.clone(),
                Arc::new(UpstreamProfileRuntime {
                    id: profile.id.clone(),
                    provider: UpstreamProvider::from_str(&profile.provider),
                    base_url,
                    fallback_model,
                    tls_sni,
                    router,
                    upstream_pool: pool_handle,
                    proxy_url: None,
                    fallback_profile_id: None,
                    fallback_max_retries: 2,
                }),
            );
        }
        Ok(map)
    }

    pub fn management_config(&self) -> ManagementConfig {
        self.management.clone().unwrap_or_default()
    }

    pub fn upstream_base_url(&self) -> &str {
        self.upstream
            .base_url
            .as_deref()
            .unwrap_or("https://api.deepseek.com")
    }

    pub fn fallback_model(&self) -> &str {
        self.upstream.model.as_deref().unwrap_or("deepseek-v4-pro")
    }

    /// Resolved DeepSeek upstream key secrets for the outbound pool.
    pub fn upstream_key_secrets(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .upstream
            .keys
            .iter()
            .map(|k| k.inner().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        if keys.is_empty() {
            let api = self.api_key.inner();
            if !api.is_empty() {
                keys.push(api.to_string());
            }
        }
        keys
    }

    pub fn upstream_key_cooldown_secs(&self) -> u64 {
        self.upstream.key_cooldown_secs
    }

    /// Validate the configuration and return a list of errors.
    /// Returns `Ok(())` if all checks pass, or `Err(errors)` with a list of
    /// human-readable validation error messages.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        let upstream_keys = self.upstream_key_secrets();
        if upstream_keys.is_empty() {
            errors.push(
                "At least one upstream DeepSeek API key is required (upstream.keys, api_key, or CRABCACHE_UPSTREAM_KEYS / CRABCACHE_API_KEY)".into(),
            );
        } else {
            for (i, key) in upstream_keys.iter().enumerate() {
                if key.starts_with("sk-your-") {
                    errors.push(format!(
                        "upstream key #{} appears to be a placeholder (starts with 'sk-your-')",
                        i + 1
                    ));
                }
            }
        }

        // Validate each profile can resolve at least one key (explicit or fallback to global).
        for profile in self.resolved_upstream_profiles() {
            let resolved = self.profile_key_secrets(&profile);
            if resolved.is_empty() {
                errors.push(format!(
                    "upstream profile '{}' has no API keys and global fallback is also empty",
                    profile.id
                ));
            }
        }

        if let Err(e) = parse_upstream_base_url(self.upstream_base_url()) {
            errors.push(format!("invalid upstream base_url: {e}"));
        }

        if self.upstream.deepseek_endpoints.is_empty() {
            errors.push(
                "At least one upstream endpoint is required in [upstream].deepseek_endpoints (or set base_url to auto-derive)"
                    .into(),
            );
        }

        for ep in &self.upstream.deepseek_endpoints {
            if ep.parse::<SocketAddr>().is_err() && ep.to_socket_addrs().is_err() {
                errors.push(format!("Invalid upstream endpoint address: '{ep}'"));
            }
        }

        if let Some(threshold) = self.semantic.similarity_threshold
            && !(0.0..=1.0).contains(&threshold)
        {
            errors.push("semantic.similarity_threshold must be in [0.0, 1.0]".into());
        }

        if self.metrics_addr.parse::<SocketAddr>().is_err() {
            errors.push(format!("Invalid metrics_addr: '{}'", self.metrics_addr));
        }

        if self.listen_addr.parse::<SocketAddr>().is_err() {
            errors.push(format!("Invalid listen_addr: '{}'", self.listen_addr));
        }

        if let Some(mgmt) = &self.management {
            if mgmt.listen_addr.parse::<SocketAddr>().is_err() {
                errors.push(format!(
                    "Invalid management.listen_addr: '{}'",
                    mgmt.listen_addr
                ));
            }
            let admin_key = mgmt.admin_key.inner();
            if admin_key.is_empty() {
                errors.push("management.admin_key must not be empty".into());
            } else if admin_key == "change-me-in-production" {
                errors.push(
                    "management.admin_key is still the default placeholder; set a strong key"
                        .into(),
                );
            }
        }

        if self.cache.max_sse_cache_bytes > MAX_SSE_CACHE_BYTES_CAP {
            errors.push(format!(
                "cache.max_sse_cache_bytes must be <= {MAX_SSE_CACHE_BYTES_CAP} (64 MiB)"
            ));
        }

        if self.limits.max_request_body_bytes == 0 {
            errors.push("limits.max_request_body_bytes must be > 0".into());
        } else if self.limits.max_request_body_bytes > MAX_REQUEST_BODY_BYTES_CAP {
            errors.push(format!(
                "limits.max_request_body_bytes must be <= {MAX_REQUEST_BODY_BYTES_CAP} (64 MiB)"
            ));
        }

        if self.limits.max_concurrent_requests == 0 {
            errors.push("limits.max_concurrent_requests must be > 0".into());
        }

        if let Some(reasoning) = &self.reasoning {
            let strategy = reasoning.missing_reasoning_strategy.as_str();
            if strategy != "recover" && strategy != "reject" {
                errors.push(format!(
                    "reasoning.missing_reasoning_strategy must be \"recover\" or \"reject\" (deepseek-cursor-proxy), got \"{strategy}\""
                ));
            }
            if reasoning.thinking_mode != "enabled" && reasoning.thinking_mode != "disabled" {
                errors.push(format!(
                    "reasoning.thinking_mode must be \"enabled\" or \"disabled\", got \"{}\"",
                    reasoning.thinking_mode
                ));
            }
        }

        if let Some(coalesce_max) = self.upstream.max_coalesce_inflight
            && coalesce_max > self.limits.max_concurrent_requests
        {
            errors.push(format!(
                    "upstream.max_coalesce_inflight ({coalesce_max}) must be <= limits.max_concurrent_requests ({})",
                    self.limits.max_concurrent_requests
                ));
        }

        if let Err(msg) = validate_cursor_models(&self.cursor_models_config()) {
            errors.push(msg);
        }

        if let Some(ref tl) = self.trace_logging {
            if tl.enabled {
                if tl.max_lines < 100 {
                    errors.push(format!(
                        "trace_logging.max_lines ({}) is too low, minimum is 100",
                        tl.max_lines
                    ));
                }
                if tl.max_files == 0 {
                    errors.push("trace_logging.max_files must be at least 1".to_string());
                }
            }
            if let Some(ref cd) = tl.composition_debug {
                if cd.enabled {
                    if cd.max_lines < 100 {
                        errors.push(format!(
                            "trace_logging.composition_debug.max_lines ({}) is too low, minimum is 100",
                            cd.max_lines
                        ));
                    }
                    if cd.max_files == 0 {
                        errors.push(
                            "trace_logging.composition_debug.max_files must be at least 1"
                                .to_string(),
                        );
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Non-fatal deployment warnings (logged at startup).
    pub fn security_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        let api_key = self.api_key.inner();
        for prefix in WEAK_API_KEY_PREFIXES {
            if api_key.starts_with(prefix) {
                warnings.push(format!(
                    "api_key looks like a placeholder (prefix '{prefix}'); set CRABCACHE_API_KEY or a real DeepSeek key"
                ));
                break;
            }
        }
        if api_key.len() < 16 && !api_key.is_empty() {
            warnings.push("api_key is unusually short for production".into());
        }

        if let Some(mgmt) = &self.management {
            let admin_key = mgmt.admin_key.inner();
            if WEAK_ADMIN_KEYS.contains(&admin_key) {
                warnings.push(format!(
                    "management.admin_key is a known weak/default value ({admin_key}); set a strong key via config or CRABCACHE_GATEWAY_ADMIN_KEY"
                ));
                if std::env::var("CRABCACHE_GATEWAY_ADMIN_KEY").is_err() {
                    warnings.push(
                        "CRABCACHE_GATEWAY_ADMIN_KEY is not set; management API is using the weak key from the config file"
                            .into(),
                    );
                }
            }
        }

        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crab_proxy::CompositionDebugConfig;

    #[test]
    fn test_config_load_missing_file() {
        let result = GatewayConfig::load("/nonexistent/config.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_management_defaults() {
        let mgmt = ManagementConfig::default();
        assert_eq!(mgmt.listen_addr, "127.0.0.1:9080");
        assert_eq!(mgmt.invalidate_scan_timeout_secs, 300);
    }

    #[test]
    fn test_default_max_sse_cache_bytes() {
        assert_eq!(default_max_sse_cache_bytes(), 4_194_304);
    }

    #[test]
    fn test_limits_defaults() {
        let limits = LimitsConfig::default();
        assert_eq!(limits.max_request_body_bytes, 4_194_304);
        assert_eq!(limits.max_concurrent_requests, 512);
    }

    #[test]
    fn test_upstream_base_url_env_override() {
        let dir = std::env::temp_dir().join(format!("crabcache_cfg_bu_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gateway.toml");
        std::fs::write(
            &path,
            r#"
listen_addr = "127.0.0.1:8080"
metrics_addr = "127.0.0.1:9090"
api_key = "sk-test-key-1234567890"
[upstream]
base_url = "https://api.deepseek.com"
deepseek_endpoints = ["api.deepseek.com:443"]
[cache]
l1_redis_url = "redis://127.0.0.1:6379"
[semantic]
enabled = false
model_path = ""
tokenizer_path = ""
qdrant_url = ""
collection_name = ""
"#,
        )
        .unwrap();

        // SAFETY: test runs single-threaded; no concurrent env access.
        unsafe {
            std::env::set_var("CRABCACHE_UPSTREAM_BASE_URL", "https://api.openai.com");
        }
        let config = GatewayConfig::load(path.to_str().unwrap()).unwrap();
        // SAFETY: test runs single-threaded; no concurrent env access.
        unsafe {
            std::env::remove_var("CRABCACHE_UPSTREAM_BASE_URL");
        }
        let _ = std::fs::remove_dir_all(dir);

        assert_eq!(config.upstream_base_url(), "https://api.openai.com");
    }

    #[test]
    fn test_l1_redis_url_env_override() {
        let dir = std::env::temp_dir().join(format!("crabcache_cfg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gateway.toml");
        std::fs::write(
            &path,
            r#"
listen_addr = "127.0.0.1:8080"
metrics_addr = "127.0.0.1:9090"
api_key = "sk-test-key-1234567890"
upstream = { deepseek_endpoints = ["127.0.0.1:443"] }
cache = { l1_redis_url = "redis://127.0.0.1:6379" }
semantic = { enabled = false, model_path = "", tokenizer_path = "", qdrant_url = "", collection_name = "" }
"#,
        )
        .unwrap();

        // SAFETY: test runs single-threaded; no concurrent env access.
        unsafe {
            std::env::set_var("CRABCACHE_L1_REDIS_URL", "redis://redis:6379");
        }
        let config = GatewayConfig::load(path.to_str().unwrap()).unwrap();
        // SAFETY: test runs single-threaded; no concurrent env access.
        unsafe {
            std::env::remove_var("CRABCACHE_L1_REDIS_URL");
        }
        let _ = std::fs::remove_dir_all(dir);

        assert_eq!(config.cache.l1_redis_url, "redis://redis:6379");
    }

    #[test]
    fn test_validate_coalesce_exceeds_concurrent_limit() {
        let config = GatewayConfig {
            listen_addr: "127.0.0.1:8080".into(),
            metrics_addr: "127.0.0.1:9090".into(),
            api_key: SecretString::new("sk-real-key-12345678".into()),
            upstream: UpstreamConfig {
                deepseek_endpoints: vec!["127.0.0.1:443".into()],
                default_weight: None,
                base_url: None,
                model: None,
                tls_sni: None,
                keys: vec![],
                key_cooldown_secs: 60,
                health_check_interval_secs: 30,
                max_coalesce_inflight: Some(10_000),
                coalesce_timeout_secs: None,
                profiles: vec![],
                deepseek_user_concurrency: crab_proxy::DeepSeekUserConcurrencyConfig::default(),
            },
            gateway: GatewaySection::default(),
            cache: CacheConfig {
                l0_max_capacity: None,
                l0_ttl_secs: None,
                l1_redis_url: "redis://127.0.0.1".into(),
                l1_pool_size: None,
                l1_connection_timeout_secs: None,
                default_ttl_secs: None,
                model_ttl_overrides: None,
                consumer_ttl_overrides: None,
                stream_cache_enabled: false,
                cache_key_namespace: None,
                fingerprint_version: 1,
                fingerprint_normalize_content: true,
                pricing: None,
                max_sse_cache_bytes: default_max_sse_cache_bytes(),
            },
            semantic: SemanticConfig {
                enabled: false,
                model_path: String::new(),
                tokenizer_path: String::new(),
                qdrant_url: String::new(),
                collection_name: String::new(),
                vector_size: None,
                similarity_threshold: None,
                ttl_secs: None,
                min_query_chars: 32,
                max_query_chars: 8192,
                max_concurrent_embeds: 4,
                embed_only_on_exact_miss: true,
                model_sha256: None,
            },
            connection: None,
            reasoning: None,
            trace_logging: None,
            raw_capture: None,
            management: None,
            limits: LimitsConfig::default(),
            state: StateBackendConfig::default(),
            features: FeaturesConfig::default(),
        };
        let err = config.validate().unwrap_err();
        assert!(err.iter().any(|e| e.contains("max_coalesce_inflight")));
    }

    #[test]
    fn test_security_warnings_weak_admin() {
        let config = GatewayConfig {
            listen_addr: "127.0.0.1:8080".into(),
            metrics_addr: "127.0.0.1:9090".into(),
            api_key: SecretString::new("sk-real-key-12345678".into()),
            upstream: UpstreamConfig {
                deepseek_endpoints: vec!["127.0.0.1:443".into()],
                default_weight: None,
                base_url: None,
                model: None,
                tls_sni: None,
                keys: vec![],
                key_cooldown_secs: 60,
                health_check_interval_secs: 30,
                max_coalesce_inflight: None,
                coalesce_timeout_secs: None,
                profiles: vec![],
                deepseek_user_concurrency: crab_proxy::DeepSeekUserConcurrencyConfig::default(),
            },
            gateway: GatewaySection::default(),
            cache: CacheConfig {
                l0_max_capacity: None,
                l0_ttl_secs: None,
                l1_redis_url: "redis://127.0.0.1".into(),
                l1_pool_size: None,
                l1_connection_timeout_secs: None,
                default_ttl_secs: None,
                model_ttl_overrides: None,
                consumer_ttl_overrides: None,
                stream_cache_enabled: false,
                cache_key_namespace: None,
                fingerprint_version: 1,
                fingerprint_normalize_content: true,
                pricing: None,
                max_sse_cache_bytes: default_max_sse_cache_bytes(),
            },
            semantic: SemanticConfig {
                enabled: false,
                model_path: String::new(),
                tokenizer_path: String::new(),
                qdrant_url: String::new(),
                collection_name: String::new(),
                vector_size: None,
                similarity_threshold: None,
                ttl_secs: None,
                min_query_chars: 32,
                max_query_chars: 8192,
                max_concurrent_embeds: 4,
                embed_only_on_exact_miss: true,
                model_sha256: None,
            },
            connection: None,
            reasoning: None,
            trace_logging: None,
            raw_capture: None,
            management: Some(ManagementConfig {
                listen_addr: "127.0.0.1:9080".into(),
                admin_key: SecretString::new("dev-only-gateway-admin-secret".into()),
                invalidate_scan_timeout_secs: 300,
            }),
            limits: LimitsConfig::default(),
            state: StateBackendConfig::default(),
            features: FeaturesConfig::default(),
        };
        let warnings = config.security_warnings();
        assert!(warnings.iter().any(|w| w.contains("admin_key")));
    }

    fn minimal_valid_gateway_config() -> GatewayConfig {
        GatewayConfig {
            listen_addr: "127.0.0.1:8080".into(),
            metrics_addr: "127.0.0.1:9090".into(),
            api_key: SecretString::new("sk-real-key-12345678".into()),
            upstream: UpstreamConfig {
                deepseek_endpoints: vec!["127.0.0.1:443".into()],
                default_weight: None,
                base_url: None,
                model: None,
                tls_sni: None,
                keys: vec![],
                key_cooldown_secs: 60,
                health_check_interval_secs: 30,
                max_coalesce_inflight: None,
                coalesce_timeout_secs: None,
                profiles: vec![],
                deepseek_user_concurrency: crab_proxy::DeepSeekUserConcurrencyConfig::default(),
            },
            gateway: GatewaySection::default(),
            cache: CacheConfig {
                l0_max_capacity: None,
                l0_ttl_secs: None,
                l1_redis_url: "redis://127.0.0.1".into(),
                l1_pool_size: None,
                l1_connection_timeout_secs: None,
                default_ttl_secs: None,
                model_ttl_overrides: None,
                consumer_ttl_overrides: None,
                stream_cache_enabled: false,
                cache_key_namespace: None,
                fingerprint_version: 1,
                fingerprint_normalize_content: true,
                pricing: None,
                max_sse_cache_bytes: default_max_sse_cache_bytes(),
            },
            semantic: SemanticConfig {
                enabled: false,
                model_path: String::new(),
                tokenizer_path: String::new(),
                qdrant_url: String::new(),
                collection_name: String::new(),
                vector_size: None,
                similarity_threshold: None,
                ttl_secs: None,
                min_query_chars: 32,
                max_query_chars: 8192,
                max_concurrent_embeds: 4,
                embed_only_on_exact_miss: true,
                model_sha256: None,
            },
            connection: None,
            reasoning: None,
            trace_logging: None,
            raw_capture: None,
            management: None,
            limits: LimitsConfig::default(),
            state: StateBackendConfig::default(),
            features: FeaturesConfig::default(),
        }
    }

    #[test]
    fn validate_rejects_trace_logging_max_lines_too_low() {
        let mut config = minimal_valid_gateway_config();
        config.trace_logging = Some(TraceConfig {
            enabled: true,
            path: "/tmp/trace.jsonl".into(),
            max_lines: 50,
            max_files: 1,
            composition_debug: None,
            max_payload_bytes: 0,
            max_response_preview_bytes: 0,
            pg_url: None,
        });
        let err = config.validate().unwrap_err();
        assert!(err.iter().any(|e| e.contains("trace_logging.max_lines")));
    }

    #[test]
    fn validate_rejects_trace_logging_max_files_zero() {
        let mut config = minimal_valid_gateway_config();
        config.trace_logging = Some(TraceConfig {
            enabled: true,
            path: "/tmp/trace.jsonl".into(),
            max_lines: 100,
            max_files: 0,
            composition_debug: None,
            max_payload_bytes: 0,
            max_response_preview_bytes: 0,
            pg_url: None,
        });
        let err = config.validate().unwrap_err();
        assert!(err.iter().any(|e| e.contains("trace_logging.max_files")));
    }

    #[test]
    fn validate_rejects_composition_debug_max_lines_too_low() {
        let mut config = minimal_valid_gateway_config();
        config.trace_logging = Some(TraceConfig {
            enabled: false,
            path: "/tmp/trace.jsonl".into(),
            max_lines: 100,
            max_files: 1,
            composition_debug: Some(CompositionDebugConfig {
                enabled: true,
                path: "/tmp/debug.jsonl".into(),
                max_lines: 50,
                max_files: 1,
            }),
            max_payload_bytes: 0,
            max_response_preview_bytes: 0,
            pg_url: None,
        });
        let err = config.validate().unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("composition_debug.max_lines"))
        );
    }

    #[test]
    fn validate_rejects_composition_debug_max_files_zero() {
        let mut config = minimal_valid_gateway_config();
        config.trace_logging = Some(TraceConfig {
            enabled: false,
            path: "/tmp/trace.jsonl".into(),
            max_lines: 100,
            max_files: 1,
            composition_debug: Some(CompositionDebugConfig {
                enabled: true,
                path: "/tmp/debug.jsonl".into(),
                max_lines: 100,
                max_files: 0,
            }),
            max_payload_bytes: 0,
            max_response_preview_bytes: 0,
            pg_url: None,
        });
        let err = config.validate().unwrap_err();
        assert!(
            err.iter()
                .any(|e| e.contains("composition_debug.max_files"))
        );
    }
}
