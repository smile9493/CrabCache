use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Supported OAuth providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
    Gemini,
    Xai,
    Kimi,
    Antigravity,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Xai => "xai",
            Self::Kimi => "kimi",
            Self::Antigravity => "antigravity",
        }
    }

    /// Returns the filename prefix used by CLIProxyAPI (e.g. "claude", "codex", "gemini", "xai").
    pub fn file_prefix(&self) -> &'static str {
        self.as_str()
    }
}

impl FromStr for Provider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "xai" => Ok(Self::Xai),
            "kimi" => Ok(Self::Kimi),
            "antigravity" => Ok(Self::Antigravity),
            other => Err(format!("unknown provider: {other}")),
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A persisted credential record (maps to a JSON file in the `auths/` directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    /// Unique identifier (typically the filename without extension).
    pub id: String,
    /// Which provider this credential belongs to.
    pub provider: Provider,
    /// The OAuth access token (will be used as Bearer for API calls).
    pub access_token: String,
    /// OAuth refresh token (used to obtain new access tokens).
    pub refresh_token: Option<String>,
    /// ID token (JWT, present for Codex and xAI).
    pub id_token: Option<String>,
    /// User email.
    pub email: Option<String>,
    /// When the access token expires.
    pub expired_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Last time the token was refreshed.
    pub last_refresh: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether this credential is disabled.
    #[serde(default)]
    pub disabled: bool,
    /// Provider-specific metadata (`plan_type`, `account_id`, `project_id`, `base_url`, etc.).
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Original file path (populated when loaded from disk, not serialized).
    #[serde(skip)]
    pub file_path: Option<std::path::PathBuf>,
}
