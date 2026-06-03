//! Webhook storage, configuration, and delivery logic.
//!
//! Provides `WebhookStore` for managing webhook configurations and
//! `WebhookDelivery` for async HTTP delivery with HMAC signing and retry.

use crab_proxy::event_bus::GatewayEvent;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Configuration for a single webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Unique webhook identifier.
    pub id: String,
    /// Target URL to receive webhook POST requests.
    pub url: String,
    /// Secret key for HMAC-SHA256 signature verification.
    pub secret: String,
    /// Event types this webhook subscribes to (e.g., ["RequestCompleted"]).
    /// Empty vec means all events.
    pub events: Vec<String>,
    /// Whether this webhook is enabled.
    pub enabled: bool,
    /// Creation timestamp (seconds since Unix epoch).
    pub created_at: u64,
    /// Last successful trigger timestamp.
    pub last_triggered: Option<u64>,
    /// Consecutive failure count (reset on success).
    pub failure_count: u32,
}

/// Request body for registering a new webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterWebhookRequest {
    /// Target URL to receive webhook POST requests.
    pub url: String,
    /// Optional secret for HMAC-SHA256 signing. Auto-generated if omitted.
    pub secret: Option<String>,
    /// Event types to subscribe to. Empty means all events.
    pub events: Option<Vec<String>>,
    /// Whether the webhook is enabled. Defaults to true.
    pub enabled: Option<bool>,
}

/// Response from webhook test delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTestResult {
    /// Whether the test delivery succeeded.
    pub success: bool,
    /// HTTP status code from the test delivery.
    pub status: Option<u16>,
    /// Error message if delivery failed.
    pub error: Option<String>,
}

/// Thread-safe store for webhook configurations.
///
/// Uses `parking_lot::RwLock` for concurrent read access and exclusive write access.
/// Supports dynamic registration and unregistration of webhooks.
#[derive(Clone)]
pub struct WebhookStore {
    webhooks: Arc<RwLock<HashMap<String, WebhookConfig>>>,
}

