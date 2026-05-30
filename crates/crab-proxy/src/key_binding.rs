//! Conversation-level upstream key binding for MiMo pipeline.
//!
//! Once a conversation binds to a key, all subsequent requests from the same
//! conversation use that key (no rotation) until:
//! - The binding expires after `mimo_key_binding_ttl_secs` of idle time, or
//! - The key's concurrency exceeds `mimo_key_max_inflight` (temporarily overflow).
//!
//! Uses moka's `time_to_idle` for automatic expiration without background cleanup.

use moka::sync::Cache;
use std::sync::Arc;
use std::time::Duration;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A binding from a stable session id to a specific upstream key.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub key_id: String,
    pub bound_at_ms: u64,
    pub last_used_ms: u64,
}

/// Thread-safe store mapping stable session id -> upstream key id.
///
/// Backed by moka with `time_to_idle` expiration: each `get()` call
/// resets the idle timer, so active conversations stay bound indefinitely.
pub struct KeyBindingStore {
    cache: Cache<String, KeyBinding>,
}

impl KeyBindingStore {
    pub fn new(ttl_secs: u64) -> Arc<Self> {
        Arc::new(Self {
            cache: Cache::builder()
                .time_to_idle(Duration::from_secs(ttl_secs))
                .build(),
        })
    }

    /// Look up the bound key for a session. Returns `None` if no binding
    /// exists or if the binding has expired (idle TTL exceeded).
    pub fn get(&self, session_id: &str) -> Option<KeyBinding> {
        self.cache.get(session_id)
    }

    /// Create or update a binding: session_id -> key_id.
    pub fn put(&self, session_id: String, key_id: String) {
        let now = now_ms();
        self.cache.insert(
            session_id,
            KeyBinding {
                key_id,
                bound_at_ms: now,
                last_used_ms: now,
            },
        );
    }

    /// Refresh the last_used timestamp for an existing binding (resets idle timer).
    pub fn touch(&self, session_id: &str) {
        if let Some(mut binding) = self.cache.get(session_id) {
            binding.last_used_ms = now_ms();
            self.cache.insert(session_id.to_string(), binding);
        }
    }

    /// Remove a binding explicitly (e.g., after key 401 / permanent failure).
    pub fn remove(&self, session_id: &str) {
        self.cache.remove(session_id);
    }

    /// Number of active bindings (for diagnostics).
    pub fn len(&self) -> u64 {
        self.cache.iter().count() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_get() {
        let store = KeyBindingStore::new(300);
        store.put("session-1".into(), "key-a".into());
        let b = store.get("session-1").unwrap();
        assert_eq!(b.key_id, "key-a");
    }

    #[test]
    fn missing_session_returns_none() {
        let store = KeyBindingStore::new(300);
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn touch_refreshes_last_used() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        let before = store.get("s1").unwrap().last_used_ms;
        store.touch("s1");
        let after = store.get("s1").unwrap().last_used_ms;
        assert!(after >= before);
    }

    #[test]
    fn remove_clears_binding() {
        let store = KeyBindingStore::new(300);
        store.put("s1".into(), "k1".into());
        store.remove("s1");
        assert!(store.get("s1").is_none());
    }
}
