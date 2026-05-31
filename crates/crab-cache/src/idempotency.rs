//! Client-controlled request deduplication (Idempotency Layer).
//!
//! Complements `RequestCoalescing`: coalescing merges *concurrent identical* requests,
//! while idempotency deduplicates *sequential retries* using a client-provided key
//! (`Idempotency-Key` or `X-Request-Id` header).
//!
//! Entries are stored in-memory with a short TTL window (default 5 s) and evicted
//! automatically on lookup or via periodic cleanup.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// A stored idempotent response.
struct IdempotencyEntry {
    status: u16,
    body: Vec<u8>,
    created_at: Instant,
}

/// Thread-safe, short-lived idempotency store backed by `DashMap`.
pub struct IdempotencyStore {
    entries: DashMap<String, IdempotencyEntry>,
    window: Duration,
    max_entries: usize,
    hits: AtomicU64,
    misses: AtomicU64,
    saves: AtomicU64,
}

impl IdempotencyStore {
    /// Create a new store with the given TTL window and capacity cap.
    pub fn new(window: Duration, max_entries: usize) -> Self {
        Self {
            entries: DashMap::new(),
            window,
            max_entries,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            saves: AtomicU64::new(0),
        }
    }

    /// Check whether `key` has a cached response within the window.
    /// Returns `Some((status, body))` on hit, `None` on miss.
    pub fn check(&self, key: &str) -> Option<(u16, Vec<u8>)> {
        if let Some(entry) = self.entries.get(key) {
            if entry.created_at.elapsed() < self.window {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some((entry.status, entry.body.clone()));
            }
            // Expired — drop the ref before removing.
            drop(entry);
            self.entries.remove(key);
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Store a completed response under `key`.
    /// Silently drops the write if the store is at capacity.
    pub fn save(&self, key: &str, status: u16, body: Vec<u8>) {
        if self.entries.len() >= self.max_entries {
            return; // Back-pressure: refuse to grow beyond cap.
        }
        self.saves.fetch_add(1, Ordering::Relaxed);
        self.entries.insert(
            key.to_string(),
            IdempotencyEntry {
                status,
                body,
                created_at: Instant::now(),
            },
        );
    }

    /// Remove all expired entries. Call periodically (e.g. every 10 s).
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        self.entries.retain(|_, entry| now.duration_since(entry.created_at) < self.window);
    }

    /// Current number of live entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Cumulative hit count.
    pub fn hit_count(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Cumulative miss count.
    pub fn miss_count(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// Cumulative save count.
    pub fn save_count(&self) -> u64 {
        self.saves.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_hit_and_miss() {
        let store = IdempotencyStore::new(Duration::from_secs(5), 100);
        assert!(store.check("k1").is_none());
        store.save("k1", 200, b"hello".to_vec());
        let (status, body) = store.check("k1").unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"hello");
    }

    #[test]
    fn expired_entry_returns_miss() {
        let store = IdempotencyStore::new(Duration::from_millis(1), 100);
        store.save("k1", 200, b"hello".to_vec());
        std::thread::sleep(Duration::from_millis(10));
        assert!(store.check("k1").is_none());
    }

    #[test]
    fn capacity_cap_refuses_writes() {
        let store = IdempotencyStore::new(Duration::from_secs(5), 2);
        store.save("a", 200, b"a".to_vec());
        store.save("b", 200, b"b".to_vec());
        store.save("c", 200, b"c".to_vec()); // should be silently dropped
        assert_eq!(store.len(), 2);
        assert!(store.check("c").is_none());
    }

    #[test]
    fn cleanup_removes_expired() {
        let store = IdempotencyStore::new(Duration::from_millis(1), 100);
        store.save("a", 200, b"a".to_vec());
        std::thread::sleep(Duration::from_millis(10));
        store.cleanup_expired();
        assert!(store.is_empty());
    }
}
