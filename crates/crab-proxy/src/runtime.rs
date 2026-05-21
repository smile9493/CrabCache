use crate::context::{ConnectionConfig, StoredKey};
use crate::upstream_pool::UpstreamKeyPool;
use crate::upstream_profile::UpstreamProfileRuntime;
use crab_cache::{FingerprintConfig, TtlConfig};
use crab_pipeline::{CursorModelsConfig, PipelineGlobals, PipelineMode};
use crab_route::{AffinityRouter, BackendHealth};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DomainPolicy {
    pub monthly_token_budget: u64,
    pub monthly_cost_budget_usd: f64,
    pub min_hit_rate: f64,
    pub enabled: bool,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Default)]
struct DomainUsage {
    tokens: u64,
    spend_usd: f64,
}

pub struct RuntimeConfig {
    pub keys: DashMap<String, StoredKey>,
    pub ttl: Arc<RwLock<TtlConfig>>,
    pub router: RwLock<AffinityRouter>,
    pub conn_config: RwLock<ConnectionConfig>,
    pub stream_cache_enabled: AtomicBool,
    pub fingerprint: RwLock<FingerprintConfig>,
    pub upstream_base_url: RwLock<String>,
    pub fallback_model: RwLock<String>,
    /// DeepSeek upstream API key pool (outbound Bearer).
    pub upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
    pub upstream_profiles: HashMap<String, Arc<UpstreamProfileRuntime>>,
    pub default_upstream_profile_id: RwLock<String>,
    pub pipeline_globals: RwLock<PipelineGlobals>,
    /// When true, tokens in `legacy_client_tokens` may authenticate as clients.
    pub legacy_api_key_as_client_auth: bool,
    pub legacy_client_tokens: HashSet<String>,
    pub domain_policies: Arc<RwLock<HashMap<String, DomainPolicy>>>,
    domain_usage: Mutex<HashMap<String, DomainUsage>>,
    pub started_at: Instant,
    pub backend_health: Arc<RwLock<std::collections::HashMap<String, BackendHealth>>>,
}

impl RuntimeConfig {
    pub fn new(
        router: AffinityRouter,
        ttl: Arc<RwLock<TtlConfig>>,
        conn_config: ConnectionConfig,
        stream_cache_enabled: bool,
        fingerprint: FingerprintConfig,
        upstream_base_url: String,
        fallback_model: String,
        upstream_pool: Arc<UpstreamKeyPool>,
        upstream_profiles: HashMap<String, Arc<UpstreamProfileRuntime>>,
        default_upstream_profile_id: String,
        pipeline_globals: PipelineGlobals,
        legacy_api_key_as_client_auth: bool,
        legacy_client_tokens: HashSet<String>,
    ) -> Arc<Self> {
        let backends = router
            .backends()
            .iter()
            .map(|b| (b.name.clone(), BackendHealth::new_healthy()))
            .collect::<std::collections::HashMap<_, _>>();

        Arc::new(Self {
            keys: DashMap::new(),
            ttl,
            router: RwLock::new(router),
            conn_config: RwLock::new(conn_config),
            stream_cache_enabled: AtomicBool::new(stream_cache_enabled),
            fingerprint: RwLock::new(fingerprint),
            upstream_base_url: RwLock::new(upstream_base_url),
            fallback_model: RwLock::new(fallback_model),
            upstream_pool: Arc::new(RwLock::new(upstream_pool)),
            upstream_profiles,
            default_upstream_profile_id: RwLock::new(default_upstream_profile_id),
            pipeline_globals: RwLock::new(pipeline_globals),
            legacy_api_key_as_client_auth,
            legacy_client_tokens,
            domain_policies: Arc::new(RwLock::new(HashMap::new())),
            domain_usage: Mutex::new(HashMap::new()),
            started_at: Instant::now(),
            backend_health: Arc::new(RwLock::new(backends)),
        })
    }

    pub fn effective_domain_label(domain: Option<&str>) -> &str {
        domain.unwrap_or("unclassified")
    }

    pub fn replace_domain_policies(&self, policies: HashMap<String, DomainPolicy>) {
        if let Ok(mut guard) = self.domain_policies.write() {
            *guard = policies;
        }
    }

