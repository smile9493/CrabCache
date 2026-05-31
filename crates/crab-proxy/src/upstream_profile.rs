use crate::upstream_pool::UpstreamKeyPool;
use crab_pipeline::UpstreamProvider;
use crab_route::LbRouter;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct UpstreamProfileRuntime {
    pub id: String,
    pub provider: UpstreamProvider,
    pub base_url: String,
    pub fallback_model: String,
    pub tls_sni: String,
    pub router: LbRouter,
    /// Shared with `RuntimeConfig::upstream_pool` so Management hot-reload applies to outbound calls.
    pub upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
    /// Optional proxy for OAuth HTTP requests (socks5://, http://, etc.).
    pub proxy_url: Option<String>,
    /// Profile ID to try when this profile's upstream fails (5xx, 429, timeout).
    pub fallback_profile_id: Option<String>,
    /// Maximum number of fallback attempts per request (default 2).
    pub fallback_max_retries: u32,
}

/// Validate that a fallback chain has no cycles.
/// Returns `Ok(())` if no cycle, or `Err(profile_id)` if a cycle is detected.
pub fn validate_fallback_chain(
    profiles: &std::collections::HashMap<String, Option<String>>,
) -> Result<(), String> {
    use std::collections::HashSet;

    for start_id in profiles.keys() {
        let mut visited: HashSet<&str> = HashSet::new();
        let mut current: &str = start_id.as_str();
        loop {
            if !visited.insert(current) {
                return Err(current.to_string());
            }
            match profiles.get(current) {
                Some(Some(next)) => current = next.as_str(),
                _ => break,
            }
        }
    }
    Ok(())
}

impl UpstreamProfileRuntime {
    pub fn resolve_upstream_pool(&self) -> Arc<UpstreamKeyPool> {
        Arc::clone(&self.upstream_pool.read())
    }

    pub fn profile_descriptor(&self) -> crab_pipeline::ProfileDescriptor {
        crab_pipeline::ProfileDescriptor {
            id: self.id.clone(),
            provider: self.provider,
        }
    }
}
