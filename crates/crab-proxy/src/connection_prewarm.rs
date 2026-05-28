//! Runtime connection pre-warm via the proxy loopback (same path as startup warmup in `crab-gateway`).

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tracing::debug;

/// Shared HTTP client for loopback pre-warm (lazy, one per process).
fn prewarm_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default()
    })
}

/// Send `GET /v1/models` through the proxy listener so Pingora's pool is exercised for the affinity key.
pub async fn prewarm_via_proxy(
    semaphore: Arc<Semaphore>,
    loopback_addr: &str,
    api_key: &str,
    affinity_key: Option<&str>,
) {
    let Ok(_permit) = semaphore.try_acquire() else {
        debug!(addr = %loopback_addr, "Skipping runtime pre-warm: concurrency limit");
        return;
    };

    let url = format!("http://{loopback_addr}/v1/models");
    let mut req = prewarm_client()
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"));
    if let Some(key) = affinity_key.filter(|k| !k.is_empty()) {
        req = req.header("x-request-affinity", key);
    }

    match req.send().await {
        Ok(resp) => {
            debug!(
                addr = %loopback_addr,
                status = resp.status().as_u16(),
                "Runtime connection pre-warm completed"
            );
        }
        Err(e) => {
            debug!(addr = %loopback_addr, error = %e, "Runtime connection pre-warm failed");
        }
    }
}
