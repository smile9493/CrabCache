//! Integration tests for the gateway management HTTP API.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crab_cache::{FingerprintConfig, L0Config, TieredCache, TtlConfig};
use crab_control::{
    CACHE_INVALIDATE_CONFIRM_ALL, CACHE_INVALIDATE_CONFIRM_HEADER, GATEWAY_ADMIN_KEY_HEADER,
};
use crab_gateway::management::{ManagementState, router};
use crab_pipeline::{PipelineGlobals, PipelineMode, UpstreamProvider};
use crab_proxy::{
    ClientKeyLimiter, ConnectionConfig, ReasoningConfig, RuntimeConfig, UpstreamKeyPool,
    UpstreamProfileRuntime,
};
use crab_reasoning::ReasoningBackend;
use std::collections::HashMap;
use crab_state::{RedisStateConfig, RedisStateStore, apply_snapshot_to_runtime};
use crab_route::AffinityRouter;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tower::ServiceExt;

fn test_runtime() -> Arc<RuntimeConfig> {
    let backends = crab_control::parse_backend_endpoints(
        &["127.0.0.1:443".to_string()],
        1,
        "api.deepseek.com",
    )
    .unwrap();
    let router = AffinityRouter::new(&backends).unwrap();
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let upstream_pool =
        UpstreamKeyPool::from_secrets(vec!["sk-upstream-test-key-12345678".into()], 60);
    let pool_handle = Arc::new(RwLock::new(upstream_pool));
    let mut profiles = HashMap::new();
    profiles.insert(
        "deepseek".to_string(),
        Arc::new(UpstreamProfileRuntime {
            id: "deepseek".to_string(),
            provider: UpstreamProvider::Deepseek,
            base_url: "https://api.deepseek.com".to_string(),
            fallback_model: "deepseek-v4-pro".to_string(),
            tls_sni: "api.deepseek.com".to_string(),
            router: AffinityRouter::new(&backends).unwrap(),
            upstream_pool: pool_handle.clone(),
        }),
    );
    RuntimeConfig::new(
        router,
        ttl,
        ConnectionConfig::default(),
        true,
        FingerprintConfig::default(),
        "https://api.deepseek.com".to_string(),
        "deepseek-v4-pro".to_string(),
        pool_handle,
        profiles,
        "deepseek".to_string(),
        PipelineGlobals::default(),
        false,
        std::collections::HashSet::new(),
    )
}

async fn test_management_state() -> Option<ManagementState> {
    let redis_url = std::env::var("CRABCACHE_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let pool = bb8::Pool::builder()
        .max_size(1)
        .connection_timeout(std::time::Duration::from_secs(2))
        .build(bb8_redis::RedisConnectionManager::new(redis_url).ok()?)
        .await
        .ok()?;
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let tiered_cache = Arc::new(
        TieredCache::new(pool, L0Config::default(), ttl)
            .await
            .ok()?,
    );

    let reasoning_store = Arc::new(
        ReasoningBackend::open_sqlite(":memory:", Some(3600), Some(1000)).expect("reasoning store"),
    );

    Some(ManagementState {
        runtime: test_runtime(),
        tiered_cache,
        reasoning_store,
        reasoning_config: Arc::new(RwLock::new(ReasoningConfig::default())),
        admin_key: "test-admin".to_string(),
        state_store: None,
        invalidate_all_in_progress: Arc::new(AtomicBool::new(false)),
        invalidate_job: Arc::new(Mutex::new(None)),
        invalidate_rate: Arc::new(Mutex::new(
            crab_gateway::management::InvalidateRateState::default(),
        )),
        invalidate_scan_timeout_secs: 300,
        client_key_limiter: ClientKeyLimiter::new(),
    })
}

fn redis_required_in_ci() -> bool {
    std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok()
}

async fn require_management_state() -> Option<ManagementState> {
    let state = test_management_state().await?;
    if state.tiered_cache.ping().await {
        Some(state)
    } else {
        None
    }
}

fn skip_or_panic_redis_unavailable() {
    if redis_required_in_ci() {
        panic!("Redis required for management API integration tests in CI");
    }
    eprintln!("SKIP: Redis not reachable");
}

#[tokio::test]
async fn health_without_auth() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ready_returns_ok_when_redis_up() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ready"], true);
    assert_eq!(json["redis"], "ok");
}

