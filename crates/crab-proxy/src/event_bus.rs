//! Event bus for gateway-wide pub/sub notifications.
//!
//! Uses `tokio::sync::broadcast` for efficient multi-consumer event distribution.
//! Events are published from proxy phases (logging, response_filter) and consumed
//! by background tasks (webhook delivery, future extensions).

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Gateway event types published through the event bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum GatewayEvent {
    /// Request completed successfully (emitted from logging phase).
    RequestCompleted(RequestCompletedEvent),
    /// Cache invalidation performed.
    CacheInvalidated(CacheInvalidatedEvent),
    /// API key created.
    KeyCreated(KeyCreatedEvent),
    /// API key revoked.
    KeyRevoked(KeyRevokedEvent),
    /// Upstream error occurred.
    UpstreamError(UpstreamErrorEvent),
    /// Rate limit triggered.
    RateLimitHit(RateLimitHitEvent),
}

/// Request completed event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCompletedEvent {
    pub request_id: String,
    pub model: String,
    pub consumer: Option<String>,
    pub project_id: Option<String>,
    pub latency_ms: u64,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
    pub is_streaming: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub upstream_status: Option<u16>,
    pub pipeline: Option<String>,
    pub backend_name: Option<String>,
    pub client_ip: Option<String>,
    pub timestamp: u64,
}

/// Cache invalidation event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheInvalidatedEvent {
    pub scope: String,
    pub success: bool,
    pub admin: Option<String>,
}

/// Key created event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyCreatedEvent {
    pub key_id: String,
    pub admin: Option<String>,
}

/// Key revoked event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRevokedEvent {
    pub key_id: String,
    pub admin: Option<String>,
}

/// Upstream error event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamErrorEvent {
    pub request_id: String,
    pub status: u16,
    pub model: String,
    pub backend: Option<String>,
    pub error_body: Option<String>,
}

/// Rate limit hit event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitHitEvent {
    pub request_id: String,
    pub key_id: Option<String>,
    pub client_ip: Option<String>,
    pub retry_after_secs: Option<u64>,
}

/// Event bus for publishing and subscribing to gateway events.
///
/// Uses `tokio::sync::broadcast` internally. The `broadcast::Sender` is `Clone`
/// and can be shared across multiple threads/tasks.
pub struct EventBus {
    sender: broadcast::Sender<GatewayEvent>,
    subscriber_count: Arc<AtomicUsize>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            subscriber_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Publish an event to all subscribers.
    ///
    /// This is non-blocking. If there are no subscribers, the event is silently dropped.
    /// If a subscriber is lagging (too slow), it will receive a `RecvError::Lagged` error
    /// on its next `recv()` call.
    pub fn publish(&self, event: GatewayEvent) {
        // broadcast::send returns Ok(n) where n is the number of active receivers.
        // It returns Err if there are no receivers, which is fine for our use case.
        let _ = self.sender.send(event);
    }

    /// Subscribe to events. Returns a receiver that will get all future events.
    pub fn subscribe(&self) -> broadcast::Receiver<GatewayEvent> {
        self.subscriber_count.fetch_add(1, Ordering::SeqCst);
        self.sender.subscribe()
    }

    /// Get the current number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::SeqCst)
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            subscriber_count: Arc::clone(&self.subscriber_count),
        }
    }
}

/// Global event bus instance.
///
/// Initialized once and shared across all workers. The capacity (1024) is chosen
/// to handle moderate event rates while bounded memory usage.
static GLOBAL_EVENT_BUS: once_cell::sync::Lazy<EventBus> =
    once_cell::sync::Lazy::new(|| EventBus::new(1024));

/// Get the global event bus instance.
pub fn global_event_bus() -> &'static EventBus {
    &GLOBAL_EVENT_BUS
}

/// Get current timestamp in seconds since Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        let event = GatewayEvent::RequestCompleted(RequestCompletedEvent {
            request_id: "test-123".to_string(),
            model: "gpt-4o".to_string(),
            consumer: Some("test-user".to_string()),
            project_id: None,
            latency_ms: 100,
            cache_hit: false,
            cache_tier: None,
            is_streaming: false,
            input_tokens: Some(50),
            output_tokens: Some(25),
            upstream_status: Some(200),
            pipeline: None,
            backend_name: None,
            client_ip: Some("127.0.0.1".to_string()),
            timestamp: now_secs(),
        });

        bus.publish(event.clone());

        // Receiver should get the event
        let received = rx.try_recv();
        assert!(received.is_ok());

        match received.unwrap() {
            GatewayEvent::RequestCompleted(e) => {
                assert_eq!(e.request_id, "test-123");
                assert_eq!(e.model, "gpt-4o");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_event_bus_no_subscribers() {
        let bus = EventBus::new(16);

        // Publishing with no subscribers should not panic
        let event = GatewayEvent::RequestCompleted(RequestCompletedEvent {
            request_id: "test-456".to_string(),
            model: "gpt-4o".to_string(),
            consumer: None,
            project_id: None,
            latency_ms: 50,
            cache_hit: false,
            cache_tier: None,
            is_streaming: false,
            input_tokens: None,
            output_tokens: None,
            upstream_status: Some(200),
            pipeline: None,
            backend_name: None,
            client_ip: None,
            timestamp: now_secs(),
        });
        bus.publish(event);
    }

    #[test]
    fn test_event_bus_multiple_subscribers() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        let event = GatewayEvent::CacheInvalidated(CacheInvalidatedEvent {
            scope: "all".to_string(),
            success: true,
            admin: Some("admin@test.com".to_string()),
        });

        bus.publish(event);

        // Both receivers should get the event
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn test_event_bus_clone() {
        let bus1 = EventBus::new(16);
        let bus2 = bus1.clone();

        assert_eq!(bus1.subscriber_count(), bus2.subscriber_count());

        let mut rx = bus2.subscribe();
        let event = GatewayEvent::KeyCreated(KeyCreatedEvent {
            key_id: "key-123".to_string(),
            admin: None,
        });

        bus1.publish(event);
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn test_event_serialization() {
        let event = GatewayEvent::RequestCompleted(RequestCompletedEvent {
            request_id: "test-789".to_string(),
            model: "deepseek-v4".to_string(),
            consumer: Some("team-alpha".to_string()),
            project_id: Some("proj-123".to_string()),
            latency_ms: 245,
            cache_hit: true,
            cache_tier: Some("L0".to_string()),
            is_streaming: false,
            input_tokens: Some(1500),
            output_tokens: Some(320),
            upstream_status: Some(200),
            pipeline: Some("CursorDeepSeekV4".to_string()),
            backend_name: Some("deepseek-primary".to_string()),
            client_ip: Some("192.168.1.100".to_string()),
            timestamp: 1748679840,
        });

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: GatewayEvent = serde_json::from_str(&json).unwrap();

        match deserialized {
            GatewayEvent::RequestCompleted(e) => {
                assert_eq!(e.request_id, "test-789");
                assert_eq!(e.model, "deepseek-v4");
                assert_eq!(e.latency_ms, 245);
                assert!(e.cache_hit);
            }
            _ => panic!("Wrong event type after deserialization"),
        }
    }
}
