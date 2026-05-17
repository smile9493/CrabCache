//! Integration tests for the gateway management HTTP API.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crab_cache::{FingerprintConfig, L0Config, TieredCache, TtlConfig};
use crab_control::{
    CACHE_INVALIDATE_CONFIRM_ALL, CACHE_INVALIDATE_CONFIRM_HEADER, GATEWAY_ADMIN_KEY_HEADER,
};
use crab_gateway::management::{router, ManagementState};
use crab_proxy::{ConnectionConfig, RuntimeConfig};
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
    RuntimeConfig::new(
        router,
        ttl,
        ConnectionConfig::default(),
        true,
        FingerprintConfig::default(),
        "https://api.deepseek.com".to_string(),
        "deepseek-v4-pro".to_string(),
        "sk-bootstrap".to_string(),
    )
}

async fn test_management_state() -> Option<ManagementState> {
    let redis_url =
        std::env::var("CRABCACHE_TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let pool = bb8::Pool::builder()
        .max_size(1)
        .connection_timeout(std::time::Duration::from_secs(2))
        .build(
            bb8_redis::RedisConnectionManager::new(redis_url).ok()?,
        )
        .await
        .ok()?;
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let tiered_cache = Arc::new(TieredCache::new(pool, L0Config::default(), ttl).await.ok()?);

    Some(ManagementState {
        runtime: test_runtime(),
        tiered_cache,
        admin_key: "test-admin".to_string(),
        invalidate_all_in_progress: Arc::new(AtomicBool::new(false)),
        invalidate_job: Arc::new(Mutex::new(None)),
        invalidate_rate: Arc::new(Mutex::new(crab_gateway::management::InvalidateRateState::default())),
        invalidate_scan_timeout_secs: 300,
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
                .body(Body::from(
                    r#"{"name":"ci-key","enabled":true}"#,
                ))
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
                .header(CACHE_INVALIDATE_CONFIRM_HEADER, CACHE_INVALIDATE_CONFIRM_ALL)
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
