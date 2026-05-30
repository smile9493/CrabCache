use super::common;
use super::{AuthError, Authenticator, LoginOptions};
use crate::http_client;
use crate::types::{Provider, TokenRecord};
use async_trait::async_trait;
use base64::Engine;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration as StdDuration;

const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEFAULT_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const SCOPES: &str = "openid email profile offline_access";
const DEFAULT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const DEVICE_USERCODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const DEVICE_VERIFY_URL: &str = "https://auth.openai.com/codex/device";
const DEVICE_DEFAULT_POLL_SECS: u64 = 5;
const DEVICE_POLL_TIMEOUT_SECS: u64 = 900;
const REFRESH_MAX_RETRIES: usize = 3;

/// Result of [`start_device_usercode`] — device session metadata for client-driven polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexDeviceStart {
    /// Internal device_auth_id for subsequent poll calls.
    pub device_auth_id: String,
    /// User code displayed to the human (e.g. "ABCD-1234").
    pub user_code: String,
    /// URL the human should visit to authorize.
    pub verify_url: String,
    /// Minimum seconds between poll attempts.
    pub poll_interval_secs: u64,
}

/// Single-poll result from [`poll_device_once`].
#[derive(Debug, Clone)]
pub enum CodexDevicePollResult {
    /// Authorization not yet granted; retry after `poll_interval_secs`.
    Pending,
    /// Authorization completed; body contains `authorization_code` + `code_verifier`.
    Ready { body: serde_json::Value },
    /// Non-retryable server error.
    Failed { status: u16, body: String },
}

fn get_client_id() -> String {
    std::env::var("CRABCACHE_OAUTH_CODEX_CLIENT_ID")
        .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string())
}

fn device_usercode_url() -> String {
    std::env::var("CRABCACHE_OAUTH_CODEX_DEVICE_USERCODE_URL")
        .unwrap_or_else(|_| DEVICE_USERCODE_URL.to_string())
}

fn device_token_url() -> String {
    std::env::var("CRABCACHE_OAUTH_CODEX_DEVICE_TOKEN_URL")
        .unwrap_or_else(|_| DEVICE_TOKEN_URL.to_string())
}

fn token_url() -> String {
    std::env::var("CRABCACHE_OAUTH_CODEX_TOKEN_URL").unwrap_or_else(|_| TOKEN_URL.to_string())
}

/// Device-auth poll endpoint (`POST` JSON `{device_auth_id, user_code}`).
pub fn codex_device_token_endpoint() -> String {
    device_token_url()
}

/// OAuth token exchange endpoint (`POST` form `grant_type=authorization_code`).
pub fn codex_oauth_token_endpoint() -> String {
    token_url()
}

/// Claims parsed from an OIDC id_token JWT
#[derive(Debug, Default)]
pub struct JwtClaims {
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
    pub user_id: Option<String>,
}

pub struct CodexAuthenticator;

impl CodexAuthenticator {
    pub fn new() -> Self {
        Self
    }

    /// Step 1: Request a device user code from OpenAI.
    ///
    /// Returns [`CodexDeviceStart`] with the `user_code` and `verify_url` to display
    /// to the human, plus `device_auth_id` for subsequent [`poll_device_once`] calls.
    pub async fn start_device_usercode() -> Result<CodexDeviceStart, AuthError> {
        Self::start_device_usercode_with_url_and_proxy(&device_usercode_url(), None).await
    }

    /// Step 2: Single poll attempt for device authorization status.
    ///
    /// Call this repeatedly (with `poll_interval_secs` delay) until the result is
    /// no longer [`CodexDevicePollResult::Pending`]. On [`Ready`](CodexDevicePollResult::Ready),
    /// pass the `body` to [`complete_device_from_poll`].
    pub async fn poll_device_once(
        device_auth_id: &str,
        user_code: &str,
    ) -> Result<CodexDevicePollResult, AuthError> {
        Self::poll_device_once_with_url_and_proxy(&device_token_url(), device_auth_id, user_code, None)
            .await
    }

