use crate::{CacheEntry, L0Config, TtlConfig};
use anyhow::Result;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use crab_metrics::{global_metrics, CacheTier};
use moka::future::Cache;
use redis::AsyncCommands;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use tracing::{debug, warn};

pub struct TieredCache {
    l0: Cache<String, CacheEntry>,
    l1_pool: Pool<RedisConnectionManager>,
    ttl_config: Arc<RwLock<TtlConfig>>,
}

impl TieredCache {
    pub async fn new(
        l1_pool: Pool<RedisConnectionManager>,
        l0_config: L0Config,
        ttl_config: Arc<RwLock<TtlConfig>>,
    ) -> Result<Self> {
        let l0 = Cache::builder()
            .max_capacity(l0_config.max_capacity)
            .time_to_live(Duration::from_secs(l0_config.ttl_secs))
            .build();

        Ok(Self {
            l0,
            l1_pool,
            ttl_config,
        })
    }

    pub async fn get(&self, key: &str) -> Option<(CacheEntry, CacheTier)> {
        if let Some(entry) = self.l0.get(key).await {
            global_metrics().record_cache_hit(CacheTier::L0Moka, &entry.model, None);
            debug!(key = key, tier = "L0", "Cache hit");
            return Some((entry, CacheTier::L0Moka));
        }

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache get");
                global_metrics().record_cache_miss(CacheTier::Miss);
                return None;
            }
        };

        let result: Option<String> = match conn.get(format!("cache:{}", key)).await {
            Ok(val) => val,
            Err(e) => {
                warn!(error = %e, key = key, "Redis GET failed for cache key");
                global_metrics().record_cache_miss(CacheTier::Miss);
                return None;
            }
        };

        if let Some(json) = result {
            match serde_json::from_str::<CacheEntry>(&json) {
                Ok(entry) => {
                    global_metrics().record_cache_hit(CacheTier::L1Redis, &entry.model, None);
                    debug!(key = key, tier = "L1", "Cache hit");

                    self.l0.insert(key.to_string(), entry.clone()).await;

                    return Some((entry, CacheTier::L1Redis));
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        key = key,
                        json_len = json.len(),
                        "Failed to deserialize cache entry from Redis, deleting bad entry"
                    );
                    // Delete the bad entry to prevent permanent stale data
                    let cache_key = format!("cache:{}", key);
                    if let Err(del_err) = conn.del::<&str, ()>(&cache_key).await {
                        warn!(
                            error = %del_err,
                            key = key,
                            "Failed to delete bad cache entry from Redis"
                        );
                    }
                }
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
        let ttl = self
            .ttl_config
            .read()
            .map_err(|e| anyhow::anyhow!("TTL config lock poisoned: {}", e))?
            .resolve(model, consumer);

        self.l0.insert(key.to_string(), entry.clone()).await;

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache put");
                return Err(e.into());
            }
        };

        let json = serde_json::to_string(&entry)?;
        let cache_key = format!("cache:{}", key);

        if let Err(e) = conn.set_ex::<&str, &str, ()>(&cache_key, &json, ttl).await {
            warn!(error = %e, key = key, "Redis SETEX failed for cache put");
            return Err(e.into());
        }

        debug!(
            key = key,
            ttl_secs = ttl,
            "Cache entry stored in L0 and L1"
        );

        Ok(())
    }

    pub fn resolve_ttl(&self, model: &str, consumer: Option<&str>) -> u64 {
        self.ttl_config
            .read()
            .map(|c| c.resolve(model, consumer))
            .unwrap_or(3600)
    }

    pub fn ttl_config(&self) -> Arc<RwLock<TtlConfig>> {
        self.ttl_config.clone()
    }

    pub async fn invalidate(&self, key: &str) -> Result<()> {
        self.l0.invalidate(key).await;

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache invalidate");
                return Err(e.into());
            }
        };

        let cache_key = format!("cache:{}", key);
        if let Err(e) = conn.del::<&str, ()>(&cache_key).await {
            warn!(error = %e, key = key, "Redis DEL failed for cache invalidate");
            return Err(e.into());
        }

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
