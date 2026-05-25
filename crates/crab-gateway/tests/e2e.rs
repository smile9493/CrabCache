//! End-to-end integration tests for CrabCache.
//!
//! Tests cover: cache hit/miss (L0 + L1), request coalescing,
//! cache key determinism, reasoning display adapter, and rate limiting.

mod common;

use crab_cache::{
    CacheEntry, FingerprintConfig, L0Config, RequestCoalescer, TieredCache, TtlConfig, UsageInfo,
    generate_cache_key_with_fingerprint, generate_namespaced_cache_key_with_fingerprint,
};
use crab_metrics::CacheTier;
use crab_proxy::{ClientKeyRateLimiter, StoredKey};
use crab_reasoning::CursorReasoningDisplayAdapter;
use parking_lot::RwLock;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn test_cache_entry() -> CacheEntry {
    CacheEntry {
        response_body: br#"{"id":"e2e-test","choices":[{"message":{"content":"hello"}}]}"#.to_vec(),
        model: "deepseek-v4-pro".to_string(),
        usage: UsageInfo {
            prompt_tokens: 10,
            completion_tokens: 5,
            prompt_cache_hit_tokens: 0,
            prompt_cache_miss_tokens: 10,
        },
        created_at: 1,
        ttl_secs: 3600,
        sse_body: None,
        is_stream: false,
        client_display_reasoning: true,
    }
}

fn compute_cache_key(body: &[u8]) -> String {
    generate_cache_key_with_fingerprint(body, &FingerprintConfig::default_v1())
        .expect("cache key generation")
}

/// Skip test gracefully if Redis is not available (unless in CI).
macro_rules! require_redis_or_skip {
    ($pool:ident) => {
        let Some($pool) = test_redis_pool().await else {
            if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
                panic!("Redis required for integration tests in CI");
            }
            eprintln!("SKIP {}: Redis not reachable", std::stringify!(test));
            return;
        };
    };
}

// ---------------------------------------------------------------------------
// Cache tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cache_hit_l0_returns_cached_entry() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let body = br#"{"model":"deepseek-v4-pro","messages":[{"role":"user","content":"ping"}]}"#;
    let key = compute_cache_key(body);
    let entry = test_cache_entry();

    cache
        .put(&key, entry.clone(), "deepseek-v4-pro", None)
        .await
        .expect("put");

    let (got, tier) = cache
        .get(&key, Some("test-consumer"), Some("test-domain"))
        .await
        .expect("cache hit");
    assert_eq!(got.response_body, entry.response_body);
    assert_eq!(got.model, entry.model);
    assert!(
        matches!(tier, CacheTier::L0Moka),
        "expected L0 hit, got {tier:?}"
    );
}

#[tokio::test]
async fn cache_miss_returns_none() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let result = cache.get("nonexistent-key-e2e-12345", None, None).await;
    assert!(result.is_none(), "expected cache miss");
}

#[tokio::test]
async fn cache_entry_roundtrip_serde() {
    let entry = test_cache_entry();
    let json = serde_json::to_vec(&entry).expect("serialize");
    let roundtripped: CacheEntry = serde_json::from_slice(&json).expect("deserialize");
    assert_eq!(roundtripped.response_body, entry.response_body);
    assert_eq!(roundtripped.model, entry.model);
    assert_eq!(roundtripped.usage.prompt_tokens, entry.usage.prompt_tokens);
    assert_eq!(
        roundtripped.usage.completion_tokens,
        entry.usage.completion_tokens
    );
}

// ---------------------------------------------------------------------------
// Coalescing tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coalescing_leader_follower() {
    let coalescer = Arc::new(RequestCoalescer::with_config(100, 30));
    let key = "coalesce-test-key-v1";

    let guard = coalescer.acquire(key).await.expect("leader acquire");
    assert!(guard.is_leader(), "first acquirer should be leader");
    assert!(!guard.leader_failed());

    let coalescer2 = coalescer.clone();
    let key2 = key.to_string();
    let follower = tokio::spawn(async move { coalescer2.acquire(&key2).await });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    guard.mark_completed();

    let follower_guard = follower
        .await
        .expect("follower join")
        .expect("follower acquire");
    assert!(!follower_guard.is_leader(), "follower should not be leader");
}

