use crate::{CacheEntry, TtlConfig};
use anyhow::Result;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use crab_metrics::{global_metrics, CacheTier};
use moka::future::Cache;
use redis::AsyncCommands;
use std::time::Duration;
use tracing::debug;

pub struct TieredCache {
    l0: Cache<String, CacheEntry>,
    l1_pool: Pool<RedisConnectionManager>,
    ttl_config: TtlConfig,
}

impl TieredCache {
    pub async fn new(l1_pool: Pool<RedisConnectionManager>, ttl_config: TtlConfig) -> Result<Self> {
        let l0 = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(3600))
            .build();

        Ok(Self {
            l0,
            l1_pool,
            ttl_config,
        })
    }

    pub async fn get(&self, key: &str) -> Option<(CacheEntry, CacheTier)> {
        if let Some(entry) = self.l0.get(key).await {
            global_metrics().record_cache_hit(CacheTier::L0Moka, "unknown", None);
            debug!(key = key, tier = "L0", "Cache hit");
            return Some((entry, CacheTier::L0Moka));
        }

        let mut conn = self.l1_pool.get().await.ok()?;
        let result: Option<String> = conn.get(format!("cache:{}", key)).await.ok()?;

        if let Some(json) = result {
            if let Ok(entry) = serde_json::from_str::<CacheEntry>(&json) {
                global_metrics().record_cache_hit(CacheTier::L1Redis, "unknown", None);
                debug!(key = key, tier = "L1", "Cache hit");

                self.l0.insert(key.to_string(), entry.clone()).await;

                return Some((entry, CacheTier::L1Redis));
            }
        }

        global_metrics().record_cache_miss(CacheTier::Miss);
        debug!(key = key, "Cache miss");
        None
    }

    pub async fn put(
        &self,
        key: &str,
        entry: CacheEntry,
        model: &str,
        consumer: Option<&str>,
    ) -> Result<()> {
        let ttl = self.ttl_config.resolve(model, consumer);

        self.l0.insert(key.to_string(), entry.clone()).await;

        let mut conn = self.l1_pool.get().await?;
        let json = serde_json::to_string(&entry)?;
        let cache_key = format!("cache:{}", key);

        let _: () = conn.set_ex(&cache_key, &json, ttl).await?;

        debug!(
            key = key,
            ttl_secs = ttl,
            "Cache entry stored in L0 and L1"
        );

        Ok(())
    }

    pub async fn invalidate(&self, key: &str) -> Result<()> {
        self.l0.invalidate(key).await;

        let mut conn = self.l1_pool.get().await?;
        let cache_key = format!("cache:{}", key);
        let _: () = conn.del(&cache_key).await?;

        debug!(key = key, "Cache entry invalidated");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ttl_config() {
        let config = TtlConfig::new(3600);
        assert_eq!(config.resolve("v4-pro", None), 3600);
    }
}