    /// Step 3: Exchange authorization code from a successful poll into a [`TokenRecord`].
    ///
    /// Extracts `authorization_code` and `code_verifier` from `poll_body` (the `body` field
    /// from [`CodexDevicePollResult::Ready`]), then exchanges for an OAuth token.
    pub async fn complete_device_from_poll(
        poll_body: &serde_json::Value,
    ) -> Result<TokenRecord, AuthError> {
        Self::complete_device_from_poll_with_url_and_proxy(&token_url(), poll_body, None).await
    }

    /// [`start_device_usercode`] with explicit URL override (used by admin BFF / tests).
    pub async fn start_device_usercode_with_url(
        usercode_url: &str,
    ) -> Result<CodexDeviceStart, AuthError> {
        Self::start_device_usercode_with_url_and_proxy(usercode_url, None).await
    }

    /// [`start_device_usercode`] with explicit URL and optional proxy (used by admin BFF).
    pub async fn start_device_usercode_with_url_and_proxy(
        usercode_url: &str,
        proxy_url: Option<&str>,
    ) -> Result<CodexDeviceStart, AuthError> {
        let client = http_client::build_anti_detect_client(proxy_url)?;
        let client_id = get_client_id();

        let usercode_req = client
            .post(usercode_url)
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "client_id": client_id }))
            .build()
            .map_err(|e| AuthError::OAuth(format!("failed to build request: {e}")))?;
        let usercode_req = http_client::scrub_request_headers(usercode_req);
        let usercode_resp = client.execute(usercode_req).await
            .map_err(|e| AuthError::OAuth(format!("device usercode request failed: {e}")))?;

        let status = usercode_resp.status();
        let body: serde_json::Value = usercode_resp.json().await
            .map_err(|e| AuthError::OAuth(format!("failed to parse usercode response: {e}")))?;
        if !status.is_success() {
            if status.as_u16() == 404 {
                return Err(AuthError::OAuth(
                    "codex device endpoint is unavailable (status 404)".into(),
                ));
            }
            return Err(AuthError::TokenExchangeFailed {
                status: status.as_u16(),
                body: body.to_string(),
            });
        }

        let device_auth_id = body["device_auth_id"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AuthError::OAuth("missing device_auth_id".into()))?
            .to_string();

        let user_code = body["user_code"]
            .as_str()
            .or_else(|| body["usercode"].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AuthError::OAuth("missing user_code".into()))?
            .to_string();

        let poll_interval_secs = parse_codex_device_poll_interval(&body["interval"]);

        Ok(CodexDeviceStart {
            device_auth_id,
            user_code,
            verify_url: DEVICE_VERIFY_URL.to_string(),
            poll_interval_secs,
        })
    }

    /// [`poll_device_once`] with explicit URL override (used by admin BFF / tests).
    pub async fn poll_device_once_with_url(
        token_url: &str,
        device_auth_id: &str,
        user_code: &str,
    ) -> Result<CodexDevicePollResult, AuthError> {
        Self::poll_device_once_with_url_and_proxy(token_url, device_auth_id, user_code, None).await
    }

    /// [`poll_device_once`] with explicit URL and optional proxy.
    pub async fn poll_device_once_with_url_and_proxy(
        token_url: &str,
        device_auth_id: &str,
        user_code: &str,
        proxy_url: Option<&str>,
    ) -> Result<CodexDevicePollResult, AuthError> {
        let client = http_client::build_anti_detect_client(proxy_url)?;
        let req = client
            .post(token_url)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "device_auth_id": device_auth_id,
                "user_code": user_code,
            }))
            .build()
            .map_err(|e| AuthError::OAuth(format!("failed to build request: {e}")))?;
        let req = http_client::scrub_request_headers(req);
        let resp = client.execute(req).await
            .map_err(|e| AuthError::OAuth(format!("device poll request failed: {e}")))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| AuthError::OAuth(format!("failed to parse poll response: {e}")))?;

        if status.is_success() {
            return Ok(CodexDevicePollResult::Ready { body });
        }

        match status.as_u16() {
            403 | 404 => Ok(CodexDevicePollResult::Pending),
            _ => Ok(CodexDevicePollResult::Failed {
                status: status.as_u16(),
                body: body.to_string(),
            }),
        }
    }

    /// [`complete_device_from_poll`] with explicit URL override (used by admin BFF / tests).
    pub async fn complete_device_from_poll_with_url(
        oauth_token_url: &str,
        poll_body: &serde_json::Value,
    ) -> Result<TokenRecord, AuthError> {
        Self::complete_device_from_poll_with_url_and_proxy(oauth_token_url, poll_body, None).await
    }

    /// [`complete_device_from_poll`] with explicit URL and optional proxy.
    pub async fn complete_device_from_poll_with_url_and_proxy(
        oauth_token_url: &str,
        poll_body: &serde_json::Value,
        proxy_url: Option<&str>,
    ) -> Result<TokenRecord, AuthError> {
        let authorization_code = poll_body["authorization_code"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AuthError::OAuth("missing authorization_code".into()))?;

        let code_verifier = poll_body["code_verifier"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AuthError::OAuth("missing code_verifier".into()))?;

        let client = http_client::build_anti_detect_client(proxy_url)?;
        exchange_authorization_code_with_url(
            &client,
            authorization_code,
            DEVICE_REDIRECT_URI,
            code_verifier,
            oauth_token_url,
        )
        .await
    }

    /// OpenAI Codex device authorization flow (headless / no browser callback).
    ///
    /// Convenience wrapper that chains [`start_device_usercode`] → poll loop →
    /// [`complete_device_from_poll`] for CLI usage. For web/BFF usage, call
    /// the individual steps directly and drive the poll loop from the client.
    pub async fn device_login(&self) -> Result<TokenRecord, AuthError> {
        let start = Self::start_device_usercode().await?;

        println!("Starting Codex device authentication...");
        println!("Codex device URL: {}", start.verify_url);
        println!("Codex device code: {}", start.user_code);

        let poll_result = poll_codex_device_token(
            &start.device_auth_id,
            &start.user_code,
            start.poll_interval_secs,
            None,
        )
        .await?;

        Self::complete_device_from_poll(&poll_result).await
    }
}

