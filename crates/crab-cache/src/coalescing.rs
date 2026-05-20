use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

pub struct RequestCoalescer {
    inflight: DashMap<String, Arc<InflightEntry>>,
    max_inflight: usize,
    follower_timeout: Duration,
}

struct InflightEntry {
    notify: Arc<Notify>,
    completed: std::sync::atomic::AtomicBool,
    /// Set when the leader upstream request failed (429/502/etc.) so followers do not retry upstream.
    failed: std::sync::atomic::AtomicBool,
}

impl RequestCoalescer {
    pub fn new() -> Self {
        Self {
            inflight: DashMap::new(),
            max_inflight: 1000,
            follower_timeout: Duration::from_secs(60),
        }
    }

    pub fn with_config(max_inflight: usize, follower_timeout_secs: u64) -> Self {
        Self {
            inflight: DashMap::new(),
            max_inflight,
            follower_timeout: Duration::from_secs(follower_timeout_secs),
        }
    }

    pub async fn acquire(&self, key: &str) -> Result<CoalesceGuard, CoalesceError> {
        loop {
            if let Some(entry) = self.inflight.get(key) {
                let entry = entry.clone();

                if entry.completed.load(std::sync::atomic::Ordering::Acquire) {
                    return Ok(CoalesceGuard {
                        key: key.to_string(),
                        is_leader: false,
                        inflight: self.inflight.clone(),
                        entry: Some(entry),
                    });
                }

                let wait_result = timeout(self.follower_timeout, entry.notify.notified()).await;

                if wait_result.is_err() {
                    return Err(CoalesceError::Timeout);
                }

                if entry.completed.load(std::sync::atomic::Ordering::Acquire) {
                    return Ok(CoalesceGuard {
                        key: key.to_string(),
                        is_leader: false,
                        inflight: self.inflight.clone(),
                        entry: Some(entry),
                    });
                }

                continue;
            }

            if self.inflight.len() >= self.max_inflight {
                return Err(CoalesceError::CapacityExceeded);
            }

            let entry = Arc::new(InflightEntry {
                notify: Arc::new(Notify::new()),
                completed: std::sync::atomic::AtomicBool::new(false),
                failed: std::sync::atomic::AtomicBool::new(false),
            });

            use dashmap::mapref::entry::Entry;
            match self.inflight.entry(key.to_string()) {
                Entry::Occupied(existing) => {
                    let entry = existing.get().clone();

                    if entry.completed.load(std::sync::atomic::Ordering::Acquire) {
                        return Ok(CoalesceGuard {
                            key: key.to_string(),
                            is_leader: false,
                            inflight: self.inflight.clone(),
                            entry: Some(entry),
                        });
                    }

                    let wait_result = timeout(self.follower_timeout, entry.notify.notified()).await;

                    if wait_result.is_err() {
                        return Err(CoalesceError::Timeout);
                    }
                    continue;
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(entry.clone());
                    return Ok(CoalesceGuard {
                        key: key.to_string(),
                        is_leader: true,
                        inflight: self.inflight.clone(),
                        entry: Some(entry),
                    });
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.inflight.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inflight.is_empty()
    }
}

impl Default for RequestCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoalesceError {
    Timeout,
    CapacityExceeded,
}

impl std::fmt::Display for CoalesceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoalesceError::Timeout => write!(f, "Coalescing wait timeout"),
            CoalesceError::CapacityExceeded => write!(f, "Too many inflight requests"),
        }
    }
}

impl std::error::Error for CoalesceError {}

pub struct CoalesceGuard {
    key: String,
    is_leader: bool,
    inflight: DashMap<String, Arc<InflightEntry>>,
    entry: Option<Arc<InflightEntry>>,
}

impl CoalesceGuard {
    pub fn is_leader(&self) -> bool {
        self.is_leader
    }

    pub fn mark_completed(&self) {
        if let Some(entry) = &self.entry {
            entry
                .completed
                .store(true, std::sync::atomic::Ordering::Release);
            entry.notify.notify_waiters();
        }
    }

    /// Leader upstream failed; wake followers without allowing upstream retry.
    pub fn mark_failed(&self) {
        if let Some(entry) = &self.entry {
            entry
                .failed
                .store(true, std::sync::atomic::Ordering::Release);
            entry
                .completed
                .store(true, std::sync::atomic::Ordering::Release);
            entry.notify.notify_waiters();
        }
    }

    /// True when the leader marked failure before followers resumed.
    pub fn leader_failed(&self) -> bool {
        self.entry.as_ref().is_some_and(|entry| {
            entry
                .failed
                .load(std::sync::atomic::Ordering::Acquire)
        })
    }
}

impl Drop for CoalesceGuard {
    fn drop(&mut self) {
        if self.is_leader {
            if let Some(entry) = &self.entry {
                entry
                    .completed
                    .store(true, std::sync::atomic::Ordering::Release);
                entry.notify.notify_waiters();
            }
            self.inflight.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_coalescer_leader() {
        let coalescer = RequestCoalescer::new();
        let guard = coalescer.acquire("test-key").await.unwrap();
        assert!(guard.is_leader());
    }

    #[tokio::test]
    async fn test_coalescer_follower() {
        let coalescer = Arc::new(RequestCoalescer::new());

        let guard1 = coalescer.acquire("test-key").await.unwrap();
        assert!(guard1.is_leader());

        let coalescer_clone = coalescer.clone();
        let handle = tokio::spawn(async move {
            let guard2 = coalescer_clone.acquire("test-key").await.unwrap();
            assert!(!guard2.is_leader());
        });

        sleep(Duration::from_millis(100)).await;
        guard1.mark_completed();
        drop(guard1);

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_coalescer_multiple_followers() {
        let coalescer = Arc::new(RequestCoalescer::new());

        let guard1 = coalescer.acquire("test-key").await.unwrap();
        assert!(guard1.is_leader());

        let mut handles = vec![];
        for _ in 0..10 {
            let coalescer_clone = coalescer.clone();
            handles.push(tokio::spawn(async move {
                let guard = coalescer_clone.acquire("test-key").await.unwrap();
                assert!(!guard.is_leader());
            }));
        }

        sleep(Duration::from_millis(100)).await;
        guard1.mark_completed();
        drop(guard1);

        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_coalescer_capacity_limit() {
        let coalescer = RequestCoalescer::with_config(2, 60);

        let _guard1 = coalescer.acquire("key1").await.unwrap();
        let _guard2 = coalescer.acquire("key2").await.unwrap();

        let result = coalescer.acquire("key3").await;
        assert!(matches!(result, Err(CoalesceError::CapacityExceeded)));
    }

    #[tokio::test]
    async fn test_coalescer_leader_failed_propagates() {
        let coalescer = Arc::new(RequestCoalescer::new());

        let guard1 = coalescer.acquire("test-key").await.unwrap();
        assert!(guard1.is_leader());

        let coalescer_clone = coalescer.clone();
        let handle = tokio::spawn(async move {
            let guard2 = coalescer_clone.acquire("test-key").await.unwrap();
            assert!(!guard2.is_leader());
            assert!(guard2.leader_failed());
        });

        sleep(Duration::from_millis(100)).await;
        guard1.mark_failed();
        drop(guard1);

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_coalescer_timeout() {
        let coalescer = RequestCoalescer::with_config(100, 1);

        let _guard = coalescer.acquire("test-key").await.unwrap();

        let result = coalescer.acquire("test-key").await;
        assert!(matches!(result, Err(CoalesceError::Timeout)));
    }
}
