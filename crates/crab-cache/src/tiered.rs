use crate::{CacheEntry, L0Config, TtlConfig};
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use crab_metrics::{CacheTier, global_metrics};
use moka::future::Cache;
use redis::{AsyncCommands, cmd};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Error type for cache operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache operation failed: {0}")]
    Operation(String),
}

impl From<serde_json::Error> for CacheError {
    fn from(e: serde_json::Error) -> Self {
        CacheError::Operation(e.to_string())
    }
}

/// Options for Redis SCAN during cache invalidation.
#[derive(Clone, Debug, Default)]
pub struct InvalidateScanOptions {
    pub max_duration: Option<Duration>,
}

impl InvalidateScanOptions {
    pub fn from_timeout_secs(secs: u64) -> Self {
        Self {
            max_duration: Some(Duration::from_secs(secs)),
        }
    }
}

const SCAN_PROGRESS_EVERY_BATCHES: u64 = 10;

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
    ) -> Result<Self, CacheError> {
        let l0 = Cache::builder()
            .max_capacity(l0_config.max_capacity)
            .time_to_live(Duration::from_secs(l0_config.ttl_secs))
            .support_invalidation_closures()
            .build();

        Ok(Self {
            l0,
            l1_pool,
            ttl_config,
        })
    }

    #[tracing::instrument(skip(self), fields(key = %key, consumer = consumer.map(|c| c.as_ref()).unwrap_or("none")))]
    pub async fn get(
        &self,
        key: &str,
        consumer: Option<&str>,
        domain: Option<&str>,
    ) -> Option<(CacheEntry, CacheTier)> {
        if let Some(entry) = self.l0.get(key).await {
            global_metrics().record_cache_hit(CacheTier::L0Moka, &entry.model, consumer, domain);
            debug!(key = key, tier = "L0", "Cache hit");
            return Some((entry, CacheTier::L0Moka));
        }

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache get");
                global_metrics().record_cache_miss(CacheTier::Miss, domain);
                return None;
            }
        };

        let result: Option<String> = match conn.get(format!("cache:{key}")).await {
            Ok(val) => val,
            Err(e) => {
                warn!(error = %e, key = key, "Redis GET failed for cache key");
                global_metrics().record_cache_miss(CacheTier::Miss, domain);
                return None;
            }
        };

        if let Some(json) = result {
            match serde_json::from_str::<CacheEntry>(&json) {
                Ok(entry) => {
                    global_metrics().record_cache_hit(
                        CacheTier::L1Redis,
                        &entry.model,
                        consumer,
                        domain,
                    );
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
                    let cache_key = format!("cache:{key}");
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

        global_metrics().record_cache_miss(CacheTier::Miss, domain);
        debug!(key = key, "Cache miss");
        None
    }

    #[tracing::instrument(skip(self, entry), fields(key = %key, model = %model, consumer = consumer.map(|c| c.as_ref()).unwrap_or("none")))]
    pub async fn put(
        &self,
        key: &str,
        entry: CacheEntry,
        model: &str,
        consumer: Option<&str>,
    ) -> Result<(), CacheError> {
        let ttl = self
            .ttl_config
            .read()
            .map_err(|e| CacheError::Operation(format!("TTL config lock poisoned: {e}")))?
            .resolve(model, consumer);

        self.l0.insert(key.to_string(), entry.clone()).await;

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache put");
                return Err(CacheError::Operation(e.to_string()));
            }
        };

        let json = serde_json::to_string(&entry)?;
        let cache_key = format!("cache:{key}");

        if let Err(e) = conn.set_ex::<&str, &str, ()>(&cache_key, &json, ttl).await {
            warn!(error = %e, key = key, "Redis SETEX failed for cache put");
            return Err(CacheError::Operation(e.to_string()));
        }

        debug!(key = key, ttl_secs = ttl, "Cache entry stored in L0 and L1");

        Ok(())
    }

    /// Returns true if Redis responds to PING (used for readiness probes).
    pub async fn ping(&self) -> bool {
        let check = async {
            let mut conn = self.l1_pool.get().await.ok()?;
            let pong: String = cmd("PING").query_async(&mut *conn).await.ok()?;
            Some(pong)
        };
        matches!(
            tokio::time::timeout(Duration::from_secs(2), check).await,
            Ok(Some(_))
        )
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

    #[tracing::instrument(skip(self), fields(key = %key))]
    pub async fn invalidate(&self, key: &str) -> Result<(), CacheError> {
        self.l0.invalidate(key).await;

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache invalidate");
                return Err(CacheError::Operation(e.to_string()));
            }
        };

        let cache_key = format!("cache:{key}");
        if let Err(e) = conn.del::<&str, ()>(&cache_key).await {
            warn!(error = %e, key = key, "Redis DEL failed for cache invalidate");
            return Err(CacheError::Operation(e.to_string()));
        }

        debug!(key = key, "Cache entry invalidated");
        Ok(())
    }

    /// Invalidate all cache entries (L0 + L1 scan and delete).
    #[tracing::instrument(skip(self, scan), fields(scope = "all"))]
    pub async fn invalidate_all(&self, scan: InvalidateScanOptions) -> Result<(), CacheError> {
        self.l0.invalidate_all();
        self.scan_delete_l1("cache:*", "all", scan).await
    }

    /// Invalidate cache entries matching a prefix pattern in the cache key.
    #[tracing::instrument(skip(self, scan), fields(prefix = %prefix))]
    pub async fn invalidate_prefix(&self, prefix: &str, scan: InvalidateScanOptions) -> Result<(), CacheError> {
        let prefix_owned = prefix.to_string();
        if let Err(e) = self
            .l0
            .invalidate_entries_if(move |k, _| k.starts_with(&prefix_owned))
        {
            warn!(error = %e, prefix = prefix, "L0 invalidate_entries_if failed during invalidate_prefix");
        }

        let pattern = format!("cache:{prefix}*");
        self.scan_delete_l1(&pattern, prefix, scan).await
    }

    async fn scan_delete_l1(
        &self,
        pattern: &str,
        scope_label: &str,
        scan: InvalidateScanOptions,
    ) -> Result<(), CacheError> {
        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    error = %e,
                    scope = scope_label,
                    "Failed to get Redis connection for cache invalidation scan"
                );
                return Err(CacheError::Operation(e.to_string()));
            }
        };

        let started = Instant::now();
        let mut cursor = 0u64;
        let batch_size = 100i64;
        let mut total = 0u64;
        let mut batch_count = 0u64;
        let mut timed_out = false;

        loop {
            if let Some(max) = scan.max_duration
                && started.elapsed() >= max
            {
                timed_out = true;
                warn!(
                    scope = scope_label,
                    deleted_so_far = total,
                    elapsed_secs = started.elapsed().as_secs(),
                    "Cache invalidation scan timed out; L0 already cleared, L1 may be partially deleted"
                );
                break;
            }

            let result: (u64, Vec<String>) = cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(pattern)
                .arg("COUNT")
                .arg(batch_size)
                .query_async(&mut *conn)
                .await
                .map_err(|e| {
                    warn!(
                        error = %e,
                        scope = scope_label,
                        "Redis SCAN failed during cache invalidation"
                    );
                    CacheError::Operation(format!("Redis SCAN failed: {e}"))
                })?;

            cursor = result.0;
            let keys = result.1;
            batch_count += 1;

            if !keys.is_empty() {
                let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
                match cmd("DEL")
                    .arg(&key_refs)
                    .query_async::<()>(&mut *conn)
                    .await
                {
                    Ok(_) => {
                        total += keys.len() as u64;
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            count = keys.len(),
                            scope = scope_label,
                            "Redis DEL batch failed during cache invalidation"
                        );
                    }
                }
            }

            if batch_count.is_multiple_of(SCAN_PROGRESS_EVERY_BATCHES) {
                info!(
                    scope = scope_label,
                    deleted_so_far = total,
                    batches = batch_count,
                    "Cache invalidation scan in progress"
                );
            }

            if cursor == 0 {
                break;
            }
        }

        if timed_out {
            warn!(
                scope = scope_label,
                deleted_so_far = total,
                "Cache invalidation scan ended early due to timeout"
            );
        } else {
            info!(
                scope = scope_label,
                total_deleted = total,
                "Cache invalidation scan completed"
            );
        }

        debug!(
            scope = scope_label,
            total_deleted = total,
            "Cache invalidation finished"
        );
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
