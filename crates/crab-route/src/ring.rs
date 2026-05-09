use anyhow::Result;
use pingora_ketama::{Bucket, Continuum};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::debug;

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

pub struct AffinityRouter {
    continuum: Continuum,
    backends: Vec<Arc<Backend>>,
}

impl AffinityRouter {
    pub fn new(backends: &[Backend]) -> Result<Self> {
        if backends.is_empty() {
            anyhow::bail!("Backends list cannot be empty");
        }

        let buckets: Vec<Bucket> = backends
            .iter()
            .map(|b| Bucket::new(b.addr, b.weight))
            .collect();

        let continuum = Continuum::new(&buckets);

        let backends: Vec<Arc<Backend>> = backends.iter().map(|b| Arc::new(b.clone())).collect();

        debug!(
            backend_count = backends.len(),
            "AffinityRouter initialized"
        );

        Ok(Self {
            continuum,
            backends,
        })
    }

    pub fn select(&self, key: &[u8]) -> Option<&Backend> {
        let addr = self.continuum.node(key)?;

        self.backends
            .iter()
            .find(|b| b.addr == addr)
            .map(|b| b.as_ref())
    }

    pub fn update(&mut self, backends: &[Backend]) -> Result<()> {
        if backends.is_empty() {
            anyhow::bail!("Backends list cannot be empty");
        }

        let buckets: Vec<Bucket> = backends
            .iter()
            .map(|b| Bucket::new(b.addr, b.weight))
            .collect();

        self.continuum = Continuum::new(&buckets);
        self.backends = backends.iter().map(|b| Arc::new(b.clone())).collect();

        debug!(
            backend_count = backends.len(),
            "AffinityRouter updated"
        );

        Ok(())
    }

    pub fn backends(&self) -> &[Arc<Backend>] {
        &self.backends
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
        let router = AffinityRouter::new(&backends);
        assert!(router.is_ok());
    }

    #[test]
    fn test_router_empty_backends() {
        let router = AffinityRouter::new(&[]);
        assert!(router.is_err());
    }

    #[test]
    fn test_consistent_routing() {
        let backends = create_test_backends();
        let router = AffinityRouter::new(&backends).unwrap();

        let key1 = b"test-conversation-1";
        let key2 = b"test-conversation-1";

        let backend1 = router.select(key1);
        let backend2 = router.select(key2);

        assert_eq!(backend1.map(|b| &b.name), backend2.map(|b| &b.name));
    }

    #[test]
    fn test_different_keys_different_backends() {
        let backends = create_test_backends();
        let router = AffinityRouter::new(&backends).unwrap();

        let mut backend_counts = std::collections::HashMap::new();

        for i in 0..100 {
            let key = format!("conversation-{}", i);
            if let Some(backend) = router.select(key.as_bytes()) {
                *backend_counts.entry(backend.name.clone()).or_insert(0) += 1;
            }
        }

        assert!(backend_counts.len() > 1);
    }

    #[test]
    fn test_drift_rate_on_add() {
        let backends = create_test_backends();
        let router = AffinityRouter::new(&backends).unwrap();

        let mut original_mapping = std::collections::HashMap::new();
        for i in 0..100 {
            let key = format!("conversation-{}", i);
            if let Some(backend) = router.select(key.as_bytes()) {
                original_mapping.insert(key, backend.name.clone());
            }
        }

        let mut new_backends = create_test_backends();
        new_backends.push(Backend::new(
            "backend-4".to_string(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 4)), 8080),
            1,
            "api.deepseek.com".to_string(),
        ));

        let mut router = router;
        router.update(&new_backends).unwrap();

        let mut changed = 0;
        for (key, original_backend) in &original_mapping {
            if let Some(backend) = router.select(key.as_bytes()) {
                if backend.name != *original_backend {
                    changed += 1;
                }
            }
        }

        let drift_rate = changed as f64 / original_mapping.len() as f64;
        assert!(
            drift_rate < 0.4,
            "Drift rate {} exceeds 40%",
            drift_rate
        );
    }
}
