//! Data-plane integration tests for CrabCache.
//!
//! Tests cover: prefix L0 index warm-up, exact key roundtrip,
//! and early MiMo cache key determinism.
//! Requires Redis when `CI` or `GITHUB_ACTIONS` env is set.

mod common;

use crab_cache::{
    CacheEntry, FingerprintConfig, L0Config, TieredCache, TtlConfig, UsageInfo,
    generate_cache_key_with_fingerprint,
};
use crab_metrics::CacheTier;
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

fn make_entry(body: &[u8], model: &str) -> CacheEntry {
    CacheEntry {
        response_body: body.to_vec(),
        model: model.to_string(),
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

macro_rules! require_redis_or_skip {
    ($pool:ident) => {
        let Some($pool) = test_redis_pool().await else {
            if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
                panic!("Redis required for data-plane tests in CI");
            }
            eprintln!("SKIP {}: Redis not reachable", std::stringify!(test));
            return;
        };
    };
}

// ---------------------------------------------------------------------------
// Prefix L0 index: update → prefix_l0_lookup returns L0 entry
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prefix_l0_lookup_after_index_update() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    // Write an exact key to L0 (put propagates to L0 + L1).
    let body = br#"{"model":"mimo-v2","messages":[{"role":"system","content":"You are helpful."},{"role":"user","content":"msg1"}]}"#;
    let key = compute_cache_key(body);
    let entry = make_entry(body, "mimo-v2");

    cache
        .put(&key, entry.clone(), "mimo-v2", None)
        .await
        .expect("put");

    // Compute a prefix hash (simulates messages[0..n-1] hash).
    // In proxy code, prefix_hash = SHA256(normalized_messages_prefix).
    // Here we just use the same key as both full_key and prefix_hash stand-in,
    // since we only test that prefix_index → L0 entry linkage works.
    let prefix_hash = format!("prefix:{}", &key[..32]);
    assert!(
        cache.prefix_l0_lookup(&prefix_hash).await.is_none(),
        "prefix should miss before it is registered"
    );
    cache.update_prefix_index(&prefix_hash, &key);

    // prefix_l0_lookup should resolve prefix → full_key → L0 entry.
    let found = cache
        .prefix_l0_lookup(&prefix_hash)
        .await
        .expect("prefix L0 hit");
    assert_eq!(found.response_body, entry.response_body);
    assert_eq!(found.model, entry.model);
    assert_eq!(found.usage.prompt_tokens, entry.usage.prompt_tokens);
    assert_eq!(found.usage.completion_tokens, entry.usage.completion_tokens);
    assert!(!found.is_stream);
}

#[tokio::test]
async fn prefix_l0_lookup_miss_when_prefix_not_registered() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let result = cache.prefix_l0_lookup("nonexistent-prefix").await;
    assert!(result.is_none(), "unregistered prefix should miss");
}

#[tokio::test]
async fn prefix_l0_clear_after_invalidation() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let body = br#"{"model":"mimo-v2","messages":[{"role":"user","content":"test"}]}"#;
    let key = compute_cache_key(body);
    let entry = make_entry(body, "mimo-v2");

    cache.put(&key, entry, "mimo-v2", None).await.expect("put");

    let prefix_hash = "prefix:clear-test";
    cache.update_prefix_index(prefix_hash, &key);
    assert!(cache.prefix_l0_lookup(prefix_hash).await.is_some());

    // clear_prefix_index should invalidate all prefix mappings.
    cache.clear_prefix_index();
    assert!(
        cache.prefix_l0_lookup(prefix_hash).await.is_none(),
        "after clear, prefix should miss"
    );
}

// ---------------------------------------------------------------------------
// Exact key roundtrip: put → get (simulates early MiMo exact cache path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exact_key_roundtrip_l0_hit() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    // Simulate the body that body_quick_parse extracts a model from.
    let body =
        br#"{"model":"mimo-v2","stream":true,"messages":[{"role":"user","content":"hello"}]}"#;
    let key = compute_cache_key(body);
    let entry = make_entry(body, "mimo-v2");

    cache
        .put(&key, entry.clone(), "mimo-v2", None)
        .await
        .expect("put");

    // Exact get — should hit L0 (Moka).
    let (got, tier) = cache
        .get(&key, Some("test-consumer"), Some("test-domain"))
        .await
        .expect("exact cache hit");
    assert_eq!(got.response_body, entry.response_body);
    assert_eq!(got.model, entry.model);
    assert_eq!(got.usage.prompt_tokens, entry.usage.prompt_tokens);
    assert_eq!(got.usage.completion_tokens, entry.usage.completion_tokens);
    assert_eq!(got.usage.prompt_cache_hit_tokens, entry.usage.prompt_cache_hit_tokens);
    assert_eq!(
        got.usage.prompt_cache_miss_tokens,
        entry.usage.prompt_cache_miss_tokens
    );
    assert_eq!(got.is_stream, entry.is_stream);
    assert!(
        matches!(tier, CacheTier::L0Moka),
        "expected L0 exact hit, got {tier:?}"
    );
}

#[tokio::test]
async fn exact_key_miss_for_different_body() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let body_a = br#"{"model":"mimo-v2","messages":[{"role":"user","content":"A"}]}"#;
    let body_b = br#"{"model":"mimo-v2","messages":[{"role":"user","content":"B"}]}"#;

    let key_a = compute_cache_key(body_a);
    let key_b = compute_cache_key(body_b);
    assert_ne!(key_a, key_b, "different bodies must produce different keys");

    let entry_a = make_entry(body_a, "mimo-v2");
    cache
        .put(&key_a, entry_a, "mimo-v2", None)
        .await
        .expect("put");

    // Key B should not hit.
    let result = cache.get(&key_b, None, None).await;
    assert!(result.is_none(), "different body key should miss");
}

