use base64::Engine;
use crab_auth::oauth::{claude, codex, common, gemini};

#[test]
fn test_pkce_verifier_length() {
    let verifier = common::generate_pkce_verifier();
    assert_eq!(verifier.len(), 128);
    assert!(verifier
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_'));
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
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"alg":"RS256","typ":"JWT"}"#);
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