impl Default for CodexAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── PKCE BFF API (for admin dashboard) ───────────────────────────────────────

/// Start a PKCE login session.
///
/// Returns `(verifier, challenge, state, auth_url)`. The BFF stores `verifier`+`state`
/// in a `DashMap` keyed by `session_id`, and returns `auth_url` to the frontend.
pub fn start_pkce_login() -> (String, String, String, String) {
    start_pkce_login_with_port(DEFAULT_PORT)
}

/// Like [`start_pkce_login`] but with explicit port.
pub fn start_pkce_login_with_port(port: u16) -> (String, String, String, String) {
    let verifier = common::generate_pkce_verifier();
    let challenge = common::generate_pkce_challenge(&verifier);
    let state = common::generate_random_state();
    let auth_url = build_auth_url(&challenge, &state, port);
    (verifier, challenge, state, auth_url)
}

/// Extract `code` and `state` from a pasted callback URL.
///
/// The user is asked to paste the full redirect URL from the browser (e.g.
/// `http://localhost:1455/auth/callback?code=abc&state=xyz`). This function
/// parses out the `code` and `state` query parameters.
pub fn extract_code_from_callback_url(callback_url: &str) -> Result<(String, String), AuthError> {
    let url = url::Url::parse(callback_url)
        .map_err(|e| AuthError::OAuth(format!("invalid callback URL: {e}")))?;

    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }

    let code = code.ok_or_else(|| AuthError::OAuth("callback URL missing 'code' parameter".into()))?;
    let state = state.ok_or_else(|| AuthError::OAuth("callback URL missing 'state' parameter".into()))?;
    Ok((code, state))
}