#[tokio::test]
async fn create_and_list_keys() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"ci-key","enabled":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);

    let list = app
        .oneshot(
            Request::builder()
                .uri("/v1/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
}

#[tokio::test]
async fn create_key_with_project_id_roundtrip() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"project-key","enabled":true,"project_id":"proj_alpha"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(create.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        created["project_id"].as_str(),
        Some("proj_alpha")
    );

    let patch = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/v1/keys/{}",
                    created["key_full"].as_str().expect("key_full")
                ))
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"project_id":"proj_beta"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(patch.into_body(), usize::MAX)
        .await
        .unwrap();
    let patched: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        patched["project_id"].as_str(),
        Some("proj_beta")
    );
}

#[tokio::test]
async fn create_key_with_domain_roundtrip() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"name":"domain-key","enabled":true,"domain":"backend-team"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(create.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        created["domain"].as_str(),
        Some("backend-team")
    );

    let list = app
        .oneshot(
            Request::builder()
                .uri("/v1/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(list.into_body(), usize::MAX)
        .await
        .unwrap();
    let keys: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert!(
        keys.iter()
            .any(|k| k["domain"].as_str() == Some("backend-team")),
        "listed key should include domain"
    );
}

#[tokio::test]
async fn rejects_missing_admin_key() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalidate_all_requires_confirm_header() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/cache/invalidate")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scope":"all"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invalidate_all_accepts_with_confirm_header() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/cache/invalidate")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header(
                    CACHE_INVALIDATE_CONFIRM_HEADER,
                    CACHE_INVALIDATE_CONFIRM_ALL,
                )
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scope":"all"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_invalidate_status_without_job() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/cache/invalidate/status")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["all_in_progress"], false);
    assert!(json["job"].is_null());
}

#[tokio::test]
async fn get_fingerprint_returns_runtime_config() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/cache/fingerprint")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn upstream_keys_list_and_replace() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/upstream/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/upstream/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"keys":[{"id":"k-a","secret":"sk-aaaaaaaaaaaaaaaa","enabled":true},{"id":"k-b","secret":"sk-bbbbbbbbbbbbbbbb","enabled":true}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["keys"].as_array().map(|a| a.len()), Some(2));
}

#[tokio::test]
async fn status_includes_upstream_key_fields() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("upstream_key_count").is_some());
    assert!(json.get("upstream_keys_available").is_some());
    assert_eq!(json["upstream_key_count"].as_u64(), Some(1));
}

