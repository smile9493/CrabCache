//! DTO types for the Codex OAuth device-flow BFF.

use serde::{Deserialize, Serialize};

/// Response to `POST .../oauth/codex/device/start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexDeviceStartResponse {
    pub session_id: String,
    pub user_code: String,
    pub verify_url: String,
    pub poll_interval_secs: u64,
}

/// Response to `GET .../oauth/codex/device/:session_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexDeviceStatusResponse {
    /// One of: `pending`, `completed`, `failed`, `expired`.
    pub status: String,
    pub user_code: String,
    /// Present when `status == "completed"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Present when `status == "completed"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Present when `status == "completed"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
    /// Present when `status == "failed"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Summary of a saved Codex credential (token content is excluded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexCredentialSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired_at: Option<String>,
    pub disabled: bool,
}

/// Response to `GET /api/admin/oauth/codex/credentials`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexCredentialListResponse {
    pub credentials: Vec<CodexCredentialSummary>,
}

/// Request body for `POST .../oauth/codex/import`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexImportRequest {
    pub credential_id: String,
}

/// Response to `POST .../oauth/codex/import`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexImportResponse {
    pub credential_id: String,
    pub profile_id: String,
}

// ─── PKCE flow DTOs ──────────────────────────────────────────────────────────

/// Response to `POST .../oauth/codex/pkce/start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPkceStartResponse {
    pub session_id: String,
    /// URL to open in the browser for authorization.
    pub auth_url: String,
    /// `"auto"` if a callback listener is running; `"manual"` otherwise.
    pub mode: String,
}

/// Request body for `POST .../oauth/codex/pkce/exchange`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPkceExchangeRequest {
    /// The full callback URL pasted from the browser redirect (manual mode).
    /// Example: `http://localhost:1455/auth/callback?code=abc&state=xyz`
    pub callback_url: String,
}

/// Response to `POST .../oauth/codex/pkce/exchange` or `GET .../pkce/:session_id` when completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexPkceExchangeResponse {
    /// Session status: `pending`, `completed`, `failed`, `expired`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
