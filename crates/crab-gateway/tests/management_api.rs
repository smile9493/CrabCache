//! Integration tests for the gateway management HTTP API.
//! Requires Redis only when exercising full gateway; these tests use RuntimeConfig in-process.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use crab_cache::{FingerprintConfig, TtlConfig};
use crab_control::GATEWAY_ADMIN_KEY_HEADER;
use crab_gateway::management::{router, ManagementState};
use crab_proxy::{ConnectionConfig, RuntimeConfig};
use crab_route::AffinityRouter;
use std::sync::{Arc, RwLock};
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

#[tokio::test]
async fn health_without_auth() {
    let app = router(ManagementState {
        runtime: test_runtime(),
        admin_key: "test-admin".to_string(),
    });

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
async fn create_and_list_keys() {
    let app = router(ManagementState {
        runtime: test_runtime(),
        admin_key: "test-admin".to_string(),
    });

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
    let app = router(ManagementState {
        runtime: test_runtime(),
        admin_key: "test-admin".to_string(),
    });

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
