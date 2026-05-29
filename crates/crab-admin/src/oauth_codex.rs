//! BFF handlers for Codex OAuth device login.
//!
//! Provides session-managed endpoints so the Dashboard can drive the
//! multi-step device-authorization flow without a callback server.

use crate::state::AppState;
use crate::upstream_profiles;
use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use crab_admin_types::oauth::*;
use crab_auth::oauth::codex::CodexAuthenticator;
use crab_auth::store::{FileTokenStore, TokenStore};
use std::sync::Arc;
use std::path::PathBuf;
use uuid::Uuid;

/// Resolve the proxy_url for a profile from the gateway.
async fn resolve_profile_proxy_url(state: &AppState, profile_id: &str) -> Option<String> {
    state
        .gateway
        .list_upstream_profiles()
        .await
        .ok()
        .and_then(|resp| {
            resp.profiles
                .into_iter()
                .find(|p| p.id == profile_id)
                .and_then(|p| p.proxy_url)
        })
}

/// Resolve the auth credential directory from env or default.
pub fn resolve_auth_dir() -> PathBuf {
    std::env::var("CRABCACHE_AUTH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".crabcache").join("auths")
        })
}

/// Session metadata for one PKCE authorization attempt.
#[derive(Clone)]
pub struct CodexPkceSession {
    pub verifier: String,
    pub state: String,
    pub redirect_uri: String,
    pub profile_id: String,
    /// `"auto"` if callback listener running; `"manual"` otherwise.
    pub mode: String,
    /// `"pending"` | `"completed"` | `"failed"` | `"expired"`.
    pub status: String,
    pub credential_id: Option<String>,
    pub error: Option<String>,
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

impl CodexPkceSession {
    fn is_expired(&self) -> bool {
        (Utc::now() - self.created_at).num_seconds() > 300 // 5 min timeout
    }

