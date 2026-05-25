use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Mock Redis backend for testing TieredCache without real Redis.
///
/// Simulates network latency and connection pool behavior with configurable parameters.
#[derive(Clone)]
pub struct MockRedisBackend {
    store: Arc<DashMap<String, (String, Instant, u64)>>,
    latency: Duration,
    failure_rate: f64,
}

impl MockRedisBackend {
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
            latency: Duration::from_millis(2),
            failure_rate: 0.0,
        }
    }

    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = latency;
        self
    }

    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        tokio::time::sleep(self.latency).await;

        if rand::random::<f64>() < self.failure_rate {
            return None;
        }

        self.store.get(key).and_then(|entry| {
            let elapsed = entry.1.elapsed().as_secs();
            if elapsed < entry.2 {
                Some(entry.0.clone())
            } else {
                None
            }
        })
    }

    pub async fn set_ex(&self, key: &str, value: &str, ttl: u64) -> Result<()> {
        tokio::time::sleep(self.latency).await;

        if rand::random::<f64>() < self.failure_rate {
            anyhow::bail!("Simulated Redis failure");
        }

        self.store
            .insert(key.to_string(), (value.to_string(), Instant::now(), ttl));
        Ok(())
    }

    pub async fn del(&self, key: &str) -> Result<()> {
        tokio::time::sleep(self.latency).await;
        self.store.remove(key);
        Ok(())
    }

    pub fn clear(&self) {
        self.store.clear();
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

impl Default for MockRedisBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_redis_basic_operations() {
        let redis = MockRedisBackend::new();

        redis.set_ex("key1", "value1", 60).await.unwrap();
        let val = redis.get("key1").await;
        assert_eq!(val, Some("value1".to_string()));

        redis.del("key1").await.unwrap();
        let val = redis.get("key1").await;
        assert_eq!(val, None);
    }

    #[tokio::test]
    async fn test_mock_redis_ttl_expiration() {
        let redis = MockRedisBackend::new();
        redis.set_ex("key1", "value1", 1).await.unwrap();

        assert!(redis.get("key1").await.is_some());

        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(redis.get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_mock_redis_failure_rate() {
        let redis = MockRedisBackend::new().with_failure_rate(1.0);
        redis.set_ex("key1", "value1", 60).await.unwrap_err();
        assert!(redis.get("key1").await.is_none());
    }
}
