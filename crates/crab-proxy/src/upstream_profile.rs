use crate::upstream_pool::UpstreamKeyPool;
use crab_pipeline::UpstreamProvider;
use crab_route::AffinityRouter;
use std::sync::{Arc, RwLock};

pub struct UpstreamProfileRuntime {
    pub id: String,
    pub provider: UpstreamProvider,
    pub base_url: String,
    pub fallback_model: String,
    pub tls_sni: String,
    pub router: AffinityRouter,
    /// Shared with `RuntimeConfig::upstream_pool` so Management hot-reload applies to outbound calls.
    pub upstream_pool: Arc<RwLock<Arc<UpstreamKeyPool>>>,
}

impl UpstreamProfileRuntime {
    pub fn resolve_upstream_pool(&self) -> Arc<UpstreamKeyPool> {
        self.upstream_pool
            .read()
            .map(|p| Arc::clone(&p))
            .unwrap_or_else(|_| panic!("upstream_pool lock poisoned"))
    }

    pub fn profile_descriptor(&self) -> crab_pipeline::ProfileDescriptor {
        crab_pipeline::ProfileDescriptor {
            id: self.id.clone(),
            provider: self.provider,
        }
    }
}
