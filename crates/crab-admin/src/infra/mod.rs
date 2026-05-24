pub mod collector;
pub mod docker;
pub mod history;
pub mod host;
pub mod rates;
pub mod speed_test;
pub mod types;

use crate::infra::types::*;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use parking_lot::RwLock;

/// Collect an infrastructure snapshot (containers + host disks + volumes).
pub async fn collect_snapshot(
    docker: &Option<bollard::Docker>,
    compose_project: &str,
    prev: &RwLock<HashMap<String, ContainerRawSample>>,
) -> InfraSnapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut collection_error: Option<String> = None;
    let (containers, new_prev) = if let Some(d) = docker {
        let prev_map = prev.read().clone();
        match docker::collect_snapshot(d, compose_project, &prev_map).await {
            Ok((containers, new_prev)) => (containers, new_prev),
            Err(e) => {
                tracing::warn!(error = %e, "Infra snapshot collection failed");
                collection_error = Some(e);
                (vec![], prev.read().clone())
            }
        }
    } else {
        (vec![], HashMap::new())
    };

    *prev.write() = new_prev;

    let host_disks = host::collect_host_disks();

    // Collect volume disk usage
    let volumes = if let Some(d) = docker {
        match docker::collect_volumes(d, compose_project).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Volume collection failed");
                collection_error = Some(e);
                vec![]
            }
        }
    } else {
        vec![]
    };

    if docker.is_some() && containers.is_empty() && collection_error.is_none() {
        collection_error = Some(format!(
            "no running containers with label com.docker.compose.project={compose_project}"
        ));
    }

    InfraSnapshot {
        containers,
        host_disks,
        volumes,
        collected_at: now,
        compose_project: compose_project.to_string(),
        docker_connected: docker.is_some(),
        collection_error,
    }
}

/// Resolve compose project name from environment, falling back to default.
pub fn resolve_compose_project() -> String {
    std::env::var("CRABCACHE_COMPOSE_PROJECT")
        .or_else(|_| std::env::var("COMPOSE_PROJECT_NAME"))
        .unwrap_or_else(|_| "crabcache".to_string())
}

/// Resolve Docker host URI.
pub fn resolve_docker_host() -> String {
    std::env::var("CRABCACHE_DOCKER_HOST")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string())
}

/// Resolve infra cache TTL from env.
pub fn resolve_cache_ttl_secs() -> u64 {
    std::env::var("CRABCACHE_INFRA_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

/// In-memory cache for infra snapshots (written only by the background collector).
pub struct InfraCache {
    pub snapshot: RwLock<Option<(InfraSnapshot, u64)>>,
}

impl InfraCache {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(None),
        }
    }

    pub fn store(&self, snapshot: InfraSnapshot) {
        let collected_at = snapshot.collected_at;
        *self.snapshot.write() = Some((snapshot, collected_at));
    }
}
