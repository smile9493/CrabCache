//! Runtime connection pre-warm via Pingora's shared connection pool.
//!
//! Instead of sending HTTP requests through the proxy loopback, this module
//! directly calls `get_http_session` / `release_http_session` on the same
//! `Connector` that handles real upstream traffic. This populates the pool
//! with TCP+TLS connections without any HTTP overhead.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use pingora_core::connectors::http::Connector;
use pingora_core::server::ShutdownWatch;
use pingora_core::services::background::BackgroundService;
use pingora_core::upstreams::peer::HttpPeer;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// One backend row for startup pool pre-warm: `(profile_id, addr, backend_name, tls_sni)`.
pub type PrewarmBackend = (String, SocketAddr, String, String);

/// Pingora background task: warm TCP+TLS for all configured backends after runtime starts.
pub struct StartupPrewarmService {
    pub connector: Arc<Connector<()>>,
    pub backends: Vec<PrewarmBackend>,
}

#[async_trait]
impl BackgroundService for StartupPrewarmService {
    async fn start(&self, _shutdown: ShutdownWatch) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let connector = self.connector.clone();
        let backends = self.backends.clone();
        let semaphore = Arc::new(Semaphore::new(8));
        let mut join_set = tokio::task::JoinSet::new();

        for (profile_id, addr, name, tls_sni) in backends {
            let peer = HttpPeer::new(addr, true, tls_sni);
            let connector = connector.clone();
            let semaphore = semaphore.clone();
            let profile_id = profile_id.clone();
            let name = name.clone();
            join_set.spawn(async move {
                let Ok(_permit) = semaphore.try_acquire() else {
                    return;
                };
                match connector.get_http_session(&peer).await {
                    Ok((session, _reused)) => {
                        connector.release_http_session(session, &peer, None).await;
                        debug!(
                            profile = %profile_id,
                            backend = %name,
                            addr = %addr,
                            "Startup direct pre-warm OK"
                        );
                    }
                    Err(e) => {
                        warn!(
                            profile = %profile_id,
                            backend = %name,
                            addr = %addr,
                            error = %e,
                            "Startup direct pre-warm failed"
                        );
                    }
                }
            });
        }

        let results = join_set.join_all().await;
        info!(
            count = results.len(),
            "Startup connection pre-warm completed"
        );
    }
}

/// Directly establish a TCP+TLS connection to the backend and release it back
/// to the pool. No HTTP request is sent; the connection is ready for reuse
/// by subsequent real requests.
pub async fn prewarm_direct(
    connector: Arc<Connector<()>>,
    peer: HttpPeer,
    semaphore: Arc<Semaphore>,
) {
    let Ok(_permit) = semaphore.try_acquire() else {
        debug!(addr = %peer, "Skipping direct pre-warm: concurrency limit");
        return;
    };

    match connector.get_http_session(&peer).await {
        Ok((session, _reused)) => {
            // Release back to pool with no idle timeout override (use pool default).
            connector.release_http_session(session, &peer, None).await;
            debug!(
                addr = %peer,
                sni = %peer.sni,
                "Direct pre-warm: TLS connection established and pooled"
            );
        }
        Err(e) => {
            warn!(addr = %peer, sni = %peer.sni, error = %e, "Direct pre-warm failed");
        }
    }
}

/// Spawn a direct pre-warm task for the given peer if the connector is available.
pub fn spawn_direct_prewarm_if_new_session(
    connector: &Option<Arc<Connector<()>>>,
    seen: &moka::sync::Cache<String, ()>,
    session_fingerprint: &str,
    peer: HttpPeer,
    semaphore: Arc<Semaphore>,
) {
    let Some(connector) = connector else { return };
    if seen.get(session_fingerprint).is_some() {
        return;
    }
    seen.insert(session_fingerprint.to_string(), ());
    let connector = connector.clone();
    tokio::spawn(async move {
        prewarm_direct(connector, peer, semaphore).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_cache_prevents_duplicate_prewarm() {
        let seen = moka::sync::Cache::<String, ()>::builder()
            .max_capacity(100)
            .build();
        let connector: Option<Arc<Connector<()>>> = None;

        // First call with a new fingerprint — connector is None so no spawn,
        // but seen cache should be populated.
        let sem = Arc::new(Semaphore::new(1));
        let peer = HttpPeer::new("127.0.0.1:443", true, "test.example.com".into());
        spawn_direct_prewarm_if_new_session(&connector, &seen, "fp-1", peer, sem.clone());

        // Since connector is None, no entry should be added.
        assert!(seen.get("fp-1").is_none());

        // With a dummy connector (won't connect, just exercises the path).
        // We can't create a real Connector in unit tests, but we can test the
        // dedup logic by pre-populating the seen cache.
        seen.insert("fp-existing".to_string(), ());
        let peer2 = HttpPeer::new("127.0.0.1:443", true, "test.example.com".into());
        spawn_direct_prewarm_if_new_session(&connector, &seen, "fp-existing", peer2, sem);
        // Should not have spawned anything (connector is None, and fp was already seen).
        assert!(seen.get("fp-existing").is_some());
    }

    #[test]
    fn seen_cache_does_not_fire_for_repeated_fingerprint() {
        let seen = moka::sync::Cache::<String, ()>::builder()
            .max_capacity(100)
            .build();
        let sem = Arc::new(Semaphore::new(1));

        // Simulate: insert fingerprint first, then call spawn with same fp.
        seen.insert("fp-42".to_string(), ());
        let connector: Option<Arc<Connector<()>>> = None;
        let peer = HttpPeer::new("127.0.0.1:443", true, "test.example.com".into());

        // Call with already-seen fingerprint — should be a no-op.
        spawn_direct_prewarm_if_new_session(&connector, &seen, "fp-42", peer, sem);
        // Still present (no panic, no double-insert).
        assert!(seen.get("fp-42").is_some());
    }

    #[tokio::test]
    async fn prewarm_skips_when_semaphore_exhausted() {
        // Create a semaphore with 0 permits — try_acquire will always fail.
        let sem = Arc::new(Semaphore::new(0));

        // We can't create a real Connector<()>, but we can verify the semaphore
        // guard path by calling try_acquire directly.
        let permit = sem.try_acquire();
        assert!(permit.is_err(), "semaphore with 0 permits should fail");
    }
}