    fn into_status_response(&self) -> CodexPkceExchangeResponse {
        CodexPkceExchangeResponse {
            status: if self.is_expired() && self.status == "pending" {
                "expired".to_string()
            } else {
                self.status.clone()
            },
            credential_id: self.credential_id.clone(),
            email: self.email.clone(),
            account_id: self.account_id.clone(),
            error: self.error.clone(),
        }
    }
}

/// POST `/api/admin/upstream/profiles/:id/oauth/codex/pkce/start`
pub async fn start_pkce_login(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> Result<Json<CodexPkceStartResponse>, (axum::http::StatusCode, String)> {
    let (verifier, _challenge, state_param, auth_url) =
        crab_auth::oauth::codex::start_pkce_login();

    let session_id = Uuid::new_v4();
    let redirect_uri = format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}");

    // Resolve proxy for this profile.
    let proxy_url = resolve_profile_proxy_url(&state, &profile_id).await;

    // Try to bind the callback port for auto mode.
    let mode = match tokio::net::TcpListener::bind(format!("0.0.0.0:{CALLBACK_PORT}")).await {
        Ok(listener) => {
            // Port is available — spawn auto callback listener.
            let sessions = state.codex_pkce_sessions.clone();
            let sid = session_id;
            let expected_state = state_param.clone();
            let verifier_clone = verifier.clone();
            let redirect_uri_clone = redirect_uri.clone();
            let profile_id_clone = profile_id.clone();
            let auth_dir = state.auth_dir.clone();
            let state_arc = state.clone();
            let proxy_for_exchange = proxy_url.clone();

            tokio::spawn(async move {
                // Wait up to 5 min for the callback.
                let accept_result =
                    tokio::time::timeout(std::time::Duration::from_secs(300), async {
                        loop {
                            let Ok((mut stream, _)) = listener.accept().await else {
                                continue;
                            };
                            // Read the HTTP request.
                            let mut buf = vec![0u8; 4096];
                            let n = match tokio::io::AsyncReadExt::read(
                                &mut stream,
                                &mut buf,
                            )
                            .await
                            {
                                Ok(n) => n,
                                Err(_) => continue,
                            };
                            let request = String::from_utf8_lossy(&buf[..n]);
                            let first_line = request.lines().next().unwrap_or("");
                            let parts: Vec<&str> = first_line.split_whitespace().collect();
                            if parts.len() < 2 {
                                continue;
                            }
                            let path_with_query = parts[1];
                            if let Some(qpos) = path_with_query.find('?') {
                                let query = &path_with_query[qpos + 1..];
                                let mut code = None;
                                let mut recv_state = None;
                                for param in query.split('&') {
                                    if let Some((k, v)) = param.split_once('=') {
                                        match k {
                                            "code" => code = Some(urldecode(v)),
                                            "state" => recv_state = Some(urldecode(v)),
                                            _ => {}
                                        }
                                    }
                                }
                                if let (Some(code), Some(recv_state)) = (code, recv_state)
                                    && recv_state == expected_state
                                {
                                    // Send a success page to the browser before closing.
                                    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n\
                                        <html><body style='font-family:sans-serif;text-align:center;padding:60px'>\
                                        <h2>Login Successful</h2><p>You can close this tab.</p></body></html>";
                                    use tokio::io::AsyncWriteExt;
                                    let _ = stream.write_all(response.as_bytes()).await;
                                    return Some(code);
                                }
                            }
                        }
                    })
                    .await;

                if let Ok(Some(code)) = accept_result {
                    // Exchange code for token (with proxy if configured).
                    let token_url = std::env::var("CRABCACHE_OAUTH_CODEX_TOKEN_URL")
                        .unwrap_or_else(|_| "https://auth.openai.com/oauth/token".to_string());
                    match crab_auth::oauth::codex::complete_pkce_exchange_with_url_and_proxy(
                        &code,
                        &verifier_clone,
                        &redirect_uri_clone,
                        &token_url,
                        proxy_for_exchange.as_deref(),
                    )
                    .await
                    {
                        Ok(record) => {
                            let email = record.email.clone();
                            let account_id = record
                                .metadata
                                .get("account_id")
                                .and_then(|v| v.as_str())
                                .map(String::from);

                            // Save credential.
                            let store = FileTokenStore::new(&auth_dir);
                            let credential_id = store.save(&record).await.ok();

                            // Append to key pool.
                            let auth_id = account_id
                                .clone()
                                .unwrap_or_else(|| email.clone().unwrap_or_default());
                            let _ = upstream_profiles::put_profile_keys_append(
                                &state_arc,
                                &profile_id_clone,
                                &record.access_token,
                                &auth_id,
                            )
                            .await;

                            if let Some(mut entry) = sessions.get_mut(&sid) {
                                entry.status = "completed".to_string();
                                entry.credential_id = credential_id;
                                entry.email = email;
                                entry.account_id = account_id;
                            }
                        }
                        Err(e) => {
                            if let Some(mut entry) = sessions.get_mut(&sid) {
                                entry.status = "failed".to_string();
                                entry.error = Some(format!("exchange failed: {e}"));
                            }
                        }
                    }
                } else if let Some(mut entry) = sessions.get_mut(&sid) {
                    entry.status = "expired".to_string();
                }
            });

            "auto".to_string()
        }
        Err(_) => "manual".to_string(),
    };

    state.codex_pkce_sessions.insert(
        session_id,
        CodexPkceSession {
            verifier,
            state: state_param,
            redirect_uri,
            profile_id: profile_id.clone(),
            mode: mode.clone(),
            status: "pending".to_string(),
            credential_id: None,
            error: None,
            email: None,
            account_id: None,
            created_at: Utc::now(),
        },
    );

    Ok(Json(CodexPkceStartResponse {
        session_id: session_id.to_string(),
        auth_url,
        mode,
    }))
}

/// POST `/api/admin/upstream/profiles/:id/oauth/codex/pkce/exchange`
pub async fn exchange_pkce(
    State(state): State<Arc<AppState>>,
    Path((profile_id, session_id_str)): Path<(String, String)>,
    Json(req): Json<CodexPkceExchangeRequest>,
) -> Result<Json<CodexPkceExchangeResponse>, (axum::http::StatusCode, String)> {
    let session_id = Uuid::parse_str(&session_id_str).map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid session id".to_string(),
        )
    })?;

    let entry = state
        .codex_pkce_sessions
        .get(&session_id)
        .ok_or_else(|| {
            (
                axum::http::StatusCode::NOT_FOUND,
                "PKCE session not found".to_string(),
            )
        })?;

    if entry.profile_id != profile_id {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "Session not found for this profile".to_string(),
        ));
    }

    if entry.status != "pending" || entry.is_expired() {
        return Ok(Json(entry.into_status_response()));
    }

    let (code, recv_state) =
        crab_auth::oauth::codex::extract_code_from_callback_url(&req.callback_url).map_err(
            |e| {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    format!("Invalid callback URL: {e}"),
                )
            },
        )?;

    if recv_state != entry.state {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "State mismatch — did you paste the correct URL?".to_string(),
        ));
    }

    let verifier = entry.verifier.clone();
    let redirect_uri = entry.redirect_uri.clone();
    drop(entry);

    let proxy_url = resolve_profile_proxy_url(&state, &profile_id).await;
    let token_url = std::env::var("CRABCACHE_OAUTH_CODEX_TOKEN_URL")
        .unwrap_or_else(|_| "https://auth.openai.com/oauth/token".to_string());

    let record = crab_auth::oauth::codex::complete_pkce_exchange_with_url_and_proxy(
        &code,
        &verifier,
        &redirect_uri,
        &token_url,
        proxy_url.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("Token exchange failed: {e}"),
        )
    })?;

    let email = record.email.clone();
    let account_id = record
        .metadata
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Save credential.
    let store = FileTokenStore::new(&state.auth_dir);
    let credential_id = store.save(&record).await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save credential: {e}"),
        )
    })?;

    // Append to key pool.
    let auth_id = account_id
        .clone()
        .unwrap_or_else(|| email.clone().unwrap_or_default());
    upstream_profiles::put_profile_keys_append(
        &state,
        &profile_id,
        &record.access_token,
        &auth_id,
    )
    .await?;

    if let Some(mut entry) = state.codex_pkce_sessions.get_mut(&session_id) {
        entry.status = "completed".to_string();
        entry.credential_id = Some(credential_id.clone());
        entry.email = email.clone();
        entry.account_id = account_id.clone();
    }

    Ok(Json(CodexPkceExchangeResponse {
        status: "completed".to_string(),
        credential_id: Some(credential_id),
        email,
        account_id,
        error: None,
    }))
}

