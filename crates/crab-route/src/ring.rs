use arc_swap::ArcSwap;
use pingora_core::protocols::l4::socket::SocketAddr as PSocketAddr;
use pingora_core::services::background::BackgroundService;
use pingora_core::server::ShutdownWatch;
use pingora_load_balancing::{
    discovery::Static, health_check::TcpHealthCheck, selection::Consistent, Backends, Backend as PBackend,
    LoadBalancer,
};
use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

/// Error type for routing operations.
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("backends list cannot be empty")]
    EmptyBackends,
}

/// Config-layer backend descriptor (used by config, management API, profile_build).
#[derive(Debug, Clone)]
pub struct Backend {
    pub name: String,
    pub addr: SocketAddr,
    pub weight: u32,
    pub tls_sni: String,
}

impl Backend {
    pub fn new(name: String, addr: SocketAddr, weight: u32, tls_sni: String) -> Self {
        Self {
            name,
            addr,
            weight,
            tls_sni,
        }
    }
}

/// Metadata stored alongside Pingora's `Backend` for TLS SNI and display name.
#[derive(Debug, Clone)]
pub struct BackendMeta {
    pub name: String,
    pub tls_sni: String,
}

/// The result of a backend selection via consistent-hash ring.
pub struct SelectedBackend<'a> {
    pub addr: SocketAddr,
    pub name: &'a str,
    pub tls_sni: &'a str,
}

/// Wrapper around Pingora's `LoadBalancer<Consistent>` with CrabCache metadata.
///
/// Replaces the former `AffinityRouter` + `BackendHealth` + `CircuitBreakerConfig`.
/// Health checking and circuit breaking are now delegated to Pingora's built-in
/// consecutive-threshold mechanism (`TcpHealthCheck`).
///
/// Uses `ArcSwap` for the inner `LoadBalancer` so that `rebuild()` can atomically
/// swap in a new backend set while the `LbHealthService` continues running with
/// the updated reference.
pub struct LbRouter {
    lb: Arc<ArcSwap<LoadBalancer<Consistent>>>,
    meta: HashMap<SocketAddr, BackendMeta>,
}

impl LbRouter {
    /// Create a new `LbRouter` from CrabCache config-layer backends.
    ///
    /// Registers a TCP health check with 30 s frequency and performs the initial
    /// discovery+build synchronously so the router is ready to serve immediately.
    pub fn new(backends: &[Backend]) -> Result<Self, RouteError> {
        if backends.is_empty() {
            return Err(RouteError::EmptyBackends);
        }

        let mut meta = HashMap::with_capacity(backends.len());
        let mut btree = BTreeSet::new();

        for b in backends {
            let p_addr = PSocketAddr::Inet(b.addr);
            let pb = PBackend {
                addr: p_addr,
                weight: b.weight as usize,
                ext: http::Extensions::new(),
            };
            btree.insert(pb);
            meta.insert(
                b.addr,
                BackendMeta {
                    name: b.name.clone(),
                    tls_sni: b.tls_sni.clone(),
                },
            );
        }

        let lb = Self::build_lb(btree)?;

        debug!(
            backend_count = backends.len(),
            "LbRouter initialized with Pingora LoadBalancer<Consistent>"
        );

        Ok(Self {
            lb: Arc::new(ArcSwap::from_pointee(lb)),
            meta,
        })
    }

    /// Build a `LoadBalancer` from a set of backends (shared construction logic).
    fn build_lb(btree: BTreeSet<PBackend>) -> Result<LoadBalancer<Consistent>, RouteError> {
        let discovery = Static::new(btree);
        let mut bts = Backends::new(discovery);
        bts.set_health_check(TcpHealthCheck::new());

        let mut lb = LoadBalancer::from_backends(bts);
        lb.health_check_frequency = Some(Duration::from_secs(30));

        // Run the initial update synchronously so the selector is built.
        // For Static discovery this completes instantly.
        futures::FutureExt::now_or_never(lb.update())
            .expect("static discovery future was not immediately ready; this is a Pingora regression")
            .map_err(|e| {
                tracing::error!(error = %e, "initial LB update failed");
                RouteError::EmptyBackends
            })?;

        Ok(lb)
    }

