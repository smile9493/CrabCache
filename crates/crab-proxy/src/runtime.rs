use crate::context::ConnectionConfig;
use crate::stored_key::StoredKey;
use crate::upstream_pool::UpstreamKeyPool;
use crate::upstream_profile::UpstreamProfileRuntime;
use crab_cache::{FingerprintConfig, TtlConfig};
use crab_pipeline::{CursorModelsConfig, PipelineGlobals, PipelineMode};
use crab_route::{AffinityRouter, BackendHealth, CircuitBreakerConfig};
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
    pub conn_config: RwLock<Arc<ConnectionConfig>>,
    pub stream_cache_enabled: AtomicBool,
    pub fingerprint: RwLock<FingerprintConfig>,
    pub upstream_base_url: RwLock<String>,
    pub fallback_model: RwLock<String>,
    /// DeepSeek upstream API key pool (outbound Bearer).
    pub upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
    pub upstream_profiles: RwLock<HashMap<String, Arc<UpstreamProfileRuntime>>>,
    pub default_upstream_profile_id: RwLock<String>,
    pub pipeline_globals: RwLock<PipelineGlobals>,
    /// When true, tokens in `legacy_client_tokens` may authenticate as clients.
    pub legacy_api_key_as_client_auth: bool,
    pub legacy_client_tokens: HashSet<String>,
    pub domain_policies: Arc<RwLock<HashMap<String, DomainPolicy>>>,
    domain_usage: Mutex<HashMap<String, DomainUsage>>,
    pub started_at: Instant,
    pub backend_health: Arc<RwLock<std::collections::HashMap<String, BackendHealth>>>,
    pub circuit_breaker_config: CircuitBreakerConfig,
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
        upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
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
            conn_config: RwLock::new(Arc::new(conn_config)),
            stream_cache_enabled: AtomicBool::new(stream_cache_enabled),
            fingerprint: RwLock::new(fingerprint),
            upstream_base_url: RwLock::new(upstream_base_url),
            fallback_model: RwLock::new(fallback_model),
            upstream_pool,
            upstream_profiles: RwLock::new(upstream_profiles),
            default_upstream_profile_id: RwLock::new(default_upstream_profile_id),
            pipeline_globals: RwLock::new(pipeline_globals),
            legacy_api_key_as_client_auth,
            legacy_client_tokens,
            domain_policies: Arc::new(RwLock::new(HashMap::new())),
            domain_usage: Mutex::new(HashMap::new()),
            started_at: Instant::now(),
            backend_health: Arc::new(RwLock::new(backends)),
            circuit_breaker_config: CircuitBreakerConfig::default(),
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
        self.upstream_profiles
            .read()
            .ok()
            .and_then(|profiles| profiles.get(id).cloned())
    }

    pub fn default_profile(&self) -> Arc<UpstreamProfileRuntime> {
        let id = self
            .default_upstream_profile_id
            .read()
            .map(|id| id.clone())
            .unwrap_or_else(|_| "deepseek".to_string());
        self.profile(&id)
            .or_else(|| {
                self.upstream_profiles
                    .read()
                    .ok()
                    .and_then(|profiles| profiles.values().next().cloned())
            })
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
            .read()
            .map(|profiles| {
                profiles
                    .values()
                    .map(|p| p.profile_descriptor())
                    .collect()
            })
            .unwrap_or_default()
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
        let profiles = self
            .upstream_profiles
            .read()
            .map_err(|_| "upstream profiles lock poisoned")?;
        if !profiles.contains_key(default_upstream_profile) {
            return Err("unknown upstream profile");
        }
        let mut globals = self
            .pipeline_globals
            .write()
            .map_err(|_| "pipeline globals lock poisoned")?;
        globals.pipeline_mode = pipeline_mode;
        globals.default_upstream_profile = default_upstream_profile.to_string();
        let mut ids: Vec<String> = profiles.keys().cloned().collect();
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

    fn refresh_known_profile_ids(&self) -> Result<(), &'static str> {
        let mut globals = self
            .pipeline_globals
            .write()
            .map_err(|_| "pipeline globals lock poisoned")?;
        let mut ids: Vec<String> = self
            .upstream_profiles
            .read()
            .map_err(|_| "upstream profiles lock poisoned")?
            .keys()
            .cloned()
            .collect();
        ids.sort();
        globals.known_profile_ids = ids;
        Ok(())
    }

    /// Insert or replace a profile and refresh pipeline globals.
    pub fn upsert_profile(&self, profile: Arc<UpstreamProfileRuntime>) -> Result<(), String> {
        let id = profile.id.clone();
        {
            let mut profiles = self
                .upstream_profiles
                .write()
                .map_err(|_| "upstream profiles lock poisoned".to_string())?;
            profiles.insert(id.clone(), profile);
        }
        self.refresh_known_profile_ids()
            .map_err(|e| e.to_string())?;
        if id == self.default_upstream_profile_id() {
            self.sync_legacy_from_profile_id(&id)?;
        }
        Ok(())
    }

    /// Remove a profile. Fails if default or last profile.
    pub fn remove_profile(&self, id: &str) -> Result<(), String> {
        let default_id = self.default_upstream_profile_id();
        if id == default_id {
            return Err("cannot remove default upstream profile".to_string());
        }
        let mut profiles = self
            .upstream_profiles
            .write()
            .map_err(|_| "upstream profiles lock poisoned".to_string())?;
        if profiles.len() <= 1 {
            return Err("cannot remove the only upstream profile".to_string());
        }
        if profiles.remove(id).is_none() {
            return Err("unknown upstream profile".to_string());
        }
        drop(profiles);
        self.refresh_known_profile_ids()
            .map_err(|e| e.to_string())
    }

    pub fn replace_profile_pool(
        &self,
        profile_id: &str,
        pool: Arc<UpstreamKeyPool>,
    ) -> Result<(), String> {
        let profiles = self
            .upstream_profiles
            .read()
            .map_err(|_| "upstream profiles lock poisoned".to_string())?;
        let profile = profiles
            .get(profile_id)
            .ok_or_else(|| "unknown upstream profile".to_string())?
            .clone();
        drop(profiles);
        if let Ok(mut guard) = profile.upstream_pool.write() {
            *guard = pool;
        }
        if profile_id == self.default_upstream_profile_id() {
            self.replace_upstream_pool(
                profile
                    .upstream_pool
                    .read()
                    .map_err(|_| "upstream_pool lock poisoned".to_string())?
                    .clone(),
            );
        }
        Ok(())
    }

    /// Copy default profile relay fields into legacy `RuntimeConfig` fields.
    pub fn sync_legacy_from_profile_id(&self, profile_id: &str) -> Result<(), String> {
        let profile = self
            .profile(profile_id)
            .ok_or_else(|| "unknown upstream profile".to_string())?;
        if let Ok(mut base) = self.upstream_base_url.write() {
            *base = profile.base_url.clone();
        }
        if let Ok(mut model) = self.fallback_model.write() {
            *model = profile.fallback_model.clone();
        }
        let backends: Vec<crab_route::Backend> = profile
            .router
            .backends()
            .iter()
            .map(|b| (**b).clone())
            .collect();
        if let Ok(mut router) = self.router.write() {
            router
                .update(&backends)
                .map_err(|e| e.to_string())?;
        }
        let pool = profile.resolve_upstream_pool();
        self.replace_upstream_pool(pool);
        if let Ok(mut health) = self.backend_health.write() {
            let keep: std::collections::HashSet<String> =
                backends.iter().map(|b| b.name.clone()).collect();
            health.retain(|name, _| keep.contains(name));
            for b in &backends {
                health
                    .entry(b.name.clone())
                    .or_insert_with(crab_route::BackendHealth::new_healthy);
            }
        }
        Ok(())
    }

    pub fn profile_endpoints(&self, profile_id: &str) -> Vec<String> {
        self.profile(profile_id)
            .map(|p| {
                p.router
                    .backends()
                    .iter()
                    .map(|b| b.addr.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
}