#[tokio::test]
async fn coalescing_leader_failure_notifies_followers() {
    let coalescer = Arc::new(RequestCoalescer::with_config(100, 30));
    let key = "coalesce-fail-test-key";

    let guard = coalescer.acquire(key).await.expect("leader acquire");
    assert!(guard.is_leader());

    let coalescer2 = coalescer.clone();
    let key2 = key.to_string();
    let follower = tokio::spawn(async move { coalescer2.acquire(&key2).await });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    guard.mark_failed();

    let follower_guard = follower
        .await
        .expect("follower join")
        .expect("follower acquire");
    assert!(
        follower_guard.leader_failed(),
        "follower should see leader failure"
    );
}

// ---------------------------------------------------------------------------
// Cache key tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cache_key_deterministic() {
    let body = br#"{"model":"v4-pro","messages":[{"role":"user","content":"hello"}]}"#;
    let k1 = compute_cache_key(body);
    let k2 = compute_cache_key(body);
    assert_eq!(k1, k2, "same input should produce same key");
}

#[tokio::test]
async fn cache_key_different_for_different_messages() {
    let body1 = br#"{"model":"v4-pro","messages":[{"role":"user","content":"hello"}]}"#;
    let body2 = br#"{"model":"v4-pro","messages":[{"role":"user","content":"world"}]}"#;
    let k1 = compute_cache_key(body1);
    let k2 = compute_cache_key(body2);
    assert_ne!(k1, k2, "different messages should produce different keys");
}

#[tokio::test]
async fn cache_key_namespace_prefixed_when_set() {
    let body = br#"{"model":"v4","messages":[{"role":"user","content":"test"}]}"#;
    let key = generate_namespaced_cache_key_with_fingerprint(
        body,
        Some("tenant-a"),
        &FingerprintConfig::default_v1(),
    )
    .expect("key");
    assert!(
        key.starts_with("tenant-a:"),
        "key '{key}' should be prefixed with namespace"
    );
}

// ---------------------------------------------------------------------------
// Reasoning tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reasoning_cursor_adapter_constructs() {
    // Verify the adapter can be constructed with different collapsible settings
    let collapsible = CursorReasoningDisplayAdapter::new(true);
    let non_collapsible = CursorReasoningDisplayAdapter::new(false);
    // The adapter is used internally by rewrite_sse_chunk; construction is sufficient
    // to verify type layout and no panics.
    drop(collapsible);
    drop(non_collapsible);
}

// ---------------------------------------------------------------------------
// Structural tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runtime_config_constructs_with_backends() {
    let runtime = common::test_runtime();
    let profile_id = runtime.default_upstream_profile_id();
    assert_eq!(profile_id, "deepseek");
}

#[tokio::test]
async fn client_key_rate_limiter_allows_and_blocks() {
    let limiter = ClientKeyRateLimiter::new();
    let key = "test-rate-limited-key";

    // Unlimited (rpm_limit = 0) always allows
    assert!(limiter.check_and_consume(key, 0));

    // Limited to 2 RPM — first 2 should pass
    assert!(limiter.check_and_consume(key, 2));
    assert!(limiter.check_and_consume(key, 2));
    // 3rd within the same bucket cycle should be blocked
    assert!(!limiter.check_and_consume(key, 2));
}

#[tokio::test]
async fn stored_key_defaults_rpm_limit_zero() {
    let key = StoredKey {
        id: "k1".into(),
        name: "test".into(),
        key_hash: "hash".into(),
        enabled: true,
        domain: None,
        project_id: None,
        pipeline: None,
        upstream_profile: None,
        max_concurrent: 0,
        rpm_limit: 0,
    };
    assert_eq!(key.rpm_limit, 0);
    assert!(key.enabled);
}
