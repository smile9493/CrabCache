use super::common;
use super::{AuthError, Authenticator, LoginOptions};
use crate::types::{Provider, TokenRecord};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::HashMap;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
const DEFAULT_PORT: u16 = 8085;
const CALLBACK_PATH: &str = "/oauth2callback";
const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";

fn get_client_id() -> String {
    std::env::var("CRABCACHE_OAUTH_GEMINI_CLIENT_ID")
        .unwrap_or_else(|_| "681255809395-placeholder.apps.googleusercontent.com".to_string())
}

fn get_client_secret() -> String {
    std::env::var("CRABCACHE_OAUTH_GEMINI_CLIENT_SECRET").unwrap_or_default()
}

pub struct GeminiAuthenticator;

impl GeminiAuthenticator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeminiAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_param(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Build the Gemini authorization URL
pub fn build_auth_url(state: &str, port: u16) -> String {
    let client_id = get_client_id();
    let redirect_uri = format!("http://localhost:{port}{CALLBACK_PATH}");
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?\
         client_id={client_id}\
         &redirect_uri={}\
         &response_type=code\
         &scope={}\
         &access_type=offline\
         &prompt=consent\
         &state={state}",
        encode_param(&redirect_uri),
        encode_param(SCOPES),
    )
}

#[async_trait]
impl Authenticator for GeminiAuthenticator {
    fn provider(&self) -> Provider {
        Provider::Gemini
    }

    async fn login(&self, opts: &LoginOptions) -> Result<TokenRecord, AuthError> {
        let port = opts.callback_port.unwrap_or(DEFAULT_PORT);
        let state = common::generate_random_state();

        let (_handle, rx) = common::start_callback_server(port, CALLBACK_PATH, None).await?;

        let auth_url = build_auth_url(&state, port);
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
        let client_secret = get_client_secret();
        let params = [
            ("code", result.code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
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
        let access_token = token_resp["access_token"]
            .as_str()
            .ok_or_else(|| AuthError::OAuth("missing access_token".into()))?
            .to_string();
        let refresh_token = token_resp["refresh_token"].as_str().map(String::from);
        let expires_in = token_resp["expires_in"].as_i64();
        let expired_at = expires_in.map(|secs| Utc::now() + Duration::seconds(secs));

        let email = fetch_user_email(&client, &access_token).await;

        let mut metadata = HashMap::new();
        metadata.insert(
            "token_uri".to_owned(),
            serde_json::Value::String(TOKEN_URL.to_owned()),
        );
        metadata.insert(
            "client_id".to_owned(),
            serde_json::Value::String(get_client_id()),
        );
        metadata.insert(
            "client_secret".to_owned(),
            serde_json::Value::String(get_client_secret()),
        );
        metadata.insert(
            "scopes".to_owned(),
            serde_json::Value::Array(
                SCOPES
                    .split_whitespace()
                    .map(|s| serde_json::Value::String(s.to_owned()))
                    .collect(),
            ),
        );

        let id = email.clone().unwrap_or_else(|| "default".to_string());

        Ok(TokenRecord {
            id,
            provider: Provider::Gemini,
            access_token,
            refresh_token,
            id_token: None,
            expired_at,
            last_refresh: Some(Utc::now()),
            email,
            disabled: false,
            metadata,
            file_path: None,
        })
    }

    async fn refresh(&self, record: &TokenRecord) -> Result<TokenRecord, AuthError> {
        let refresh_token = record
            .refresh_token
            .as_ref()
            .ok_or_else(|| AuthError::OAuth("no refresh token available".into()))?;

        let client = reqwest::Client::new();
        let client_id = get_client_id();
        let client_secret = get_client_secret();
        let params = [
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
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
        let access_token = token_resp["access_token"]
            .as_str()
            .ok_or_else(|| AuthError::OAuth("missing access_token".into()))?
            .to_string();
        let new_refresh_token = token_resp["refresh_token"]
            .as_str()
            .or(Some(refresh_token.as_str()))
            .map(String::from);
        let expires_in = token_resp["expires_in"].as_i64();
        let expired_at = expires_in.map(|secs| Utc::now() + Duration::seconds(secs));

        let email = fetch_user_email(&client, &access_token).await;

        Ok(TokenRecord {
            id: record.id.clone(),
            provider: Provider::Gemini,
            access_token,
            refresh_token: new_refresh_token,
            id_token: None,
            expired_at,
            last_refresh: Some(Utc::now()),
            email,
            disabled: false,
            metadata: record.metadata.clone(),
            file_path: None,
        })
    }

    fn refresh_lead(&self) -> Option<std::time::Duration> {
        None
    }

    fn callback_url(&self, port: u16) -> String {
        format!("http://localhost:{port}{CALLBACK_PATH}")
    }
}

async fn fetch_user_email(client: &reqwest::Client, access_token: &str) -> Option<String> {
    let resp = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let info: serde_json::Value = resp.json().await.ok()?;
    info["email"].as_str().map(String::from)
}
