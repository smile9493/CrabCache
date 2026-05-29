use base64::Engine;
use crab_auth::oauth::codex::CodexAuthenticator;
use crab_auth::oauth::{AuthError, CodexDevicePollResult, claude, codex, common, gemini};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_pkce_verifier_length() {
    let verifier = common::generate_pkce_verifier();
    assert_eq!(verifier.len(), 128);
    assert!(
        verifier
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    );
}

#[test]
fn test_pkce_challenge_deterministic() {
    let verifier = "test_verifier_string";
    let challenge1 = common::generate_pkce_challenge(verifier);
    let challenge2 = common::generate_pkce_challenge(verifier);
    assert_eq!(challenge1, challenge2);
}

#[test]
fn test_random_state_hex() {
    let state = common::generate_random_state();
    assert_eq!(state.len(), 32);
    assert!(state.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_random_nonce_hex() {
    let nonce = common::generate_random_nonce();
    assert_eq!(nonce.len(), 32);
    assert!(nonce.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_codex_jwt_parsing() {
    let header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        r#"{"email":"test@example.com","chatgpt_account_id":"acct_123","chatgpt_plan_type":"pro","sub":"user_456"}"#,
    );
    let sig = "fake_signature";
    let jwt = format!("{header}.{payload}.{sig}");

    let claims = codex::parse_jwt_payload(&jwt).unwrap();
    assert_eq!(claims.email, Some("test@example.com".to_string()));
    assert_eq!(claims.account_id, Some("acct_123".to_string()));
    assert_eq!(claims.plan_type, Some("pro".to_string()));
}

#[test]
fn test_codex_jwt_parsing_nested_auth_claim() {
    let header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        r#"{"email":"nested@example.com","https://api.openai.com/auth":{"chatgpt_account_id":"acct_nested","chatgpt_plan_type":"plus"}}"#,
    );
    let jwt = format!("{header}.{payload}.sig");

    let claims = codex::parse_jwt_payload(&jwt).unwrap();
    assert_eq!(claims.email, Some("nested@example.com".to_string()));
    assert_eq!(claims.account_id, Some("acct_nested".to_string()));
    assert_eq!(claims.plan_type, Some("plus".to_string()));
}

#[test]
fn test_auth_url_construction_claude() {
    let verifier = "test_verifier";
    let challenge = common::generate_pkce_challenge(verifier);
    let state = "test_state_123";
    let url = claude::build_auth_url(&challenge, state, 54545);
    assert!(url.starts_with("https://claude.ai/oauth/authorize"));
    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=test_state_123"));
}

#[test]
fn test_auth_url_construction_codex() {
    let verifier = "test_verifier";
    let challenge = common::generate_pkce_challenge(verifier);
    let state = "test_state_123";
    let url = codex::build_auth_url(&challenge, state, 1455);
    assert!(url.starts_with("https://auth.openai.com/oauth/authorize"));
    assert!(url.contains("code_challenge="));
    assert!(!url.contains("code_verifier="));
}

#[test]
fn test_auth_url_construction_gemini() {
    let state = "test_state_123";
    let url = gemini::build_auth_url(state, 8085);
    assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth"));
    assert!(url.contains("access_type=offline"));
    assert!(url.contains("prompt=consent"));
}

#[test]
fn test_credential_filename_variants() {
    assert_eq!(
        codex::credential_filename("user@example.com", None, None),
        "codex-user@example.com.json"
    );
    assert_eq!(
        codex::credential_filename("user@example.com", Some("plus"), None),
        "codex-user@example.com-plus.json"
    );
    assert_eq!(
        codex::credential_filename("user@example.com", Some("team"), Some("abcd1234")),
        "codex-abcd1234-user@example.com-team.json"
    );
}

#[test]
fn test_parse_codex_device_poll_interval() {
    assert_eq!(
        codex::parse_codex_device_poll_interval(&serde_json::json!("7")),
        7
    );
    assert_eq!(
        codex::parse_codex_device_poll_interval(&serde_json::json!(10)),
        10
    );
    assert_eq!(
        codex::parse_codex_device_poll_interval(&serde_json::Value::Null),
        5
    );
}

#[test]
fn test_is_refresh_error_retryable() {
    assert!(!codex::is_refresh_error_retryable(&AuthError::TokenExchangeFailed {
        status: 400,
        body: r#"{"error":"refresh_token_reused"}"#.to_string(),
    }));
    assert!(codex::is_refresh_error_retryable(&AuthError::TokenExchangeFailed {
        status: 503,
        body: "upstream unavailable".to_string(),
    }));
}

#[tokio::test]
async fn test_retry_with_backoff_non_retryable_fails_fast() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let calls = AtomicU32::new(0);
    let result: Result<(), AuthError> = common::retry_with_backoff(3, |_| false, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(AuthError::OAuth("refresh_token_reused".into())) })
    })
    .await;

    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_codex_device_login_happy_path() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/usercode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_auth_id": "dev-auth-1",
            "user_code": "ABCD-1234",
            "interval": 1
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorization_code": "auth-code-xyz",
            "code_verifier": "verifier-from-device",
            "code_challenge": "challenge-from-device"
        })))
        .mount(&mock_server)
        .await;

    let header =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        r#"{"email":"device@example.com","https://api.openai.com/auth":{"chatgpt_account_id":"acct_dev","chatgpt_plan_type":"plus"}}"#,
    );
    let id_token = format!("{header}.{payload}.sig");

    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access-token",
            "refresh_token": "refresh-token",
            "id_token": id_token,
            "expires_in": 3600
        })))
        .mount(&mock_server)
        .await;

    let base = mock_server.uri();
    // SAFETY: test runs single-threaded; env overrides are scoped to this test.
    unsafe {
        std::env::set_var(
            "CRABCACHE_OAUTH_CODEX_DEVICE_USERCODE_URL",
            format!("{base}/api/accounts/deviceauth/usercode"),
        );
        std::env::set_var(
            "CRABCACHE_OAUTH_CODEX_DEVICE_TOKEN_URL",
            format!("{base}/api/accounts/deviceauth/token"),
        );
        std::env::set_var(
            "CRABCACHE_OAUTH_CODEX_TOKEN_URL",
            format!("{base}/oauth/token"),
        );
    }

    let record = CodexAuthenticator::new().device_login().await.unwrap();
    assert_eq!(record.access_token, "access-token");
    assert_eq!(record.email.as_deref(), Some("device@example.com"));
    assert!(record.id.contains("device@example.com"));

    unsafe {
        std::env::remove_var("CRABCACHE_OAUTH_CODEX_DEVICE_USERCODE_URL");
        std::env::remove_var("CRABCACHE_OAUTH_CODEX_DEVICE_TOKEN_URL");
        std::env::remove_var("CRABCACHE_OAUTH_CODEX_TOKEN_URL");
    }
}

