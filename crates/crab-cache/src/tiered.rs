use crate::{CacheEntry, L0Config, TtlConfig};
use arc_swap::ArcSwap;
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use crab_metrics::{CacheTier, global_metrics};
use moka::future::Cache;
use moka::policy::Expiry;
use parking_lot::RwLock;
use redis::{AsyncCommands, cmd};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CacheGetMetrics {
    /// Record L0/L1 hits and tiered miss on absent.
    Full,
    /// Record L0/L1 hits only; caller records miss after optional L2 lookup.
    HitsOnly,
    /// No Prometheus cache counters (coalesce follower probe).
    Silent,
}

impl CacheGetMetrics {
    const fn record_hits(self) -> bool {
        matches!(self, Self::Full | Self::HitsOnly)
    }

    const fn record_miss(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// Custom Moka expiry that resolves per-entry TTL from the shared `TtlConfig`,
/// using `ArcSwap` for lock-free reads on the hot path.
///
/// `expire_after_read` returns `None` so that cache reads do NOT extend the entry's lifetime
/// (the TTL set at creation/update time is authoritative; Management API TTL decreases are
/// picked up via `expire_after_update` and on the next write-through from L1).
struct DynamicTtlExpiry {
    ttl_config: Arc<ArcSwap<TtlConfig>>,
}

impl DynamicTtlExpiry {
    fn resolve_ttl(&self, model: &str) -> Option<Duration> {
        let config = self.ttl_config.load();
        Some(Duration::from_secs(config.resolve(model, None)))
    }
}

impl Expiry<String, CacheEntry> for DynamicTtlExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CacheEntry,
        _created_at: Instant,
    ) -> Option<Duration> {
        self.resolve_ttl(&value.model)
    }

    fn expire_after_read(
        &self,
        _key: &String,
        _value: &CacheEntry,
        _read_at: Instant,
        _duration_until_expiry: Option<Duration>,
        _last_modified_at: Instant,
    ) -> Option<Duration> {
        // Do NOT extend TTL on read — the entry expires at its originally-set time.
        None
    }

    fn expire_after_update(
        &self,
        _key: &String,
        value: &CacheEntry,
        _updated_at: Instant,
        _duration_until_expiry: Option<Duration>,
    ) -> Option<Duration> {
        self.resolve_ttl(&value.model)
    }
}

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
    ttl_config: Arc<ArcSwap<TtlConfig>>,
    /// Retained for backward-compat API surface that exposes the old RwLock handle.
    ttl_config_rwlock: Arc<RwLock<TtlConfig>>,
}

impl TieredCache {
    pub async fn new(
        l1_pool: Pool<RedisConnectionManager>,
        l0_config: L0Config,
        ttl_config: Arc<RwLock<TtlConfig>>,
    ) -> Result<Self, CacheError> {
        let arc_swap = Arc::new(ArcSwap::from_pointee(ttl_config.read().clone()));

        let l0 = Cache::builder()
            .max_capacity(l0_config.max_capacity)
            .expire_after(DynamicTtlExpiry {
                ttl_config: arc_swap.clone(),
            })
            .support_invalidation_closures()
            .build();

        Ok(Self {
            l0,
            l1_pool,
            ttl_config: arc_swap,
            ttl_config_rwlock: ttl_config,
        })
    }

    /// Lookup cache without incrementing Prometheus counters (e.g. coalesce follower replay).
    pub async fn get_silent(&self, key: &str) -> Option<(CacheEntry, CacheTier)> {
        self.get_inner(key, None, None, CacheGetMetrics::Silent)
            .await
    }

    /// L0/L1 lookup: record hits immediately; defer tiered miss until caller confirms L2 also missed.
    pub async fn get_defer_miss(
        &self,
        key: &str,
        consumer: Option<&str>,
        domain: Option<&str>,
    ) -> Option<(CacheEntry, CacheTier)> {
        self.get_inner(key, consumer, domain, CacheGetMetrics::HitsOnly)
            .await
    }

    /// Record a tiered cache miss after L0/L1 absent and L2 semantic lookup failed.
    pub fn record_absent_miss(&self, domain: Option<&str>) {
        global_metrics().record_cache_miss(CacheTier::Miss, domain);
    }