/// GET `/api/admin/upstream/profiles/:id/oauth/codex/pkce/:session_id`
pub async fn poll_pkce_status(
    State(state): State<Arc<AppState>>,
    Path((profile_id, session_id_str)): Path<(String, String)>,
) -> Result<Json<CodexPkceExchangeResponse>, (axum::http::StatusCode, String)> {
    let session_id = Uuid::parse_str(&session_id_str).map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid session id".to_string(),
        )
    })?;

    let entry = state.codex_pkce_sessions.get(&session_id).ok_or_else(|| {
        (
            axum::http::StatusCode::NOT_FOUND,
            "PKCE session not found".to_string(),
        )
    })?;

    if entry.profile_id != profile_id {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "Session not found for this profile".to_string(),
        ));
    }

    Ok(Json(entry.into_status_response()))
}

/// DELETE `/api/admin/upstream/profiles/:id/oauth/codex/pkce/:session_id`
pub async fn cancel_pkce_login(
    State(state): State<Arc<AppState>>,
    Path((profile_id, session_id_str)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let session_id = Uuid::parse_str(&session_id_str).map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid session id".to_string(),
        )
    })?;

    match state.codex_pkce_sessions.get(&session_id) {
        Some(s) if s.profile_id != profile_id => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                "Session not found for this profile".to_string(),
            ));
        }
        None => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                "PKCE session not found".to_string(),
            ));
        }
        _ => {}
    }
    state.codex_pkce_sessions.remove(&session_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Percent-decode a query value.
fn urldecode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

const CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
pub struct CodexDeviceSession {
    pub device_auth_id: String,
    pub user_code: String,
    pub poll_interval_secs: u64,
    pub profile_id: String,
    pub created_at: chrono::DateTime<Utc>,
    pub status: String, // "pending" | "completed" | "failed" | "expired"
    pub credential_id: Option<String>,
    pub error: Option<String>,
    pub email: Option<String>,
    pub account_id: Option<String>,
}

impl CodexDeviceSession {
    fn is_expired(&self) -> bool {
        (Utc::now() - self.created_at).num_seconds() > 900 // 15 min match DEVICE_POLL_TIMEOUT_SECS
    }

    fn into_status_response(&self) -> CodexDeviceStatusResponse {
        CodexDeviceStatusResponse {
            status: if self.is_expired() && self.status == "pending" {
                "expired".to_string()
            } else {
                self.status.clone()
            },
            user_code: self.user_code.clone(),
            email: self.email.clone(),
            account_id: self.account_id.clone(),
            credential_id: self.credential_id.clone(),
            error: self.error.clone(),
        }
    }
}

/// POST `/api/admin/upstream/profiles/:id/oauth/codex/device/start`
pub async fn start_device_login(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> Result<Json<CodexDeviceStartResponse>, (axum::http::StatusCode, String)> {
    let start = CodexAuthenticator::start_device_usercode()
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Failed to start device login: {e}"),
            )
        })?;

    let session_id = Uuid::new_v4();
    let session = CodexDeviceSession {
        device_auth_id: start.device_auth_id.clone(),
        user_code: start.user_code.clone(),
        poll_interval_secs: start.poll_interval_secs,
        profile_id: profile_id.clone(),
        created_at: Utc::now(),
        status: "pending".to_string(),
        credential_id: None,
        error: None,
        email: None,
        account_id: None,
    };

    state.codex_device_sessions.insert(session_id, session);

    Ok(Json(CodexDeviceStartResponse {
        session_id: session_id.to_string(),
        user_code: start.user_code,
        verify_url: start.verify_url,
        poll_interval_secs: start.poll_interval_secs,
    }))
}