#[tokio::test]
async fn put_upstream_relay_updates_model() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let body = serde_json::json!({
        "base_url": "https://api.deepseek.com",
        "model": "deepseek-chat",
        "endpoints": ["api.deepseek.com:443"]
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/upstream/relay")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["model"].as_str(), Some("deepseek-chat"));
}

#[tokio::test]
async fn put_upstream_keys_append_mode() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let body = serde_json::json!({
        "mode": "append",
        "keys": [{"secret": "sk-second-key-1234567890", "enabled": true}]
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/upstream/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["keys"].as_array().map(|a| a.len()), Some(2));
}

#[tokio::test]
async fn upstream_pool_all_cooled_returns_unavailable() {
    let pool = UpstreamKeyPool::from_secrets(vec!["sk-test-key-1234567890".into()], 1);
    pool.report_rate_limited("key-1");
    assert!(pool.acquire().is_none());
    assert_eq!(pool.available_count(), 0);
}

/// Client API keys written via Management API are visible after reload from Redis (multi-instance).
#[tokio::test]
async fn client_key_persisted_in_redis_state() {
    let Some(mut state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let redis_url = std::env::var("CRABCACHE_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let store = match RedisStateStore::connect(&RedisStateConfig::new(
        redis_url,
        format!("crab:state:test:{}", uuid::Uuid::new_v4()),
    ))
    .await
    {
        Ok(s) => Arc::new(s),
        Err(_) => {
            skip_or_panic_redis_unavailable();
            return;
        }
    };
    state.state_store = Some(store.clone());

    let app = router(state.clone());
    let body = serde_json::json!({"name": "redis-test", "enabled": true});
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/keys")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let token = created["key_full"].as_str().expect("key_full");

    let (_, snap) = store.load_all().await.expect("load redis state");
    assert!(
        snap.keys.contains_key(token),
        "created key should be in Redis control plane"
    );

    let runtime_b = test_runtime();
    runtime_b.keys.clear();
    apply_snapshot_to_runtime(&runtime_b, &snap, 60).expect("apply snapshot");
    assert!(
        runtime_b.keys.contains_key(token),
        "second runtime should see key after Redis reload"
    );
}

/// Domain policies written via Management API are visible after reload from Redis.
#[tokio::test]
async fn domain_policies_persisted_in_redis_state() {
    let Some(mut state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let redis_url = std::env::var("CRABCACHE_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let store = match RedisStateStore::connect(&RedisStateConfig::new(
        redis_url,
        format!("crab:state:test:{}", uuid::Uuid::new_v4()),
    ))
    .await
    {
        Ok(s) => Arc::new(s),
        Err(_) => {
            skip_or_panic_redis_unavailable();
            return;
        }
    };
    state.state_store = Some(store.clone());

    let app = router(state.clone());
    let body = serde_json::json!({
        "policies": [{
            "domain": "persist-team",
            "monthly_token_budget": 500000,
            "monthly_cost_budget_usd": 25.0,
            "min_hit_rate": 0.9,
            "enabled": true
        }]
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/domains/policies")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let (_, snap) = store.load_all().await.expect("load redis state");
    assert!(
        snap.domain_policies.contains_key("persist-team"),
        "domain policy should be in Redis control plane"
    );
    assert_eq!(
        snap.domain_policies["persist-team"].monthly_token_budget,
        500_000
    );

    let runtime_b = test_runtime();
    apply_snapshot_to_runtime(&runtime_b, &snap, 60).expect("apply snapshot");
    let policies = runtime_b.list_domain_policies();
    assert!(
        policies.iter().any(|(d, _)| d == "persist-team"),
        "second runtime should see domain policy after Redis reload"
    );
}

#[test]
fn pipeline_runtime_set_and_read() {
    let runtime = test_runtime();
    assert_eq!(
        runtime.pipeline_globals().pipeline_mode,
        PipelineMode::Auto
    );
    runtime
        .set_pipeline_runtime(PipelineMode::ForceCursorV4, "deepseek")
        .expect("set pipeline");
    assert_eq!(
        runtime.pipeline_globals().pipeline_mode,
        PipelineMode::ForceCursorV4
    );
    assert_eq!(runtime.default_upstream_profile_id(), "deepseek");
    assert!(runtime
        .set_pipeline_runtime(PipelineMode::Auto, "unknown")
        .is_err());
}

#[tokio::test]
async fn pipeline_runtime_http_roundtrip() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/runtime/pipeline")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let view: crab_control::PipelineRuntimeConfigView = serde_json::from_slice(&body).unwrap();
    assert_eq!(view.pipeline_mode, "auto");
    assert!(!view.profiles.is_empty());

    let put_body = serde_json::json!({
        "pipeline_mode": "force_cursor_v4",
        "default_upstream_profile": "deepseek",
        "profiles": view.profiles,
    });
    let put_resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/runtime/pipeline")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(put_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated: crab_control::PipelineRuntimeConfigView = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated.pipeline_mode, "force_cursor_v4");
}

#[tokio::test]
async fn cursor_models_http_roundtrip() {
    let Some(state) = require_management_state().await else {
        skip_or_panic_redis_unavailable();
        return;
    };
    let app = router(state);

    let put_body = serde_json::json!({
        "force_deepseek_profile_for_aliases": true,
        "synthetic_models_enabled": false,
        "aliases": {
            "gpt-4o": {
                "upstream": "deepseek-v4-pro",
                "pipeline": "cursor_deepseek_v4"
            }
        }
    });
    let put_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/cursor/models")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .header("content-type", "application/json")
                .body(Body::from(put_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_resp.status(), StatusCode::OK);

    let get_resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/cursor/models")
                .header(GATEWAY_ADMIN_KEY_HEADER, "test-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let view: crab_control::CursorModelsConfigView = serde_json::from_slice(&body).unwrap();
    assert!(view.aliases.contains_key("gpt-4o"));
    assert_eq!(view.aliases["gpt-4o"].upstream, "deepseek-v4-pro");
}