/// Complete the PKCE exchange: take the `code` + `verifier` + `redirect_uri`
/// and exchange for a [`TokenRecord`].
pub async fn complete_pkce_exchange(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenRecord, AuthError> {
    complete_pkce_exchange_with_url(code, verifier, redirect_uri, &token_url()).await
}

/// Like [`complete_pkce_exchange`] with explicit token URL.
pub async fn complete_pkce_exchange_with_url(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    oauth_token_url: &str,
) -> Result<TokenRecord, AuthError> {
    complete_pkce_exchange_with_url_and_proxy(code, verifier, redirect_uri, oauth_token_url, None)
        .await
}

/// Like [`complete_pkce_exchange_with_url`] with explicit proxy URL.
pub async fn complete_pkce_exchange_with_url_and_proxy(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    oauth_token_url: &str,
    proxy_url: Option<&str>,
) -> Result<TokenRecord, AuthError> {
    let client = http_client::build_anti_detect_client(proxy_url)?;
    exchange_authorization_code_with_url(&client, code, redirect_uri, verifier, oauth_token_url)
        .await
}

fn encode_param(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Build the Codex authorization URL
pub fn build_auth_url(challenge: &str, state: &str, port: u16) -> String {
    let client_id = get_client_id();
    let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");
    format!(
        "https://auth.openai.com/oauth/authorize?\
         client_id={client_id}\
         &response_type=code\
         &redirect_uri={}\
         &scope={}\
         &state={state}\
         &code_challenge={challenge}\
         &code_challenge_method=S256\
         &prompt=login\
         &id_token_add_organizations=true\
         &codex_cli_simplified_flow=true",
        encode_param(&redirect_uri),
        encode_param(SCOPES),
    )
}

/// Decode email, plan, and account_id from a Codex OAuth access_token JWT (no signature check).
pub fn decode_codex_access_token_claims(
    access_token: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let Ok(claims) = parse_jwt_payload(access_token) else {
        return (None, None, None);
    };
    let mut email = claims.email;
    let mut plan_type = claims.plan_type;
    let account_id = claims.account_id;

    if email.is_none() || plan_type.is_none() {
        if let Ok(payload) = decode_jwt_payload_value(access_token) {
            let auth = payload.get("https://api.openai.com/auth");
            let profile = payload.get("https://api.openai.com/profile");
            if email.is_none() {
                email = profile
                    .and_then(|p| p.get("email"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            if plan_type.is_none() {
                plan_type = auth
                    .and_then(|a| a.get("chatgpt_plan_type"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
        }
    }

    (email, plan_type, account_id)
}

fn decode_jwt_payload_value(jwt: &str) -> Result<serde_json::Value, AuthError> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return Err(AuthError::OAuth("invalid JWT format".into()));
    }
    let payload_b64 = parts[1];
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| AuthError::OAuth(format!("failed to decode JWT payload: {e}")))?;
    serde_json::from_slice(&payload_bytes)
        .map_err(|e| AuthError::OAuth(format!("failed to parse JWT JSON: {e}")))
}

/// Parse JWT payload without signature verification
pub fn parse_jwt_payload(jwt: &str) -> Result<JwtClaims, AuthError> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return Err(AuthError::OAuth("invalid JWT format".into()));
    }

    let payload = decode_jwt_payload_value(jwt)?;

    let auth_info = payload
        .get("https://api.openai.com/auth")
        .or_else(|| payload.get("auth"));

    let account_id = auth_info
        .and_then(|v| v["chatgpt_account_id"].as_str())
        .or_else(|| payload["chatgpt_account_id"].as_str())
        .map(String::from);

    let plan_type = auth_info
        .and_then(|v| v["chatgpt_plan_type"].as_str())
        .or_else(|| payload["chatgpt_plan_type"].as_str())
        .map(String::from);

    let user_id = auth_info
        .and_then(|v| v["chatgpt_user_id"].as_str())
        .or_else(|| payload["chatgpt_user_id"].as_str())
        .or_else(|| payload["sub"].as_str())
        .map(String::from);

    Ok(JwtClaims {
        email: payload["email"].as_str().map(String::from),
        account_id,
        plan_type,
        user_id,
    })
}

/// Credential filename compatible with CLIProxyAPI (`codex-*.json`).
pub fn credential_filename(
    email: &str,
    plan_type: Option<&str>,
    account_id_hash: Option<&str>,
) -> String {
    let email = email.trim();
    let plan = normalize_plan_type_for_filename(plan_type.unwrap_or_default());

    if plan.is_empty() {
        return format!("codex-{email}.json");
    }
    if plan == "team" {
        let hash = account_id_hash.unwrap_or("unknown").trim();
        return format!("codex-{hash}-{email}-{plan}.json");
    }
    format!("codex-{email}-{plan}.json")
}

/// Hash account id to 8-char hex prefix (CLIProxyAPI compatible).
pub fn hash_account_id(account_id: &str) -> String {
    let digest = Sha256::digest(account_id.as_bytes());
    hex::encode(&digest[..4])
}

/// Parse poll interval from device usercode response (`interval` may be string or int).
pub fn parse_codex_device_poll_interval(interval: &serde_json::Value) -> u64 {
    if let Some(s) = interval.as_str() {
        if let Ok(secs) = s.trim().parse::<u64>()
            && secs > 0
        {
            return secs;
        }
    }
    if let Some(secs) = interval.as_u64().filter(|&v| v > 0) {
        return secs;
    }
    if let Some(secs) = interval.as_i64().filter(|&v| v > 0) {
        return secs as u64;
    }
    DEVICE_DEFAULT_POLL_SECS
}

/// Returns false when refresh should not be retried (e.g. `refresh_token_reused`).
pub fn is_refresh_error_retryable(err: &AuthError) -> bool {
    match err {
        AuthError::TokenExchangeFailed { body, .. } => {
            !body.to_ascii_lowercase().contains("refresh_token_reused")
        }
        AuthError::OAuth(msg) => !msg.contains("refresh_token_reused"),
        _ => false,
    }
}

fn normalize_plan_type_for_filename(plan_type: &str) -> String {
    let normalized: Vec<String> = plan_type
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect();
    normalized.join("-")
}

async fn poll_codex_device_token(
    device_auth_id: &str,
    user_code: &str,
    poll_interval_secs: u64,
    proxy_url: Option<&str>,
) -> Result<serde_json::Value, AuthError> {
    let client = http_client::build_anti_detect_client(proxy_url)?;
    let deadline =
        tokio::time::Instant::now() + StdDuration::from_secs(DEVICE_POLL_TIMEOUT_SECS);

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::OAuth(
                "codex device authentication timed out after 15 minutes".into(),
            ));
        }

        let req = client
            .post(device_token_url())
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "device_auth_id": device_auth_id,
                "user_code": user_code,
            }))
            .build()
            .map_err(|e| AuthError::OAuth(format!("failed to build request: {e}")))?;
        let req = http_client::scrub_request_headers(req);
        let resp = client.execute(req).await
            .map_err(|e| AuthError::OAuth(format!("device poll request failed: {e}")))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| AuthError::OAuth(format!("failed to parse poll response: {e}")))?;

        if status.is_success() {
            return Ok(body);
        }

        match status.as_u16() {
            403 | 404 => {
                tokio::time::sleep(StdDuration::from_secs(poll_interval_secs)).await;
                continue;
            }
            _ => {
                return Err(AuthError::TokenExchangeFailed {
                    status: status.as_u16(),
                    body: body.to_string(),
                });
            }
        }
    }
}

