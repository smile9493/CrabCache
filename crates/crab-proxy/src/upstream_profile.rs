use crate::upstream_pool::UpstreamKeyPool;
use crab_pipeline::UpstreamProvider;
use crab_route::AffinityRouter;
use std::sync::Arc;

pub struct UpstreamProfileRuntime {
    pub id: String,
    pub provider: UpstreamProvider,
    pub base_url: String,
    pub fallback_model: String,
    pub tls_sni: String,
    pub router: AffinityRouter,
    pub upstream_pool: Arc<UpstreamKeyPool>,
}

impl UpstreamProfileRuntime {
    pub fn profile_descriptor(&self) -> crab_pipeline::ProfileDescriptor {
        crab_pipeline::ProfileDescriptor {
            id: self.id.clone(),
            provider: self.provider,
        }
    }
}
