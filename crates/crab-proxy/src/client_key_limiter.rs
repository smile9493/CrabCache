//! Per-client API key (sk-cc-*) in-flight concurrency tracking and limits.

use crate::stored_key::StoredKey;
use crab_metrics::global_metrics;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKeyLimitError {
    Exceeded,
}

impl std::fmt::Display for ClientKeyLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exceeded => write!(f, "client key concurrency limit exceeded"),
        }
    }
}

struct ClientKeySlot {
    inflight: AtomicUsize,
    max_concurrent: AtomicU32,
    consumer: Arc<str>,
    key_id: Arc<str>,
}

pub struct ClientKeyLimiter {
    slots: DashMap<String, Arc<ClientKeySlot>>,
}

/// RAII guard; decrements inflight on drop.
pub struct ClientKeyGuard {
    limiter: Arc<ClientKeyLimiter>,
    token: String,
    slot: Arc<ClientKeySlot>,
}

impl Drop for ClientKeyGuard {
    fn drop(&mut self) {
        self.limiter.release(&self.token, &self.slot);
    }
}

impl ClientKeyLimiter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            slots: DashMap::new(),
        })
    }

    pub fn sync_key(&self, token: &str, key: &StoredKey) {
        let consumer: Arc<str> = Arc::from(key.name.as_str());
        let key_id: Arc<str> = Arc::from(key.id.as_str());
        let max = key.max_concurrent;
        match self.slots.entry(token.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(mut e) => {
                let slot = e.get();
                let names_changed = slot.consumer.as_ref() != key.name.as_str()
                    || slot.key_id.as_ref() != key.id.as_str();
                if names_changed {
                    let inflight = slot.inflight.load(Ordering::Relaxed);
                    e.insert(Arc::new(ClientKeySlot {
                        inflight: AtomicUsize::new(inflight),
                        max_concurrent: AtomicU32::new(max),
                        consumer,
                        key_id,
                    }));
                } else {
                    slot.max_concurrent.store(max, Ordering::Relaxed);
                }
            }
            dashmap::mapref::entry::Entry::Vacant(e) => {
                e.insert(Arc::new(ClientKeySlot {
                    inflight: AtomicUsize::new(0),
                    max_concurrent: AtomicU32::new(max),
                    consumer,
                    key_id,
                }));
            }
        }
    }

    pub fn sync_all_keys(&self, keys: &dashmap::DashMap<String, StoredKey>) {
        for entry in keys.iter() {
            self.sync_key(entry.key(), entry.value());
        }
    }

    pub fn remove_key(&self, token: &str) {
        if let Some((_, slot)) = self.slots.remove(token) {
            let inflight = slot.inflight.load(Ordering::Relaxed);
            global_metrics().remove_client_key_inflight(&slot.key_id, &slot.consumer);
            if inflight > 0 {
                tracing::debug!(
                    token_preview = %crate::upstream_pool::key_preview(token),
                    inflight,
                    "Removed client key slot while requests still in flight"
                );
            }
        }
    }

    pub fn inflight(&self, token: &str) -> usize {
        self.slots
            .get(token)
            .map(|s| s.inflight.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn try_acquire(
        self: &Arc<Self>,
        token: &str,
        key: &StoredKey,
    ) -> Result<ClientKeyGuard, ClientKeyLimitError> {
        self.sync_key(token, key);
        let slot = self
            .slots
            .get(token)
            .map(|e| e.value().clone())
            .expect("slot just synced");

        loop {
            let current = slot.inflight.load(Ordering::Relaxed);
            let max = slot.max_concurrent.load(Ordering::Relaxed);
            if max > 0 && current >= max as usize {
                return Err(ClientKeyLimitError::Exceeded);
            }
            if slot
                .inflight
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                let inflight = current + 1;
                global_metrics().set_client_key_inflight(
                    &slot.key_id,
                    &slot.consumer,
                    inflight as i64,
                );
                return Ok(ClientKeyGuard {
                    limiter: Arc::clone(self),
                    token: token.to_string(),
                    slot,
                });
            }
        }
    }

    fn release(&self, _token: &str, slot: &ClientKeySlot) {
        let prev = slot.inflight.fetch_sub(1, Ordering::AcqRel);
        let inflight = prev.saturating_sub(1);
        global_metrics().set_client_key_inflight(&slot.key_id, &slot.consumer, inflight as i64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key(max: u32) -> StoredKey {
        StoredKey {
            id: "key-1".into(),
            name: "test-consumer".into(),
            key_hash: "sk-cc-test".into(),
            enabled: true,
            domain: None,
            project_id: None,
            pipeline: None,
            upstream_profile: None,
            max_concurrent: max,
            rpm_limit: 0,
        }
    }

    #[test]
    fn unlimited_allows_many_acquires() {
        let limiter = ClientKeyLimiter::new();
        let key = sample_key(0);
        let g1 = limiter.try_acquire("sk-cc-test", &key).unwrap();
        let g2 = limiter.try_acquire("sk-cc-test", &key).unwrap();
        assert_eq!(limiter.inflight("sk-cc-test"), 2);
        drop(g1);
        assert_eq!(limiter.inflight("sk-cc-test"), 1);
        drop(g2);
        assert_eq!(limiter.inflight("sk-cc-test"), 0);
    }

    #[test]
    fn limit_rejects_when_full() {
        let limiter = ClientKeyLimiter::new();
        let key = sample_key(1);
        let _g1 = limiter.try_acquire("sk-cc-test", &key).unwrap();
        assert!(limiter.try_acquire("sk-cc-test", &key).is_err());
    }

    #[test]
    fn patch_max_updates_limit() {
        let limiter = ClientKeyLimiter::new();
        let mut key = sample_key(1);
        let _g1 = limiter.try_acquire("sk-cc-test", &key).unwrap();
        assert!(limiter.try_acquire("sk-cc-test", &key).is_err());
        key.max_concurrent = 2;
        limiter.sync_key("sk-cc-test", &key);
        assert!(limiter.try_acquire("sk-cc-test", &key).is_ok());
    }

    #[test]
    fn remove_key_clears_slot() {
        let limiter = ClientKeyLimiter::new();
        let key = sample_key(0);
        let _g = limiter.try_acquire("sk-cc-test", &key).unwrap();
        limiter.remove_key("sk-cc-test");
        assert!(!limiter.slots.contains_key("sk-cc-test"));
    }
}