/// GET `/api/admin/upstream/profiles/:id/oauth/codex/device/:session_id`
pub async fn poll_device_status(
    State(state): State<Arc<AppState>>,
    Path((profile_id, session_id_str)): Path<(String, String)>,
) -> Result<Json<CodexDeviceStatusResponse>, (axum::http::StatusCode, String)> {
    let session_id = Uuid::parse_str(&session_id_str).map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid session id".to_string(),
        )
    })?;

    let entry = state
        .codex_device_sessions
        .get(&session_id)
        .ok_or_else(|| {
            (
                axum::http::StatusCode::NOT_FOUND,
                "Device session not found".to_string(),
            )
        })?;

    if entry.profile_id != profile_id {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            "Session not found for this profile".to_string(),
        ));
    }

    // Already terminal
    if entry.status != "pending" || entry.is_expired() {
        return Ok(Json(entry.into_status_response()));
    }

    // Extract fields needed for async calls, then drop the guard.
    let device_auth_id = entry.device_auth_id.clone();
    let user_code = entry.user_code.clone();
    drop(entry);

    let proxy_url = resolve_profile_proxy_url(&state, &profile_id).await;
    let token_url = std::env::var("CRABCACHE_OAUTH_CODEX_TOKEN_URL")
        .unwrap_or_else(|_| "https://auth.openai.com/oauth/token".to_string());

    // Single poll
    match CodexAuthenticator::poll_device_once_with_url_and_proxy(
        &token_url,
        &device_auth_id,
        &user_code,
        proxy_url.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("Poll failed: {e}"),
        )
    })? {
        crab_auth::oauth::CodexDevicePollResult::Pending => {
            // still pending
            if let Some(entry) = state.codex_device_sessions.get(&session_id) {
                Ok(Json(entry.into_status_response()))
            } else {
                Err((
                    axum::http::StatusCode::NOT_FOUND,
                    "Device session not found".to_string(),
                ))
            }
        }
        crab_auth::oauth::CodexDevicePollResult::Ready { body } => {
            // Complete the exchange
            let record =
                CodexAuthenticator::complete_device_from_poll_with_url_and_proxy(
                    &token_url,
                    &body,
                    proxy_url.as_deref(),
                )
                .await
                .map_err(|e| {
                    (
                        axum::http::StatusCode::BAD_GATEWAY,
                        format!("Token exchange failed: {e}"),
                    )
                })?;

            let email = record.email.clone();
            let account_id = record
                .metadata
                .get("account_id")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Save credential to auth directory
            let store = FileTokenStore::new(&state.auth_dir);
            let credential_id = store.save(&record).await.map_err(|e| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to save credential: {e}"),
                )
            })?;

            // Append access_token to profile key pool
            let auth_id_for_pool = account_id
                .clone()
                .unwrap_or_else(|| email.clone().unwrap_or_default());

            upstream_profiles::put_profile_keys_append(
                &state,
                &profile_id,
                &record.access_token,
                &auth_id_for_pool,
            )
            .await?;

            if let Some(mut entry) = state.codex_device_sessions.get_mut(&session_id) {
                entry.status = "completed".to_string();
                entry.credential_id = Some(credential_id.clone());
                entry.email = email.clone();
                entry.account_id = account_id.clone();
            }

            Ok(Json(CodexDeviceStatusResponse {
                status: "completed".to_string(),
                user_code,
                email,
                account_id,
                credential_id: Some(credential_id),
                error: None,
            }))
        }
        crab_auth::oauth::CodexDevicePollResult::Failed { status, body } => {
            if let Some(mut entry) = state.codex_device_sessions.get_mut(&session_id) {
                entry.status = "failed".to_string();
                entry.error = Some(format!("status {status}: {body}"));
                Ok(Json(entry.into_status_response()))
            } else {
                Ok(Json(CodexDeviceStatusResponse {
                    status: "failed".to_string(),
                    user_code,
                    email: None,
                    account_id: None,
                    credential_id: None,
                    error: Some(format!("status {status}: {body}")),
                }))
            }
        }
    }
}