#[tokio::test]
async fn exact_key_roundtrip_preserves_stream_flag() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let body = br#"{"model":"mimo-v2","stream":true,"messages":[{"role":"user","content":"stream test"}]}"#;
    let key = compute_cache_key(body);

    // Entry with is_stream = true and SSE body.
    let entry = CacheEntry {
        response_body: br#"data: {"choices":[{"delta":{"content":"hi"}}]}"#.to_vec(),
        model: "mimo-v2".to_string(),
        usage: UsageInfo {
            prompt_tokens: 5,
            completion_tokens: 2,
            prompt_cache_hit_tokens: 0,
            prompt_cache_miss_tokens: 5,
        },
        created_at: 1,
        ttl_secs: 3600,
        sse_body: Some(br#"data: {"choices":[{"delta":{"content":"hi"}}]}"#.to_vec()),
        is_stream: true,
        client_display_reasoning: false,
    };

    cache
        .put(&key, entry.clone(), "mimo-v2", None)
        .await
        .expect("put");

    let (got, _) = cache.get(&key, None, None).await.expect("hit");
    assert!(got.is_stream, "stream flag should survive roundtrip");
    assert!(got.sse_body.is_some(), "sse_body should survive roundtrip");
}

// ---------------------------------------------------------------------------
// Coalesce with exact_cache_probed flag (documented contract test)
// ---------------------------------------------------------------------------

/// The `exact_cache_probed` flag on GatewayContext prevents double-get when
/// coalescing. After first probe sets `exact_cache_probed = true`, the
/// follower path should not re-probe the cache.
///
/// This test documents the contract: a second `get` on the same key
/// returns the same entry (idempotent), confirming the flag is a safe
/// optimization rather than a correctness requirement.
#[tokio::test]
async fn coalesce_exact_cache_probed_idempotent_get() {
    require_redis_or_skip!(pool);

    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let cache = TieredCache::new(pool, L0Config::default(), ttl)
        .await
        .expect("tiered cache");

    let body = br#"{"model":"mimo-v2","messages":[{"role":"user","content":"coalesce"}]}"#;
    let key = compute_cache_key(body);
    let entry = make_entry(body, "mimo-v2");

    cache
        .put(&key, entry.clone(), "mimo-v2", None)
        .await
        .expect("put");

    // Simulate leader probe.
    let (got1, tier1) = cache.get(&key, None, None).await.expect("leader hit");
    assert!(matches!(tier1, CacheTier::L0Moka));

    // Simulate follower re-probe (same key, same result).
    let (got2, tier2) = cache.get(&key, None, None).await.expect("follower hit");
    assert!(matches!(tier2, CacheTier::L0Moka));
    assert_eq!(got1.response_body, got2.response_body);
    assert_eq!(got1.model, got2.model);
    assert_eq!(got1.usage.prompt_tokens, got2.usage.prompt_tokens);
    assert_eq!(got1.is_stream, got2.is_stream);
}

// ---------------------------------------------------------------------------
// MiMo prefix retirement (upstream body only; cache key unchanged)
// ---------------------------------------------------------------------------

#[test]
fn mimo_retire_prefix_shrinks_upstream_body() {
    use crab_reasoning::prepare_mimo_request;

    let mut messages = Vec::new();
    for i in 0..20 {
        messages.push(serde_json::json!({
            "role": if i % 2 == 0 { "user" } else { "assistant" },
            "content": format!("msg-{i}"),
        }));
    }
    let payload = serde_json::json!({
        "model": "mimo-v2.5-pro",
        "messages": messages,
    });
    let before = serde_json::to_vec(&payload).expect("serialize");
    let prepared = prepare_mimo_request(&payload, "mimo-v2.5-pro", true, 6);
    let after = serde_json::to_vec(&prepared.payload).expect("serialize upstream");
    assert!(prepared.retired_prefix_messages > 0);
    assert!(after.len() < before.len());
    // L0/L1 keys hash the client body (`before`), not the trimmed upstream payload (`after`).
    let cache_key_client = compute_cache_key(&before);
    let cache_key_upstream = compute_cache_key(&after);
    assert_ne!(cache_key_client, cache_key_upstream);
}

// ---------------------------------------------------------------------------
// MiMo session store merge (upstream messages only; cache key unchanged)
// ---------------------------------------------------------------------------

#[test]
fn session_store_merge_append_preserves_cache_key_material() {
    use crab_proxy::SessionStore;

    let stored = vec![
        serde_json::json!({"role":"user","content":"a"}),
        serde_json::json!({"role":"assistant","content":"b"}),
    ];
    let mut client = stored.clone();
    client.push(serde_json::json!({"role":"user","content":"c"}));
    let merged = SessionStore::merge_messages(&stored, &client).expect("append merge");
    assert_eq!(merged.len(), 3);
    let client_body =
        serde_json::to_vec(&serde_json::json!({"model":"mimo","messages": client})).unwrap();
    let merged_body =
        serde_json::to_vec(&serde_json::json!({"model":"mimo","messages": merged})).unwrap();
    // Same logical messages → same cache key (store must not replace original_request_body).
    assert_eq!(
        compute_cache_key(&client_body),
        compute_cache_key(&merged_body)
    );
}
