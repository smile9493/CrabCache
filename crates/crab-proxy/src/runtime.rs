use crate::context::ConnectionConfig;
use crate::stored_key::StoredKey;
use crate::upstream_pool::UpstreamKeyPool;
use crate::upstream_profile::UpstreamProfileRuntime;
use crab_cache::{FingerprintConfig, TtlConfig};
use crab_pipeline::{CursorModelsConfig, PipelineGlobals, PipelineMode};
use crab_route::LbRouter;
use dashmap::DashMap;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DomainUsage {
    pub tokens: u64,
    pub spend_usd: f64,
}

pub struct RuntimeConfig {
    pub keys: DashMap<String, StoredKey>,
    pub ttl: Arc<RwLock<TtlConfig>>,
    pub router: RwLock<LbRouter>,
    pub conn_config: RwLock<Arc<ConnectionConfig>>,
    pub stream_cache_enabled: AtomicBool,
    pub fingerprint: RwLock<Arc<FingerprintConfig>>,
    pub upstream_base_url: RwLock<String>,
    pub fallback_model: RwLock<String>,
    /// DeepSeek upstream API key pool (outbound Bearer).
    pub upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
    pub upstream_profiles: RwLock<IndexMap<String, Arc<UpstreamProfileRuntime>>>,
    pub default_upstream_profile_id: RwLock<String>,
    pub pipeline_globals: RwLock<PipelineGlobals>,
    /// When true, tokens in `legacy_client_tokens` may authenticate as clients.
    pub legacy_api_key_as_client_auth: bool,
    /// When true and the client key has no `project_id`, derive one from `sk-cc-*` (see `tenant::derive_project_id_from_client_key`).
    pub auto_project_id_from_client_key: bool,
    pub legacy_client_tokens: HashSet<String>,
    pub domain_policies: Arc<RwLock<IndexMap<String, DomainPolicy>>>,
    domain_usage: Mutex<HashMap<String, DomainUsage>>,
    pub started_at: Instant,
}

