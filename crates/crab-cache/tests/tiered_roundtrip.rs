//! Integration test: TieredCache put → get roundtrip against Redis.

use crab_cache::{
    CacheEntry, FingerprintConfig, L0Config, TieredCache, TtlConfig, UsageInfo,
    generate_cache_key_with_fingerprint,
};
use crab_metrics::CacheTier;
use parking_lot::RwLock;
use std::sync::Arc;

async fn test_redis_pool() -> Option<bb8::Pool<bb8_redis::RedisConnectionManager>> {
    let redis_url = std::env::var("CRABCACHE_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let pool = bb8::Pool::builder()
        .max_size(1)
        .connection_timeout(std::time::Duration::from_secs(2))
        .build(bb8_redis::RedisConnectionManager::new(redis_url).ok()?)
        .await
        .ok()?;
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool.clone(), L0Config::default(), ttl)
        .await
        .ok()?;
    if cache.ping().await { Some(pool) } else { None }
}

#[tokio::test]
async fn tiered_cache_put_then_get_hit() {
    let Some(pool) = test_redis_pool().await else {
        if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
            panic!("Redis required for integration tests in CI (CRABCACHE_TEST_REDIS_URL)");
        }
        eprintln!(
            "SKIP tiered_cache_put_then_get_hit: Redis not reachable (set CRABCACHE_TEST_REDIS_URL or start Redis)"
        );
        return;
    };
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let body = br#"{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"ping"}]}"#;
    let key = generate_cache_key_with_fingerprint(body, &FingerprintConfig::default_v1())
        .expect("cache key");

    let entry = CacheEntry {
        response_body: br#"{"id":"cached"}"#.to_vec(),
        model: "deepseek-v4-pro".to_string(),
        usage: UsageInfo::default(),
        created_at: 1,
        ttl_secs: 3600,
        sse_body: None,
        is_stream: false,
        client_display_reasoning: true,
    };

    cache
        .put(&key, entry.clone(), "deepseek-v4-pro", None)
        .await
        .expect("put");

    let (got, tier) = cache
        .get(&key, Some("test-consumer"), Some("test-domain"))
        .await
        .expect("cache hit");
    assert_eq!(got.response_body, entry.response_body);
    assert!(matches!(tier, CacheTier::L0Moka | CacheTier::L1Redis));
}