async fn exchange_authorization_code(
    client: &wreq::Client,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenRecord, AuthError> {
    exchange_authorization_code_with_url(client, code, redirect_uri, code_verifier, &token_url())
        .await
}

async fn exchange_authorization_code_with_url(
    client: &wreq::Client,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    oauth_token_url: &str,
) -> Result<TokenRecord, AuthError> {
    let client_id = get_client_id();
    #[derive(Serialize)]
    struct AuthorizationCodeForm<'a> {
        grant_type: &'a str,
        code: &'a str,
        client_id: &'a str,
        redirect_uri: &'a str,
        code_verifier: &'a str,
    }
    let params = AuthorizationCodeForm {
        grant_type: "authorization_code",
        code,
        client_id: client_id.as_str(),
        redirect_uri,
        code_verifier,
    };

    let req = client.post(oauth_token_url).form(&params).build()
        .map_err(|e| AuthError::OAuth(format!("failed to build request: {e}")))?;
    let req = http_client::scrub_request_headers(req);
    let resp = client.execute(req).await
        .map_err(|e| AuthError::OAuth(format!("token exchange request failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(AuthError::TokenExchangeFailed {
            status: status.as_u16(),
            body: body_text,
        });
    }

    let token_resp: serde_json::Value = resp.json().await
        .map_err(|e| AuthError::OAuth(format!("failed to parse token response: {e}")))?;
    parse_token_response(&token_resp)
}