    #[tracing::instrument(skip(self), fields(key = %key, consumer = consumer.map(|c| c).unwrap_or("none")))]
    pub async fn get(
        &self,
        key: &str,
        consumer: Option<&str>,
        domain: Option<&str>,
    ) -> Option<(CacheEntry, CacheTier)> {
        self.get_inner(key, consumer, domain, CacheGetMetrics::Full)
            .await
    }

    async fn get_inner(
        &self,
        key: &str,
        consumer: Option<&str>,
        domain: Option<&str>,
        metrics: CacheGetMetrics,
    ) -> Option<(CacheEntry, CacheTier)> {
        let record_hits = metrics.record_hits();
        let record_miss = metrics.record_miss();
        if let Some(entry) = self.l0.get(key).await {
            if record_hits {
                global_metrics().record_cache_hit(
                    CacheTier::L0Moka,
                    &entry.model,
                    consumer,
                    domain,
                );
            }
            debug!(key = key, tier = "L0", "Cache hit");
            return Some((entry, CacheTier::L0Moka));
        }

        let mut conn = match self.l1_pool.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, key = key, "Failed to get Redis connection for cache get");
                if record_miss {
                    global_metrics().record_cache_miss(CacheTier::Miss, domain);
                }
                return None;
            }
        };

        let result: Option<String> = match conn.get(format!("cache:{key}")).await {
            Ok(val) => val,
            Err(e) => {
                warn!(error = %e, key = key, "Redis GET failed for cache key");
                if record_miss {
                    global_metrics().record_cache_miss(CacheTier::Miss, domain);
                }
                return None;
            }
        };

        if let Some(json) = result {
            match serde_json::from_str::<CacheEntry>(&json) {
                Ok(entry) => {
                    if record_hits {
                        global_metrics().record_cache_hit(
                            CacheTier::L1Redis,
                            &entry.model,
                            consumer,
                            domain,
                        );
                    }
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

        if record_miss {
            global_metrics().record_cache_miss(CacheTier::Miss, domain);
        }
        debug!(key = key, "Cache miss");
        None
    }

    #[tracing::instrument(skip(self, entry), fields(key = %key, model = %model, consumer = consumer.map(|c| c).unwrap_or("none")))]
    pub async fn put(
        &self,
        key: &str,
        entry: CacheEntry,
        model: &str,
        consumer: Option<&str>,
    ) -> Result<(), CacheError> {
        let ttl = self.ttl_config.load().resolve(model, consumer);

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
        self.ttl_config.load().resolve(model, consumer)
    }

    /// Sync the internal ArcSwap from the external RwLock.
    ///
    /// Call this after updating `ttl_config()` via the management API to ensure
    /// the lock-free read path in `DynamicTtlExpiry` picks up the new config.
    pub fn sync_ttl(&self) {
        let snapshot = self.ttl_config_rwlock.read().clone();
        self.ttl_config.store(Arc::new(snapshot));
    }

    pub fn ttl_config(&self) -> Arc<parking_lot::RwLock<TtlConfig>> {
        self.ttl_config_rwlock.clone()
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
    pub async fn invalidate_prefix(
        &self,
        prefix: &str,
        scan: InvalidateScanOptions,
    ) -> Result<(), CacheError> {
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

    /// Scan and delete L1 (Redis) entries matching the given pattern.
    ///
    /// # Redis Cluster Compatibility
    ///
    /// **WARNING**: This implementation uses `SCAN` which is a single-node operation.
    /// In Redis Cluster mode, `SCAN` only scans the current node and cannot cross
    /// slots, so keys on other nodes will be missed. If Redis is migrated to Cluster
    /// mode, this method will silently return success but only partially delete data.
    ///
    /// Long-term mitigations:
    /// - Embed `{hash_tag}` in keys to ensure same-prefix keys land on the same slot
    /// - Use Redis 7.0+ `SCAN` with `TYPE` filter to reduce irrelevant key scanning
    /// - Consider `UNLINK` (non-blocking DEL) for large batches
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
