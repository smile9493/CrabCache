use super::common;
use super::{AuthError, Authenticator, LoginOptions};
use crate::types::{Provider, TokenRecord};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::HashMap;

const TOKEN_URL: &str = "https://api.anthropic.com/v1/oauth/token";
const DEFAULT_PORT: u16 = 54545;
const CALLBACK_PATH: &str = "/callback";
const SCOPES: &str = "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

fn get_client_id() -> String {
    std::env::var("CRABCACHE_OAUTH_CLAUDE_CLIENT_ID")
        .unwrap_or_else(|_| "9d1c250a-placeholder".to_string())
}

pub struct ClaudeAuthenticator;

impl ClaudeAuthenticator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_param(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Build the Claude authorization URL
pub fn build_auth_url(challenge: &str, state: &str, port: u16) -> String {
    let client_id = get_client_id();
    let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");
    format!(
        "https://claude.ai/oauth/authorize?\
         code=true\
         &client_id={client_id}\
         &response_type=code\
         &redirect_uri={}\
         &scope={}\
         &code_challenge={challenge}\
         &code_challenge_method=S256\
         &state={state}",
        encode_param(&redirect_uri),
        encode_param(SCOPES),
    )
}

#[async_trait]
impl Authenticator for ClaudeAuthenticator {
    fn provider(&self) -> Provider {
        Provider::Claude
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
        let body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": result.code,
            "state": state,
            "client_id": get_client_id(),
            "redirect_uri": redirect_uri,
            "code_verifier": verifier,
        });

        let resp = client.post(TOKEN_URL).json(&body).send().await?;
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
        let body = serde_json::json!({
            "client_id": get_client_id(),
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        });

        let resp = client.post(TOKEN_URL).json(&body).send().await?;
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
    let expires_in = resp["expires_in"].as_i64();
    let email = resp["account"]["email_address"]
        .as_str()
        .or_else(|| resp["email"].as_str())
        .map(String::from);

    let expired_at = expires_in.map(|secs| Utc::now() + Duration::seconds(secs));
    let id = email.clone().unwrap_or_else(|| "default".to_string());

    Ok(TokenRecord {
        id,
        provider: Provider::Claude,
        access_token,
        refresh_token,
        id_token: None,
        expired_at,
        last_refresh: Some(Utc::now()),
        email,
        disabled: false,
        metadata: HashMap::new(),
        file_path: None,
    })
}