    /// Select a healthy backend for the given affinity key.
    ///
    /// Returns `None` only when every backend is unhealthy (all unhealthy) or
    /// the hash ring is empty.
    pub fn select(&self, key: &[u8]) -> Option<SelectedBackend<'_>> {
        let lb = self.lb.load();
        lb.select(key, 5).and_then(|pb| {
            let addr = match pb.addr {
                PSocketAddr::Inet(a) => a,
                _ => return None,
            };
            self.meta.get(&addr).map(|m| SelectedBackend {
                addr,
                name: &m.name,
                tls_sni: &m.tls_sni,
            })
        })
    }

    /// Access the underlying Pingora `Backends` (for management API health queries).
    ///
    /// Returns an `arc_swap::Guard` that derefs to `LoadBalancer<Consistent>`.
    /// Call `.backends()` on the result to get `&Backends` for health queries.
    pub fn backends(&self) -> arc_swap::Guard<Arc<LoadBalancer<Consistent>>> {
        self.lb.load()
    }

    /// Access the `ArcSwap` handle (for `LbHealthService` registration).
    ///
    /// The health service holds this reference and automatically picks up
    /// new `LoadBalancer` instances after `rebuild()`.
    pub fn lb_swap(&self) -> Arc<ArcSwap<LoadBalancer<Consistent>>> {
        Arc::clone(&self.lb)
    }

    /// Access the metadata map (for management API backend listing).
    pub fn meta(&self) -> &HashMap<SocketAddr, BackendMeta> {
        &self.meta
    }

    /// Rebuild the router with a new set of backends.
    ///
    /// Atomically swaps the inner `LoadBalancer` so that both the proxy's
    /// `select()` calls and the `LbHealthService` background loop immediately
    /// start using the new backend set.
    pub fn rebuild(&mut self, backends: &[Backend]) -> Result<(), RouteError> {
        if backends.is_empty() {
            return Err(RouteError::EmptyBackends);
        }

        let mut new_meta = HashMap::with_capacity(backends.len());
        let mut btree = BTreeSet::new();

        for b in backends {
            let p_addr = PSocketAddr::Inet(b.addr);
            let pb = PBackend {
                addr: p_addr,
                weight: b.weight as usize,
                ext: http::Extensions::new(),
            };
            btree.insert(pb);
            new_meta.insert(
                b.addr,
                BackendMeta {
                    name: b.name.clone(),
                    tls_sni: b.tls_sni.clone(),
                },
            );
        }

        let new_lb = Self::build_lb(btree)?;
        // Atomic swap — both proxy and health service immediately see the new LB.
        self.lb.store(Arc::new(new_lb));
        self.meta = new_meta;

        debug!(
            backend_count = backends.len(),
            "LbRouter rebuilt with new backend set"
        );

        Ok(())
    }

    /// Create a `BackgroundService` that runs the LB's health check loop.
    ///
    /// The health service holds a reference to the `ArcSwap`, so it automatically
    /// picks up new `LoadBalancer` instances after `rebuild()`.
    pub fn health_service(&self) -> LbHealthService {
        LbHealthService {
            lb_swap: Arc::clone(&self.lb),
        }
    }
}

/// Background service that runs Pingora health checks.
///
/// Holds an `Arc<ArcSwap<LoadBalancer>>` so it automatically picks up
/// new `LoadBalancer` instances after `LbRouter::rebuild()`.
///
/// Instead of delegating to `LoadBalancer::run()` (which would pin to a single
/// instance), this service manually drives health check cycles, loading the
/// latest `LoadBalancer` from the `ArcSwap` on each iteration.
pub struct LbHealthService {
    lb_swap: Arc<ArcSwap<LoadBalancer<Consistent>>>,
}

impl LbHealthService {
    /// Create a new LbHealthService from an ArcSwap handle.
    pub fn new(lb_swap: Arc<ArcSwap<LoadBalancer<Consistent>>>) -> Self {
        Self { lb_swap }
    }
}

#[async_trait::async_trait]
impl BackgroundService for LbHealthService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        let freq = {
            let lb = self.lb_swap.load();
            lb.health_check_frequency
                .unwrap_or(Duration::from_secs(30))
        };

        loop {
            // Load the latest LB from the ArcSwap (picks up rebuild() swaps).
            let lb = self.lb_swap.load_full();

            // Run one health check cycle on the current backend set.
            lb.backends().run_health_check(lb.parallel_health_check).await;

            tokio::select! {
                () = tokio::time::sleep(freq) => {}
                _ = shutdown.changed() => {
                    debug!("LbHealthService shutting down");
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn create_test_backends() -> Vec<Backend> {
        vec![
            Backend::new(
                "backend-1".to_string(),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 8080),
                1,
                "api.deepseek.com".to_string(),
            ),
            Backend::new(
                "backend-2".to_string(),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)), 8080),
                1,
                "api.deepseek.com".to_string(),
            ),
            Backend::new(
                "backend-3".to_string(),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 3)), 8080),
                1,
                "api.deepseek.com".to_string(),
            ),
        ]
    }

    #[test]
    fn test_router_creation() {
        let backends = create_test_backends();
        let router = LbRouter::new(&backends);
        assert!(router.is_ok());
    }

    #[test]
    fn test_router_empty_backends() {
        let router = LbRouter::new(&[]);
        assert!(router.is_err());
    }

    #[test]
    fn test_consistent_routing() {
        let backends = create_test_backends();
        let router = LbRouter::new(&backends).unwrap();

        let key = b"test-conversation-1";
        let b1 = router.select(key);
        let b2 = router.select(key);

        assert!(b1.is_some());
        assert_eq!(
            b1.map(|b| b.name),
            b2.map(|b| b.name),
        );
    }

    #[test]
    fn test_different_keys_distribute() {
        let backends = create_test_backends();
        let router = LbRouter::new(&backends).unwrap();

        let mut seen = std::collections::HashSet::new();
        for i in 0..100 {
            let key = format!("conversation-{i}");
            if let Some(b) = router.select(key.as_bytes()) {
                seen.insert(b.name.to_string());
            }
        }
        assert!(seen.len() > 1, "expected distribution across backends");
    }

    #[test]
    fn test_rebuild() {
        let backends = create_test_backends();
        let mut router = LbRouter::new(&backends).unwrap();

        let key = b"stable-key";
        let before = router.select(key).map(|b| b.name.to_string());

        let mut new_backends = create_test_backends();
        new_backends.push(Backend::new(
            "backend-4".to_string(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 4)), 8080),
            1,
            "api.deepseek.com".to_string(),
        ));
        router.rebuild(&new_backends).unwrap();

        let after = router.select(key);
        assert!(after.is_some());

        if let (Some(b), Some(before_name)) = (&after, &before) {
            let _ = (b.name, before_name);
        }
    }

    #[test]
    fn test_rebuild_empty_backends_errors() {
        let backends = create_test_backends();
        let mut router = LbRouter::new(&backends).unwrap();
        assert!(router.rebuild(&[]).is_err());
    }
}