/// Refresh a Codex token once (with optional profile proxy).
pub async fn refresh_codex_token_with_proxy(
    record: &TokenRecord,
    proxy_url: Option<&str>,
) -> Result<TokenRecord, AuthError> {
    refresh_once(record, proxy_url).await
}

async fn refresh_once(record: &TokenRecord, proxy_url: Option<&str>) -> Result<TokenRecord, AuthError> {
    let refresh_token = record
        .refresh_token
        .as_ref()
        .ok_or_else(|| AuthError::OAuth("no refresh token available".into()))?;

    let client = http_client::build_anti_detect_client(proxy_url)?;
    let client_id = get_client_id();
    #[derive(Serialize)]
    struct RefreshTokenForm<'a> {
        grant_type: &'a str,
        client_id: &'a str,
        refresh_token: &'a str,
        scope: &'a str,
    }
    let params = RefreshTokenForm {
        grant_type: "refresh_token",
        client_id: client_id.as_str(),
        refresh_token,
        scope: "openid profile email",
    };

    let req = client.post(token_url()).form(&params).build()
        .map_err(|e| AuthError::OAuth(format!("failed to build request: {e}")))?;
    let req = http_client::scrub_request_headers(req);
    let resp = client.execute(req).await
        .map_err(|e| AuthError::OAuth(format!("refresh request failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        return Err(AuthError::TokenExchangeFailed {
            status: status.as_u16(),
            body: body_text,
        });
    }

    let token_resp: serde_json::Value = resp.json().await
        .map_err(|e| AuthError::OAuth(format!("failed to parse refresh response: {e}")))?;
    let mut refreshed = parse_token_response(&token_resp)?;
    refreshed.id = record.id.clone();
    refreshed.metadata = record.metadata.clone();
    Ok(refreshed)
}

#[async_trait]
impl Authenticator for CodexAuthenticator {
    fn provider(&self) -> Provider {
        Provider::Codex
    }