/// DELETE `/api/admin/upstream/profiles/:id/oauth/codex/device/:session_id`
pub async fn cancel_device_login(
    State(state): State<Arc<AppState>>,
    Path((profile_id, session_id_str)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    let session_id = Uuid::parse_str(&session_id_str).map_err(|_| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid session id".to_string(),
        )
    })?;

    match state.codex_device_sessions.get(&session_id) {
        Some(s) if s.profile_id != profile_id => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                "Session not found for this profile".to_string(),
            ));
        }
        None => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                "Device session not found".to_string(),
            ));
        }
        _ => {}
    }
    state.codex_device_sessions.remove(&session_id);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// GET `/api/admin/oauth/codex/credentials`
pub async fn list_codex_credentials(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CodexCredentialListResponse>, (axum::http::StatusCode, String)> {
    let store = FileTokenStore::new(&state.auth_dir);
    let records = store.list().await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list credentials: {e}"),
        )
    })?;

    let credentials: Vec<CodexCredentialSummary> = records
        .into_iter()
        .filter(|r| r.provider == crab_auth::types::Provider::Codex)
        .map(|r| CodexCredentialSummary {
            id: r.id,
            email: r.email,
            plan_type: r
                .metadata
                .get("plan_type")
                .and_then(|v| v.as_str())
                .map(String::from),
            expired_at: r.expired_at.map(|dt| dt.to_rfc3339()),
            disabled: r.disabled,
        })
        .collect();

    Ok(Json(CodexCredentialListResponse { credentials }))
}

/// POST `/api/admin/upstream/profiles/:id/oauth/codex/import`
pub async fn import_codex_credential(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    Json(req): Json<CodexImportRequest>,
) -> Result<Json<CodexImportResponse>, (axum::http::StatusCode, String)> {
    let store = FileTokenStore::new(&state.auth_dir);
    let record = store.get(&req.credential_id).await.map_err(|e| {
        (
            axum::http::StatusCode::NOT_FOUND,
            format!("Credential not found: {e}"),
        )
    })?;

    let account_id = record
        .metadata
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_default();

    upstream_profiles::put_profile_keys_append(
        &state,
        &profile_id,
        &record.access_token,
        &account_id,
    )
    .await?;

    Ok(Json(CodexImportResponse {
        credential_id: req.credential_id,
        profile_id,
    }))
}