#[tokio::test]
async fn test_start_device_usercode_with_url_success() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/usercode"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_auth_id": "dev-abc",
            "user_code": "WXYZ-9999",
            "interval": 3
        })))
        .mount(&mock_server)
        .await;

    let url = format!(
        "{}/api/accounts/deviceauth/usercode",
        mock_server.uri()
    );
    let start = CodexAuthenticator::start_device_usercode_with_url(&url)
        .await
        .unwrap();
    assert_eq!(start.device_auth_id, "dev-abc");
    assert_eq!(start.user_code, "WXYZ-9999");
    assert_eq!(start.poll_interval_secs, 3);
    assert_eq!(start.verify_url, "https://auth.openai.com/codex/device");
}

#[tokio::test]
async fn test_poll_device_once_pending_then_ready() {
    let mock_server = MockServer::start().await;

    // First poll returns 403 → Pending
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(serde_json::json!({"error": "authorization_pending"})),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&mock_server)
        .await;

    // Second poll returns 200 → Ready
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorization_code": "code123",
            "code_verifier": "verifier456"
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&mock_server)
        .await;

    let url = format!(
        "{}/api/accounts/deviceauth/token",
        mock_server.uri()
    );
    let r1 = CodexAuthenticator::poll_device_once_with_url(&url, "dev-1", "CODE-1")
        .await
        .unwrap();
    assert!(matches!(r1, CodexDevicePollResult::Pending));

    let r2 = CodexAuthenticator::poll_device_once_with_url(&url, "dev-1", "CODE-1")
        .await
        .unwrap();
    match r2 {
        CodexDevicePollResult::Ready { body } => {
            assert_eq!(body["authorization_code"], "code123");
            assert_eq!(body["code_verifier"], "verifier456");
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[tokio::test]
async fn test_poll_device_once_server_error_returns_failed() {
    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_json(serde_json::json!({"error": "internal"})),
        )
        .mount(&mock_server)
        .await;

    let url = format!(
        "{}/api/accounts/deviceauth/token",
        mock_server.uri()
    );
    let r = CodexAuthenticator::poll_device_once_with_url(&url, "dev-1", "CODE-1")
        .await
        .unwrap();
    match r {
        CodexDevicePollResult::Failed { status, body } => {
            assert_eq!(status, 500);
            assert!(body.contains("internal"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}
