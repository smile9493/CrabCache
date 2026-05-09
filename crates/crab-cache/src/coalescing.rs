use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Notify;

pub struct RequestCoalescer {
    inflight: DashMap<String, Arc<Notify>>,
}

impl RequestCoalescer {
    pub fn new() -> Self {
        Self {
            inflight: DashMap::new(),
        }
    }

    pub async fn acquire(&self, key: &str) -> CoalesceGuard {
        if let Some(notify) = self.inflight.get(key) {
            let notify = notify.clone();
            notify.notified().await;
            return CoalesceGuard {
                key: key.to_string(),
                is_leader: false,
                inflight: self.inflight.clone(),
            };
        }

        let notify = Arc::new(Notify::new());
        self.inflight.insert(key.to_string(), notify);

        CoalesceGuard {
            key: key.to_string(),
            is_leader: true,
            inflight: self.inflight.clone(),
        }
    }

    pub fn release(&self, key: &str) {
        if let Some((_, notify)) = self.inflight.remove(key) {
            notify.notify_waiters();
        }
    }
}

pub struct CoalesceGuard {
    key: String,
    is_leader: bool,
    inflight: DashMap<String, Arc<Notify>>,
}

impl CoalesceGuard {
    pub fn is_leader(&self) -> bool {
        self.is_leader
    }
}

impl Drop for CoalesceGuard {
    fn drop(&mut self) {
        if self.is_leader {
            if let Some((_, notify)) = self.inflight.remove(&self.key) {
                notify.notify_waiters();
            }
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
        let guard = coalescer.acquire("test-key").await;
        assert!(guard.is_leader());
    }

    #[tokio::test]
    async fn test_coalescer_follower() {
        let coalescer = Arc::new(RequestCoalescer::new());

        let guard1 = coalescer.acquire("test-key").await;
        assert!(guard1.is_leader());

        let coalescer_clone = coalescer.clone();
        let handle = tokio::spawn(async move {
            let guard2 = coalescer_clone.acquire("test-key").await;
            assert!(!guard2.is_leader());
        });

        sleep(Duration::from_millis(100)).await;
        drop(guard1);

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_coalescer_multiple_followers() {
        let coalescer = Arc::new(RequestCoalescer::new());

        let guard1 = coalescer.acquire("test-key").await;
        assert!(guard1.is_leader());

        let mut handles = vec![];
        for _ in 0..10 {
            let coalescer_clone = coalescer.clone();
            handles.push(tokio::spawn(async move {
                let guard = coalescer_clone.acquire("test-key").await;
                assert!(!guard.is_leader());
            }));
        }

        sleep(Duration::from_millis(100)).await;
        drop(guard1);

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