impl WebhookStore {
    /// Create a new empty webhook store.
    pub fn new() -> Self {
        Self {
            webhooks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new webhook configuration.
    pub fn register(&self, config: WebhookConfig) {
        let id = config.id.clone();
        self.webhooks.write().insert(id.clone(), config);
        debug!(webhook_id = %id, "Webhook registered");
    }

    /// Unregister a webhook by ID.
    /// Returns `true` if the webhook was found and removed.
    pub fn unregister(&self, id: &str) -> bool {
        let removed = self.webhooks.write().remove(id).is_some();
        if removed {
            debug!(webhook_id = %id, "Webhook unregistered");
        }
        removed
    }

    /// Get all registered webhooks.
    pub fn list_all(&self) -> Vec<WebhookConfig> {
        self.webhooks.read().values().cloned().collect()
    }

    /// Get a specific webhook by ID.
    pub fn get(&self, id: &str) -> Option<WebhookConfig> {
        self.webhooks.read().get(id).cloned()
    }

    /// Get all active webhooks that subscribe to the given event type.
    ///
    /// A webhook is considered active if:
    /// 1. `enabled` is `true`
    /// 2. Either `events` is empty (subscribes to all) or contains the event type
    pub fn get_active_webhooks(&self, event_type: &str) -> Vec<WebhookConfig> {
        self.webhooks
            .read()
            .values()
            .filter(|w| {
                w.enabled && (w.events.is_empty() || w.events.iter().any(|e| e == event_type))
            })
            .cloned()
            .collect()
    }

    /// Record a trigger attempt for a webhook.
    /// Updates `last_triggered` on success, increments `failure_count` on failure.
    pub fn record_trigger(&self, id: &str, success: bool) {
        let mut webhooks = self.webhooks.write();
        if let Some(webhook) = webhooks.get_mut(id) {
            if success {
                webhook.last_triggered = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
                webhook.failure_count = 0;
            } else {
                webhook.failure_count += 1;
            }
        }
    }

    /// Get the count of registered webhooks.
    pub fn count(&self) -> usize {
        self.webhooks.read().len()
    }
}

impl Default for WebhookStore {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP client for webhook delivery with HMAC signing and retry logic.
pub struct WebhookDelivery {
    store: WebhookStore,
    client: reqwest::Client,
    max_retries: u32,
    retry_base_delay_ms: u64,
}

impl WebhookDelivery {
    /// Create a new webhook delivery handler.
    pub fn new(store: WebhookStore, max_retries: u32, retry_base_delay_ms: u64) -> Self {
        Self {
            store,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            max_retries,
            retry_base_delay_ms,
        }
    }

    /// Start listening for events and delivering webhooks.
    ///
    /// This function runs indefinitely until the broadcast channel is closed.
    /// It spawns a new task for each webhook delivery to avoid blocking.
    pub async fn start(&self, mut rx: broadcast::Receiver<GatewayEvent>) {
        info!("Webhook delivery task started");
        loop {
            match rx.recv().await {
                Ok(event) => {
                    self.handle_event(event).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Webhook delivery lagged by {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Webhook delivery channel closed, stopping");
                    break;
                }
            }
        }
    }

    /// Handle a single event by dispatching to active webhooks.
    async fn handle_event(&self, event: GatewayEvent) {
        let event_type = match &event {
            GatewayEvent::RequestCompleted(_) => "RequestCompleted",
            GatewayEvent::CacheInvalidated(_) => "CacheInvalidated",
            GatewayEvent::KeyCreated(_) => "KeyCreated",
            GatewayEvent::KeyRevoked(_) => "KeyRevoked",
            GatewayEvent::UpstreamError(_) => "UpstreamError",
            GatewayEvent::RateLimitHit(_) => "RateLimitHit",
        };

        let webhooks = self.store.get_active_webhooks(event_type);
        if webhooks.is_empty() {
            return;
        }

        // Serialize event to JSON
        let payload = match serde_json::to_string(&event) {
            Ok(json) => json,
            Err(e) => {
                warn!("Failed to serialize event: {}", e);
                return;
            }
        };

        // Deliver to each webhook in separate tasks
        for webhook in webhooks {
            let client = self.client.clone();
            let store = self.store.clone();
            let max_retries = self.max_retries;
            let retry_base_delay_ms = self.retry_base_delay_ms;
            let payload = payload.clone();
            let webhook_id = webhook.id.clone();
            let webhook_url = webhook.url.clone();
            let webhook_secret = webhook.secret.clone();

            tokio::spawn(async move {
                let success = deliver_with_retry(
                    &client,
                    &webhook_url,
                    &webhook_secret,
                    &payload,
                    max_retries,
                    retry_base_delay_ms,
                )
                .await;

                // Record the result
                store.record_trigger(&webhook_id, success);

                if success {
                    debug!(
                        webhook_id = %webhook_id,
                        url = %webhook_url,
                        "Webhook delivered successfully"
                    );
                } else {
                    warn!(
                        webhook_id = %webhook_id,
                        url = %webhook_url,
                        "Webhook delivery failed after {} retries",
                        max_retries
                    );
                }
            });
        }
    }

    /// Test delivery to a specific webhook URL.
    pub async fn test_delivery(&self, url: &str, secret: &str) -> WebhookTestResult {
        let payload = serde_json::json!({
            "event_type": "TestDelivery",
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "message": "Webhook test delivery from CrabCache"
        });

        let payload_str = payload.to_string();
        let signature = compute_hmac_signature(secret, &payload_str);

        let result = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-CrabCache-Signature", &signature)
            .body(payload_str)
            .send()
            .await;

        match result {
            Ok(response) => {
                let status = response.status().as_u16();
                let success = response.status().is_success();
                WebhookTestResult {
                    success,
                    status: Some(status),
                    error: if !success {
                        Some(format!("HTTP {}", status))
                    } else {
                        None
                    },
                }
            }
            Err(e) => WebhookTestResult {
                success: false,
                status: None,
                error: Some(e.to_string()),
            },
        }
    }
}

/// Deliver a webhook payload with HMAC signature and exponential backoff retry.
///
/// Returns `true` if delivery succeeded (HTTP 2xx), `false` otherwise.
async fn deliver_with_retry(
    client: &reqwest::Client,
    url: &str,
    secret: &str,
    payload: &str,
    max_retries: u32,
    retry_base_delay_ms: u64,
) -> bool {
    let signature = compute_hmac_signature(secret, payload);

    for attempt in 0..=max_retries {
        if attempt > 0 {
            // Exponential backoff: base_delay * 2^(attempt-1)
            let delay_ms = retry_base_delay_ms * 2u64.pow(attempt - 1);
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            debug!(
                url = %url,
                attempt = attempt,
                delay_ms = delay_ms,
                "Retrying webhook delivery"
            );
        }

        match client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-CrabCache-Signature", &signature)
            .body(payload.to_string())
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    return true;
                }
                warn!(
                    url = %url,
                    status = %response.status(),
                    attempt = attempt,
                    "Webhook delivery returned non-success status"
                );
            }
            Err(e) => {
                warn!(
                    url = %url,
                    error = %e,
                    attempt = attempt,
                    "Webhook delivery failed"
                );
            }
        }
    }

    false
}

/// Compute HMAC-SHA256 signature for a payload.
pub fn compute_hmac_signature(secret: &str, payload: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Verify HMAC-SHA256 signature.
pub fn verify_hmac_signature(secret: &str, payload: &str, signature: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());

    let expected_bytes = match hex::decode(signature) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    mac.verify_slice(&expected_bytes).is_ok()
}

/// Generate a random webhook secret.
pub fn generate_webhook_secret() -> String {
    use uuid::Uuid;
    format!("whsec_{}", Uuid::new_v4().to_string().replace('-', ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_webhook_secret() {
        let secret = generate_webhook_secret();
        assert!(secret.starts_with("whsec_"));
        assert_eq!(secret.len(), 40); // "whsec_" (6) + UUID (32) + hyphens removed
    }

    #[test]
    fn test_hmac_signature_roundtrip() {
        let secret = "test-secret-key";
        let payload = "test payload data";

        let signature = compute_hmac_signature(secret, payload);
        assert!(!signature.is_empty());
        assert!(verify_hmac_signature(secret, payload, &signature));
    }

    #[test]
    fn test_hmac_signature_wrong_secret() {
        let secret = "test-secret-key";
        let payload = "test payload data";

        let signature = compute_hmac_signature(secret, payload);
        assert!(!verify_hmac_signature("wrong-secret", payload, &signature));
    }

    #[test]
    fn test_hmac_signature_wrong_payload() {
        let secret = "test-secret-key";
        let payload = "test payload data";

        let signature = compute_hmac_signature(secret, payload);
        assert!(!verify_hmac_signature(secret, "wrong payload", &signature));
    }

    #[test]
    fn test_webhook_store_register_list() {
        let store = WebhookStore::new();
        assert_eq!(store.count(), 0);

        let config = WebhookConfig {
            id: "webhook-1".to_string(),
            url: "https://example.com/webhook".to_string(),
            secret: "secret-1".to_string(),
            events: vec!["RequestCompleted".to_string()],
            enabled: true,
            created_at: 1234567890,
            last_triggered: None,
            failure_count: 0,
        };

        store.register(config);
        assert_eq!(store.count(), 1);

        let webhooks = store.list_all();
        assert_eq!(webhooks.len(), 1);
        assert_eq!(webhooks[0].id, "webhook-1");
    }

    #[test]
    fn test_webhook_store_unregister() {
        let store = WebhookStore::new();

        let config = WebhookConfig {
            id: "webhook-1".to_string(),
            url: "https://example.com/webhook".to_string(),
            secret: "secret-1".to_string(),
            events: vec![],
            enabled: true,
            created_at: 1234567890,
            last_triggered: None,
            failure_count: 0,
        };

        store.register(config);
        assert!(store.unregister("webhook-1"));
        assert!(!store.unregister("webhook-1"));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn test_webhook_store_active_filtering() {
        let store = WebhookStore::new();

        // Enabled, subscribes to all events
        store.register(WebhookConfig {
            id: "webhook-all".to_string(),
            url: "https://example.com/all".to_string(),
            secret: "secret".to_string(),
            events: vec![],
            enabled: true,
            created_at: 1234567890,
            last_triggered: None,
            failure_count: 0,
        });

        // Enabled, subscribes to RequestCompleted only
        store.register(WebhookConfig {
            id: "webhook-rc".to_string(),
            url: "https://example.com/rc".to_string(),
            secret: "secret".to_string(),
            events: vec!["RequestCompleted".to_string()],
            enabled: true,
            created_at: 1234567890,
            last_triggered: None,
            failure_count: 0,
        });

        // Disabled, subscribes to all events
        store.register(WebhookConfig {
            id: "webhook-disabled".to_string(),
            url: "https://example.com/disabled".to_string(),
            secret: "secret".to_string(),
            events: vec![],
            enabled: false,
            created_at: 1234567890,
            last_triggered: None,
            failure_count: 0,
        });

        // Query for RequestCompleted events
        let active = store.get_active_webhooks("RequestCompleted");
        assert_eq!(active.len(), 2); // webhook-all and webhook-rc
        assert!(active.iter().any(|w| w.id == "webhook-all"));
        assert!(active.iter().any(|w| w.id == "webhook-rc"));

        // Query for CacheInvalidated events
        let active = store.get_active_webhooks("CacheInvalidated");
        assert_eq!(active.len(), 1); // only webhook-all
        assert!(active.iter().any(|w| w.id == "webhook-all"));
    }

    #[test]
    fn test_webhook_store_record_trigger() {
        let store = WebhookStore::new();

        store.register(WebhookConfig {
            id: "webhook-1".to_string(),
            url: "https://example.com".to_string(),
            secret: "secret".to_string(),
            events: vec![],
            enabled: true,
            created_at: 1234567890,
            last_triggered: None,
            failure_count: 0,
        });

        // Record failure
        store.record_trigger("webhook-1", false);
        let webhook = store.get("webhook-1").unwrap();
        assert_eq!(webhook.failure_count, 1);
        assert!(webhook.last_triggered.is_none());

        // Record another failure
        store.record_trigger("webhook-1", false);
        let webhook = store.get("webhook-1").unwrap();
        assert_eq!(webhook.failure_count, 2);

        // Record success
        store.record_trigger("webhook-1", true);
        let webhook = store.get("webhook-1").unwrap();
        assert_eq!(webhook.failure_count, 0);
        assert!(webhook.last_triggered.is_some());
    }
}