    pub fn list_domain_policies(&self) -> Vec<(String, DomainPolicy)> {
        self.domain_policies
            .read()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    pub fn domain_within_quota(&self, domain: Option<&str>) -> bool {
        let label = Self::effective_domain_label(domain);
        let policy = match self.domain_policies.read() {
            Ok(guard) => guard.get(label).cloned(),
            Err(_) => return true,
        };
        let Some(policy) = policy else {
            return true;
        };
        if !policy.enabled {
            return true;
        }
        let usage = self
            .domain_usage
            .lock()
            .ok()
            .and_then(|m| m.get(label).map(|u| (u.tokens, u.spend_usd)))
            .unwrap_or((0, 0.0));
        let (tokens, spend_usd) = usage;
        if policy.monthly_token_budget > 0 && tokens >= policy.monthly_token_budget {
            return false;
        }
        if policy.monthly_cost_budget_usd > 0.0 && spend_usd >= policy.monthly_cost_budget_usd {
            return false;
        }
        true
    }

    pub fn record_domain_usage(&self, domain: Option<&str>, tokens: u64, spend_usd: f64) {
        if tokens == 0 && spend_usd <= 0.0 {
            return;
        }
        let label = Self::effective_domain_label(domain).to_string();
        if let Ok(mut guard) = self.domain_usage.lock() {
            let entry = guard.entry(label).or_default();
            entry.tokens = entry.tokens.saturating_add(tokens);
            entry.spend_usd += spend_usd;
        }
    }

    pub fn upstream_pool(&self) -> Arc<UpstreamKeyPool> {
        self.upstream_pool
            .read()
            .map(|p| Arc::clone(&p))
            .unwrap_or_else(|_| panic!("upstream_pool lock poisoned"))
    }

    pub fn replace_upstream_pool(&self, pool: Arc<UpstreamKeyPool>) {
        if let Ok(mut guard) = self.upstream_pool.write() {
            *guard = pool;
        }
    }

    pub fn is_legacy_client_token(&self, token: &str, full_auth: &str) -> bool {
        if !self.legacy_api_key_as_client_auth {
            return false;
        }
        self.legacy_client_tokens.contains(token)
            || self
                .legacy_client_tokens
                .iter()
                .any(|k| full_auth.ends_with(k))
    }

    pub fn stream_cache_enabled(&self) -> bool {
        self.stream_cache_enabled.load(Ordering::Relaxed)
    }

    pub fn set_stream_cache_enabled(&self, enabled: bool) {
        self.stream_cache_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn profile(&self, id: &str) -> Option<Arc<UpstreamProfileRuntime>> {
        self.upstream_profiles.get(id).cloned()
    }

    pub fn default_profile(&self) -> Arc<UpstreamProfileRuntime> {
        let id = self
            .default_upstream_profile_id
            .read()
            .map(|id| id.clone())
            .unwrap_or_else(|_| "deepseek".to_string());
        self.profile(&id)
            .or_else(|| self.upstream_profiles.values().next().cloned())
            .expect("at least one upstream profile required")
    }

    pub fn default_upstream_profile_id(&self) -> String {
        self.default_upstream_profile_id
            .read()
            .map(|id| id.clone())
            .unwrap_or_else(|_| "deepseek".to_string())
    }

    pub fn profile_descriptors(&self) -> Vec<crab_pipeline::ProfileDescriptor> {
        self.upstream_profiles
            .values()
            .map(|p| p.profile_descriptor())
            .collect()
    }

    pub fn pipeline_globals(&self) -> PipelineGlobals {
        self.pipeline_globals
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn set_pipeline_runtime(
        &self,
        pipeline_mode: PipelineMode,
        default_upstream_profile: &str,
    ) -> Result<(), &'static str> {
        if !self.upstream_profiles.contains_key(default_upstream_profile) {
            return Err("unknown upstream profile");
        }
        let mut globals = self
            .pipeline_globals
            .write()
            .map_err(|_| "pipeline globals lock poisoned")?;
        globals.pipeline_mode = pipeline_mode;
        globals.default_upstream_profile = default_upstream_profile.to_string();
        let mut ids: Vec<String> = self.upstream_profiles.keys().cloned().collect();
        ids.sort();
        globals.known_profile_ids = ids;
        if let Ok(mut id) = self.default_upstream_profile_id.write() {
            *id = default_upstream_profile.to_string();
        }
        Ok(())
    }

    pub fn set_cursor_models(&self, cursor_models: CursorModelsConfig) -> Result<(), String> {
        crab_pipeline::validate_cursor_models(&cursor_models)?;
        let mut globals = self
            .pipeline_globals
            .write()
            .map_err(|_| "pipeline globals lock poisoned".to_string())?;
        globals.cursor_models = cursor_models;
        Ok(())
    }

    pub fn cursor_models(&self) -> CursorModelsConfig {
        self.pipeline_globals()
            .cursor_models
            .clone()
    }
}
