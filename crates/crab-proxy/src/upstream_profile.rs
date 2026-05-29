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
