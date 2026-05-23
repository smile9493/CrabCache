use pingora_ketama::{Bucket, Continuum};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::debug;

/// Error type for affinity routing operations.
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("backends list cannot be empty")]
    EmptyBackends,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_ms: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 2,
            timeout_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackendHealth {
    pub healthy: bool,
    pub last_check_ms: u64,
    pub latency_ms: u64,
    pub circuit_state: CircuitState,
    pub consecutive_failures: u32,
    pub half_open_successes: u32,
    pub circuit_opened_at_ms: u64,
}

impl BackendHealth {
    pub fn new_healthy() -> Self {
        Self {
            healthy: true,
            last_check_ms: 0,
            latency_ms: 0,
            circuit_state: CircuitState::Closed,
            consecutive_failures: 0,
            half_open_successes: 0,
            circuit_opened_at_ms: 0,
        }
    }

    pub fn new_unhealthy() -> Self {
        Self {
            healthy: false,
            last_check_ms: 0,
            latency_ms: 0,
            circuit_state: CircuitState::Closed,
            consecutive_failures: 0,
            half_open_successes: 0,
            circuit_opened_at_ms: 0,
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn record_success(&mut self, config: &CircuitBreakerConfig) {
        self.consecutive_failures = 0;
        self.latency_ms = Self::now_ms();
        match self.circuit_state {
            CircuitState::HalfOpen => {
                self.half_open_successes += 1;
                if self.half_open_successes >= config.success_threshold {
                    self.circuit_state = CircuitState::Closed;
                    self.circuit_opened_at_ms = 0;
                    self.half_open_successes = 0;
                }
            }
            CircuitState::Closed | CircuitState::Open => {}
        }
    }

    pub fn record_failure(&mut self, config: &CircuitBreakerConfig) {
        self.consecutive_failures += 1;
        match self.circuit_state {
            CircuitState::Closed => {
                if self.consecutive_failures >= config.failure_threshold {
                    self.circuit_state = CircuitState::Open;
                    self.healthy = false;
                    self.circuit_opened_at_ms = Self::now_ms();
                }
            }
            CircuitState::HalfOpen => {
                self.circuit_state = CircuitState::Open;
                self.healthy = false;
                self.circuit_opened_at_ms = Self::now_ms();
            }
            CircuitState::Open => {}
        }
    }

    pub fn check_open_circuit(&mut self, config: &CircuitBreakerConfig) {
        if self.circuit_state == CircuitState::Open
            && self.circuit_opened_at_ms > 0
            && Self::now_ms().saturating_sub(self.circuit_opened_at_ms) >= config.timeout_ms
        {
            self.circuit_state = CircuitState::HalfOpen;
            self.healthy = true;
            self.half_open_successes = 0;
        }
    }
}

pub struct AffinityRouter {
    continuum: Continuum,
    backends: Vec<Arc<Backend>>,
}

impl AffinityRouter {
    pub fn new(backends: &[Backend]) -> Result<Self, RouteError> {
        if backends.is_empty() {
            return Err(RouteError::EmptyBackends);
        }

        let buckets: Vec<Bucket> = backends
            .iter()
            .map(|b| Bucket::new(b.addr, b.weight))
            .collect();

        let continuum = Continuum::new(&buckets);

        let backends: Vec<Arc<Backend>> = backends.iter().map(|b| Arc::new(b.clone())).collect();

        debug!(backend_count = backends.len(), "AffinityRouter initialized");

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

    /// Select a backend using the consistent hash ring, filtering by health.
    ///
    /// Iterates through backends in hash-ring order until a healthy one is found.
    /// If no backends are healthy, falls back to all backends with a warning.
    pub fn select_healthy<F>(&self, key: &[u8], is_healthy: F) -> Option<&Backend>
    where
        F: Fn(&str) -> bool,
    {
        let healthy_count = self.backends.iter().filter(|b| is_healthy(&b.name)).count();
        let total = self.backends.len();

        let addr = self.continuum.node(key)?;

        if let Some(selected) = self.backends.iter().find(|b| b.addr == addr) {
            if is_healthy(&selected.name) {
                return Some(selected.as_ref());
            }
        }

        if healthy_count > 0 {
            for b in &self.backends {
                if is_healthy(&b.name) {
                    return Some(b.as_ref());
                }
            }
        }

        tracing::warn!(
            healthy = healthy_count,
            total = total,
            "All backends unhealthy, falling back to original selection"
        );
        self.backends
            .iter()
            .find(|b| b.addr == addr)
            .map(|b| b.as_ref())
    }

    pub fn update(&mut self, backends: &[Backend]) -> Result<(), RouteError> {
        if backends.is_empty() {
            return Err(RouteError::EmptyBackends);
        }

        let buckets: Vec<Bucket> = backends
            .iter()
            .map(|b| Bucket::new(b.addr, b.weight))
            .collect();

        self.continuum = Continuum::new(&buckets);
        self.backends = backends.iter().map(|b| Arc::new(b.clone())).collect();

        debug!(backend_count = backends.len(), "AffinityRouter updated");

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
            let key = format!("conversation-{i}");
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
            let key = format!("conversation-{i}");
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
        assert!(drift_rate < 0.4, "Drift rate {drift_rate} exceeds 40%");
    }

    #[test]
    fn circuit_opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 1,
            timeout_ms: 30_000,
        };
        let mut health = BackendHealth::new_healthy();
        health.record_failure(&config);
        health.record_failure(&config);
        assert!(health.healthy);
        health.record_failure(&config);
        assert!(!health.healthy);
        assert_eq!(health.circuit_state, CircuitState::Open);
    }
}
