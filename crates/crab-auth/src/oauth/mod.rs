pub mod claude;
pub mod codex;
pub mod codex_import;
pub mod common;
pub mod gemini;
pub mod xai;

pub use codex::{CodexDevicePollResult, CodexDeviceStart};
pub use codex_import::{ensure_fresh_codex_token, parse_codex_import_documents, token_needs_refresh};

use crate::types::{Provider, TokenRecord};
use async_trait::async_trait;

/// Options for interactive login
#[derive(Default)]
pub struct LoginOptions {
    /// If true, don't open browser, just print URL
    pub no_browser: bool,
    /// Optional project ID (used by Gemini)
    pub project_id: Option<String>,
    /// Optional callback port override
    pub callback_port: Option<u16>,
    /// Use OAuth device flow instead of browser PKCE callback (Codex only)
    pub device_mode: bool,
    /// Optional prompt function for manual URL paste
    #[allow(clippy::type_complexity)]
    pub prompt: Option<Box<dyn Fn(&str) -> String + Send + Sync>>,
    /// Optional proxy URL for all OAuth HTTP requests (socks5://, http://, etc.)
    pub proxy_url: Option<String>,
}

/// Error type for OAuth operations
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("oauth error: {0}")]
    OAuth(String),
    #[error("network error (reqwest): {0}")]
    Network(#[from] reqwest::Error),
    #[error("network error (wreq): {0}")]
    WreqNetwork(#[from] wreq::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timeout waiting for callback")]
    CallbackTimeout,
    #[error("invalid state: expected {expected}, got {actual}")]
    InvalidState { expected: String, actual: String },
    #[error("token exchange failed: {status} - {body}")]
    TokenExchangeFailed { status: u16, body: String },
}

/// Trait for provider-specific OAuth authenticators
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Which provider this authenticator handles
    fn provider(&self) -> Provider;

    /// Run interactive OAuth login flow, returns a TokenRecord
    async fn login(&self, opts: &LoginOptions) -> Result<TokenRecord, AuthError>;

    /// Refresh an expired token. Returns updated TokenRecord.
    async fn refresh(&self, record: &TokenRecord) -> Result<TokenRecord, AuthError>;

    /// How early before expiry to trigger refresh. None = no proactive refresh.
    fn refresh_lead(&self) -> Option<std::time::Duration>;

    /// Get the callback URL for this authenticator (used for OAuth redirects)
    fn callback_url(&self, port: u16) -> String;
}
