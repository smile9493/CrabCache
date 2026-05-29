use super::common;
use super::{AuthError, Authenticator, LoginOptions};
use crate::types::{Provider, TokenRecord};
use async_trait::async_trait;
use base64::Engine;
use chrono::{Duration, Utc};
use std::collections::HashMap;

const OIDC_DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
const DEFAULT_PORT: u16 = 56121;
const CALLBACK_PATH: &str = "/callback";
const SCOPES: &str = "openid profile email offline_access grok-cli:access api:access";

fn get_client_id() -> String {
    std::env::var("CRABCACHE_OAUTH_XAI_CLIENT_ID").unwrap_or_else(|_| "xai-placeholder".to_string())
}

pub struct XaiAuthenticator;

impl XaiAuthenticator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for XaiAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

fn encode_param(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// OIDC discovery document
#[derive(Debug, serde::Deserialize)]
struct OidcConfig {
    authorization_endpoint: String,
    token_endpoint: String,
}

/// Fetch OIDC discovery and validate endpoints
async fn fetch_oidc_config() -> Result<OidcConfig, AuthError> {
    let client = reqwest::Client::new();
    let resp = client.get(OIDC_DISCOVERY_URL).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AuthError::TokenExchangeFailed {
            status: status.as_u16(),
            body,
        });
    }

    let config: OidcConfig = resp.json().await?;

    for (name, endpoint) in [
        ("authorization_endpoint", &config.authorization_endpoint),
        ("token_endpoint", &config.token_endpoint),
    ] {
        let parsed = url::Url::parse(endpoint)
            .map_err(|e| AuthError::OAuth(format!("invalid {name}: {e}")))?;

        if parsed.scheme() != "https" {
            return Err(AuthError::OAuth(format!("{name} must use HTTPS")));
        }

        let host = parsed.host_str().unwrap_or("");
        if host != "x.ai" && !host.ends_with(".x.ai") {
            return Err(AuthError::OAuth(format!(
                "{name} host must be x.ai or *.x.ai, got: {host}"
            )));
        }
    }

    Ok(config)
}

/// Parse JWT payload for xAI id_token (extracts email and sub)
fn parse_id_token(id_token: &str) -> (Option<String>, Option<String>) {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return (None, None);
    }
    let Ok(payload_bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1])
    else {
        return (None, None);
    };
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&payload_bytes) else {
        return (None, None);
    };
    let email = payload["email"].as_str().map(String::from);
    let sub = payload["sub"].as_str().map(String::from);
    (email, sub)
}

#[async_trait]
impl Authenticator for XaiAuthenticator {
    fn provider(&self) -> Provider {
        Provider::Xai
    }

    async fn login(&self, opts: &LoginOptions) -> Result<TokenRecord, AuthError> {
        let port = opts.callback_port.unwrap_or(DEFAULT_PORT);

        let oidc_config = fetch_oidc_config().await?;

        let verifier = common::generate_pkce_verifier();
        let challenge = common::generate_pkce_challenge(&verifier);
        let state = common::generate_random_state();
        let nonce = common::generate_random_nonce();

        let (_handle, rx) = common::start_callback_server(port, CALLBACK_PATH, None).await?;

        let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
        let client_id = get_client_id();
        let auth_url = format!(
            "{}?\
             response_type=code\
             &client_id={client_id}\
             &redirect_uri={}\
             &scope={}\
             &code_challenge={challenge}\
             &code_challenge_method=S256\
             &state={state}\
             &nonce={nonce}\
             &plan=generic\
             &referrer=cli-proxy-api",
            oidc_config.authorization_endpoint,
            encode_param(&redirect_uri),
            encode_param(SCOPES),
        );

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

        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "authorization_code"),
            ("code", result.code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", verifier.as_str()),
        ];

        let resp = client
            .post(&oidc_config.token_endpoint)
            .form(&params)
            .send()
            .await?;
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
        let id_token_raw = token_resp["id_token"].as_str().map(String::from);
        let expires_in = token_resp["expires_in"].as_i64();
        let expired_at = expires_in.map(|secs| Utc::now() + Duration::seconds(secs));

        let (email, _sub) = id_token_raw
            .as_deref()
            .map(parse_id_token)
            .unwrap_or((None, None));

        let id = email.clone().unwrap_or_else(|| "default".to_string());

        let mut metadata = HashMap::new();
        metadata.insert(
            "base_url".to_owned(),
            serde_json::Value::String("https://api.x.ai".to_owned()),
        );
        metadata.insert(
            "token_endpoint".to_owned(),
            serde_json::Value::String(oidc_config.token_endpoint),
        );
        metadata.insert(
            "auth_kind".to_owned(),
            serde_json::Value::String("oauth".to_owned()),
        );

        Ok(TokenRecord {
            id,
            provider: Provider::Xai,
            access_token,
            refresh_token,
            id_token: id_token_raw,
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

        let oidc_config = fetch_oidc_config().await?;
        let client_id = get_client_id();

        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
        ];

        let resp = client
            .post(&oidc_config.token_endpoint)
            .form(&params)
            .send()
            .await?;
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
        let id_token_raw = token_resp["id_token"].as_str().map(String::from);
        let expires_in = token_resp["expires_in"].as_i64();
        let expired_at = expires_in.map(|secs| Utc::now() + Duration::seconds(secs));

        let (email, _sub) = id_token_raw
            .as_deref()
            .map(parse_id_token)
            .unwrap_or((None, None));

        Ok(TokenRecord {
            id: record.id.clone(),
            provider: Provider::Xai,
            access_token,
            refresh_token: new_refresh_token,
            id_token: id_token_raw,
            expired_at,
            last_refresh: Some(Utc::now()),
            email,
            disabled: false,
            metadata: record.metadata.clone(),
            file_path: None,
        })
    }

    fn refresh_lead(&self) -> Option<std::time::Duration> {
        Some(Duration::minutes(5).to_std().expect("valid duration"))
    }

    fn callback_url(&self, port: u16) -> String {
        format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
    }
}