impl RuntimeConfig {
    pub fn new(
        router: LbRouter,
        ttl: Arc<RwLock<TtlConfig>>,
        conn_config: ConnectionConfig,
        stream_cache_enabled: bool,
        fingerprint: FingerprintConfig,
        upstream_base_url: String,
        fallback_model: String,
        upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
        upstream_profiles: IndexMap<String, Arc<UpstreamProfileRuntime>>,
        default_upstream_profile_id: String,
        pipeline_globals: PipelineGlobals,
        legacy_api_key_as_client_auth: bool,
        legacy_client_tokens: HashSet<String>,
        auto_project_id_from_client_key: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            keys: DashMap::new(),
            ttl,
            router: RwLock::new(router),
            conn_config: RwLock::new(Arc::new(conn_config)),
            stream_cache_enabled: AtomicBool::new(stream_cache_enabled),
            fingerprint: RwLock::new(Arc::new(fingerprint)),
            upstream_base_url: RwLock::new(upstream_base_url),
            fallback_model: RwLock::new(fallback_model),
            upstream_pool,
            upstream_profiles: RwLock::new(upstream_profiles),
            default_upstream_profile_id: RwLock::new(default_upstream_profile_id),
            pipeline_globals: RwLock::new(pipeline_globals),
            legacy_api_key_as_client_auth,
            auto_project_id_from_client_key,
            legacy_client_tokens,
            domain_policies: Arc::new(RwLock::new(IndexMap::new())),
            domain_usage: Mutex::new(HashMap::new()),
            started_at: Instant::now(),
        })
    }

    #[inline]
    pub fn effective_domain_label(domain: Option<&str>) -> &str {
        domain.unwrap_or("unclassified")
    }

    pub fn replace_domain_policies(&self, policies: IndexMap<String, DomainPolicy>) {
        *self.domain_policies.write() = policies;
    }

    pub fn list_domain_policies(&self) -> Vec<(String, DomainPolicy)> {
        self.domain_policies
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn domain_within_quota(&self, domain: Option<&str>) -> bool {
        let label = Self::effective_domain_label(domain);
        let policy = self.domain_policies.read().get(label).cloned();
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

    /// Return a snapshot of current per-domain usage counters.
    pub fn domain_usage_snapshot(&self) -> HashMap<String, DomainUsage> {
        self.domain_usage
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Replace all domain usage counters (used by Admin to restore after gateway restart).
    pub fn replace_domain_usage(&self, usage: HashMap<String, DomainUsage>) {
        if let Ok(mut guard) = self.domain_usage.lock() {
            *guard = usage;
        }
    }

    pub fn upstream_pool(&self) -> Arc<UpstreamKeyPool> {
        Arc::clone(&self.upstream_pool.read())
    }

    /// Key pool stats for the default upstream profile (same source as profile list / status).
    pub fn default_upstream_key_stats(&self) -> (usize, usize) {
        let pool = self.default_profile().resolve_upstream_pool();
        (pool.len(), pool.available_count())
    }

    pub fn replace_upstream_pool(&self, pool: Arc<UpstreamKeyPool>) {
        *self.upstream_pool.write() = pool;
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

    #[inline]
    pub fn stream_cache_enabled(&self) -> bool {
        self.stream_cache_enabled.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_stream_cache_enabled(&self, enabled: bool) {
        self.stream_cache_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn profile(&self, id: &str) -> Option<Arc<UpstreamProfileRuntime>> {
        self.upstream_profiles.read().get(id).cloned()
    }

    pub fn default_profile(&self) -> Arc<UpstreamProfileRuntime> {
        let id = self.default_upstream_profile_id.read().clone();
        self.profile(&id)
            .or_else(|| self.upstream_profiles.read().values().next().cloned())
            .expect("at least one upstream profile required")
    }

    pub fn default_upstream_profile_id(&self) -> String {
        self.default_upstream_profile_id.read().clone()
    }

    pub fn profile_descriptors(&self) -> Vec<crab_pipeline::ProfileDescriptor> {
        self.upstream_profiles
            .read()
            .values()
            .map(|p| p.profile_descriptor())
            .collect()
    }

    pub fn pipeline_globals(&self) -> PipelineGlobals {
        self.pipeline_globals.read().clone()
    }

    pub fn set_pipeline_runtime(
        &self,
        pipeline_mode: PipelineMode,
        default_upstream_profile: &str,
    ) -> Result<(), &'static str> {
        let profiles = self.upstream_profiles.read();
        if !profiles.contains_key(default_upstream_profile) {
            return Err("unknown upstream profile");
        }
        let mut globals = self.pipeline_globals.write();
        globals.pipeline_mode = pipeline_mode;
        globals.default_upstream_profile = default_upstream_profile.to_string();
        let mut ids: Vec<String> = profiles.keys().cloned().collect();
        ids.sort();
        globals.known_profile_ids = ids;
        *self.default_upstream_profile_id.write() = default_upstream_profile.to_string();
        Ok(())
    }

    pub fn set_cursor_models(&self, cursor_models: CursorModelsConfig) -> Result<(), String> {
        crab_pipeline::validate_cursor_models(&cursor_models)?;
        self.pipeline_globals.write().cursor_models = cursor_models;
        Ok(())
    }

    pub fn cursor_models(&self) -> CursorModelsConfig {
        self.pipeline_globals().cursor_models.clone()
    }

    fn refresh_known_profile_ids(&self) {
        let mut globals = self.pipeline_globals.write();
        let mut ids: Vec<String> = self.upstream_profiles.read().keys().cloned().collect();
        ids.sort();
        globals.known_profile_ids = ids;
    }

    /// Insert or replace a profile and refresh pipeline globals.
    pub fn upsert_profile(&self, profile: Arc<UpstreamProfileRuntime>) -> Result<(), String> {
        let id = profile.id.clone();
        {
            let mut profiles = self.upstream_profiles.write();
            profiles.insert(id.clone(), profile);
        }
        debug_assert!(
            self.upstream_profiles.read().contains_key(&id),
            "upsert_profile must make profile immediately readable"
        );
        self.refresh_known_profile_ids();
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
        let mut profiles = self.upstream_profiles.write();
        if profiles.len() <= 1 {
            return Err("cannot remove the only upstream profile".to_string());
        }
        if profiles.remove(id).is_none() {
            return Err("unknown upstream profile".to_string());
        }
        drop(profiles);
        self.refresh_known_profile_ids();
        Ok(())
    }

    pub fn replace_profile_pool(
        &self,
        profile_id: &str,
        pool: Arc<UpstreamKeyPool>,
    ) -> Result<(), String> {
        let profiles = self.upstream_profiles.read();
        let profile = profiles
            .get(profile_id)
            .ok_or_else(|| "unknown upstream profile".to_string())?
            .clone();
        drop(profiles);
        *profile.upstream_pool.write() = pool.clone();
        if profile_id == self.default_upstream_profile_id() {
            self.replace_upstream_pool(pool);
        }
        Ok(())
    }

    /// Copy default profile relay fields into legacy `RuntimeConfig` fields.
    pub fn sync_legacy_from_profile_id(&self, profile_id: &str) -> Result<(), String> {
        let profile = self
            .profile(profile_id)
            .ok_or_else(|| "unknown upstream profile".to_string())?;
        *self.upstream_base_url.write() = profile.base_url.clone();
        *self.fallback_model.write() = profile.fallback_model.clone();
        let backends: Vec<crab_route::Backend> = profile
            .router
            .meta()
            .iter()
            .map(|(addr, m)| crab_route::Backend::new(m.name.clone(), *addr, 1, m.tls_sni.clone()))
            .collect();
        self.router
            .write()
            .rebuild(&backends)
            .map_err(|e| e.to_string())?;
        let pool = profile.resolve_upstream_pool();
        self.replace_upstream_pool(pool);
        Ok(())
    }

    pub fn profile_endpoints(&self, profile_id: &str) -> Vec<String> {
        self.profile(profile_id)
            .map(|p| {
                p.router
                    .meta()
                    .keys()
                    .map(|addr| addr.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
}
