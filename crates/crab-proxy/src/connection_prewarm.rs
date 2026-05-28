//! Runtime connection pre-warm via Pingora's shared connection pool.
//!
//! Instead of sending HTTP requests through the proxy loopback, this module
//! directly calls `get_http_session` / `release_http_session` on the same
//! `Connector` that handles real upstream traffic. This populates the pool
//! with TCP+TLS connections without any HTTP overhead.

use std::sync::Arc;

use pingora_core::connectors::http::Connector;
use pingora_core::upstreams::peer::HttpPeer;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

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
            connector
                .release_http_session(session, &peer, None)
                .await;
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