    async fn login(&self, opts: &LoginOptions) -> Result<TokenRecord, AuthError> {
        if opts.device_mode {
            return self.device_login().await;
        }

        let port = opts.callback_port.unwrap_or(DEFAULT_PORT);
        let verifier = common::generate_pkce_verifier();
        let challenge = common::generate_pkce_challenge(&verifier);
        let state = common::generate_random_state();

        let (_handle, rx) = common::start_callback_server(port, CALLBACK_PATH, None).await?;

        let auth_url = build_auth_url(&challenge, &state, port);
        if opts.no_browser {
            println!("Open this URL in your browser:\n{auth_url}");
        } else {
            common::open_browser(&auth_url);
        }

        let result = tokio::time::timeout(std::time::Duration::from_secs(300), rx)
            .await
            .map_err(|_| AuthError::CallbackTimeout)?
            .map_err(|_| AuthError::CallbackTimeout)?;

        if result.state != state {
            return Err(AuthError::InvalidState {
                expected: state,
                actual: result.state,
            });
        }

        let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");
        let client = http_client::build_anti_detect_client(opts.proxy_url.as_deref())?;
        exchange_authorization_code(&client, &result.code, &redirect_uri, &verifier).await
    }

    async fn refresh(&self, record: &TokenRecord) -> Result<TokenRecord, AuthError> {
        let record_id = record.id.clone();
        let record_metadata = record.metadata.clone();
        let refresh_token = record
            .refresh_token
            .clone()
            .ok_or_else(|| AuthError::OAuth("no refresh token available".into()))?;

        let mut attempt_record = record.clone();
        attempt_record.refresh_token = Some(refresh_token);

        let mut last_err = None;
        let mut refreshed = None;
        for attempt in 0..REFRESH_MAX_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(StdDuration::from_secs(attempt as u64)).await;
            }
            match refresh_once(&attempt_record, None).await {
                Ok(record) => {
                    refreshed = Some(record);
                    break;
                }
                Err(err) => {
                    if !is_refresh_error_retryable(&err) {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
            }
        }

        let mut refreshed = refreshed.ok_or_else(|| {
            last_err.unwrap_or_else(|| AuthError::OAuth("token refresh failed".into()))
        })?;

        refreshed.id = record_id;
        if refreshed.metadata.is_empty() {
            refreshed.metadata = record_metadata;
        }
        Ok(refreshed)
    }

    fn refresh_lead(&self) -> Option<std::time::Duration> {
        Some(Duration::hours(4).to_std().expect("valid duration"))
    }

    fn callback_url(&self, port: u16) -> String {
        format!("http://localhost:{port}{CALLBACK_PATH}")
    }
}

fn parse_token_response(resp: &serde_json::Value) -> Result<TokenRecord, AuthError> {
    let access_token = resp["access_token"]
        .as_str()
        .ok_or_else(|| AuthError::OAuth("missing access_token".into()))?
        .to_string();
    let refresh_token = resp["refresh_token"].as_str().map(String::from);
    let id_token = resp["id_token"].as_str().map(String::from);
    let expires_in = resp["expires_in"].as_i64();

    let (email, account_id, plan_type) = if let Some(jwt) = &id_token {
        let claims = parse_jwt_payload(jwt)?;
        (claims.email, claims.account_id, claims.plan_type)
    } else {
        (None, None, None)
    };

    let expired_at = expires_in.map(|secs| Utc::now() + Duration::seconds(secs));

    let account_id_hash = account_id.as_deref().map(hash_account_id);
    let id = email
        .as_deref()
        .map(|e| {
            credential_filename(
                e,
                plan_type.as_deref(),
                account_id_hash.as_deref(),
            )
            .trim_end_matches(".json")
            .to_string()
        })
        .unwrap_or_else(|| "default".to_string());

    let mut metadata = HashMap::new();
    if let Some(v) = account_id {
        metadata.insert("account_id".to_owned(), serde_json::Value::String(v));
    }
    if let Some(v) = plan_type {
        metadata.insert("plan_type".to_owned(), serde_json::Value::String(v));
    }

    Ok(TokenRecord {
        id,
        provider: Provider::Codex,
        access_token,
        refresh_token,
        id_token,
        expired_at,
        last_refresh: Some(Utc::now()),
        email,
        disabled: false,
        metadata,
        file_path: None,
    })
}
