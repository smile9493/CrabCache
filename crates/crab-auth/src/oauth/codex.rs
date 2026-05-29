use super::common;
use super::{AuthError, Authenticator, LoginOptions};
use crate::types::{Provider, TokenRecord};
use async_trait::async_trait;
use base64::Engine;
use chrono::{Duration, Utc};
use std::collections::HashMap;

const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEFAULT_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const SCOPES: &str = "openid email profile offline_access";

fn get_client_id() -> String {
    std::env::var("CRABCACHE_OAUTH_CODEX_CLIENT_ID")
        .unwrap_or_else(|_| "app-placeholder".to_string())
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
}

impl Default for CodexAuthenticator {
    fn default() -> Self {
        Self::new()
    }
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

/// Parse JWT payload without signature verification
pub fn parse_jwt_payload(jwt: &str) -> Result<JwtClaims, AuthError> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return Err(AuthError::OAuth("invalid JWT format".into()));
    }

    let payload_b64 = parts[1];
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| AuthError::OAuth(format!("failed to decode JWT payload: {e}")))?;

    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)?;

    Ok(JwtClaims {
        email: payload["email"].as_str().map(String::from),
        account_id: payload["chatgpt_account_id"].as_str().map(String::from),
        plan_type: payload["chatgpt_plan_type"].as_str().map(String::from),
        user_id: payload["chatgpt_user_id"].as_str().map(String::from),
    })
}

#[async_trait]
impl Authenticator for CodexAuthenticator {
    fn provider(&self) -> Provider {
        Provider::Codex
    }

    async fn login(&self, opts: &LoginOptions) -> Result<TokenRecord, AuthError> {
        let port = opts.callback_port.unwrap_or(DEFAULT_PORT);
        let verifier = common::generate_pkce_verifier();
        let challenge = common::generate_pkce_challenge(&verifier);
        let state = common::generate_random_state();

        let (_handle, rx) = common::start_callback_server(port, CALLBACK_PATH, None).await?;

        let auth_url = build_auth_url(&challenge, &state, port);
        common::open_browser(&auth_url);

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
        let client = reqwest::Client::new();
        let client_id = get_client_id();
        let params = [
            ("grant_type", "authorization_code"),
            ("code", result.code.as_str()),
            ("client_id", client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code_verifier", verifier.as_str()),
        ];

        let resp = client.post(TOKEN_URL).form(&params).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchangeFailed {
                status: status.as_u16(),
                body: body_text,
            });
        }

        let token_resp: serde_json::Value = resp.json().await?;
        parse_token_response(&token_resp)
    }

    async fn refresh(&self, record: &TokenRecord) -> Result<TokenRecord, AuthError> {
        let refresh_token = record
            .refresh_token
            .as_ref()
            .ok_or_else(|| AuthError::OAuth("no refresh token available".into()))?;

        let client = reqwest::Client::new();
        let client_id = get_client_id();
        let params = [
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("scope", "openid profile email"),
        ];

        let resp = client.post(TOKEN_URL).form(&params).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AuthError::TokenExchangeFailed {
                status: status.as_u16(),
                body: body_text,
            });
        }

        let token_resp: serde_json::Value = resp.json().await?;
        parse_token_response(&token_resp)
    }

    fn refresh_lead(&self) -> Option<std::time::Duration> {
        Some(Duration::days(5).to_std().expect("valid duration"))
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
    let id = email.clone().unwrap_or_else(|| "default".to_string());

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
