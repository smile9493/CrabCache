//! BFF handlers for Codex OAuth device login.
//!
//! Provides session-managed endpoints so the Dashboard can drive the
//! multi-step device-authorization flow without a callback server.

use crate::state::{AppState, UpstreamPoolSecret};
use crate::upstream_profiles;
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use chrono::Utc;
use crab_admin_types::oauth::*;
use crab_auth::oauth::codex::CodexAuthenticator;
use crab_auth::oauth::codex::decode_codex_access_token_claims;
use crab_auth::oauth::{ensure_fresh_codex_token, parse_codex_import_documents};
use crab_auth::store::{FileTokenStore, TokenStore};
use crab_auth::types::{Provider, TokenRecord};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

/// Resolve directory for OAuth credential JSON files.
/// Defaults to `{admin-state-parent}/auths` so Docker `admin_data:/app/data` persists Codex accounts.
pub fn resolve_auth_dir() -> PathBuf {
    std::env::var("CRABCACHE_AUTH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let state_path = std::env::var("CRABCACHE_ADMIN_STATE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("data/admin-state.json"));
            if let Some(parent) = state_path.parent() {
                if !parent.as_os_str().is_empty() {
                    return parent.join("auths");
                }
            }
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".crabcache").join("auths")
        })
}

/// Ensure persistent auth dir exists; migrate legacy paths; sync credentials with key pool.
pub async fn prepare_auth_dir(state: &AppState) {
    let auth_dir = &state.auth_dir;
    if tokio::fs::create_dir_all(auth_dir).await.is_err() {
        tracing::warn!(path = %auth_dir.display(), "Failed to create Codex auth dir");
        return;
    }
    let migrated = migrate_legacy_auth_dirs(auth_dir).await;

    // Step 1: Recover key pool from credential files if pool was lost
    // (e.g. admin-state.json corrupted or missing after a hot-update).
    let recovered = recover_codex_pool_from_credentials(state).await;

    let profile_secrets = state.upstream_profile_secrets.read().clone();
    let codex_profiles = codex_like_profile_ids(state, &profile_secrets);
    let mut synced = 0usize;
    for profile_id in &codex_profiles {
        synced += reconcile_codex_credentials_with_pool(state, profile_id).await;
    }
    tracing::info!(
        path = %auth_dir.display(),
        migrated,
        recovered,
        synced,
        codex_profiles = ?codex_profiles,
        "Codex auth directory ready"
    );
}

/// If a codex-like profile's key pool is empty but credential files exist on
/// disk, rebuild the key pool from those credentials. This handles the case
/// where `admin-state.json` was lost/corrupted between hot-updates while
/// credential JSON files in `auth_dir` survived (different persistence path).
async fn recover_codex_pool_from_credentials(state: &AppState) -> usize {
    let store = FileTokenStore::new(&state.auth_dir);
    let Ok(records) = store.list().await else {
        return 0;
    };
    let codex_records: Vec<&TokenRecord> = records
        .iter()
        .filter(|r| r.provider == Provider::Codex && !r.access_token.is_empty())
        .collect();
    if codex_records.is_empty() {
        return 0;
    }

    let providers = state.upstream_profile_providers.read().clone();
    let mut recovered = 0usize;

    // Group credentials by profile_id.
    let mut by_profile: HashMap<String, Vec<&TokenRecord>> = HashMap::new();
    for rec in &codex_records {
        let profile_id = rec
            .metadata
            .get("profile_id")
            .and_then(|v| v.as_str())
            .unwrap_or("codex")
            .to_string();
        by_profile.entry(profile_id).or_default().push(rec);
    }

    for (profile_id, creds) in &by_profile {
        // Only recover for codex-like profiles.
        let is_codex = providers
            .get(profile_id)
            .map(|p| p == "codex" || p == "openai")
            .unwrap_or_else(|| profile_id == "codex");
        if !is_codex {
            continue;
        }

        // Only recover if the pool is currently empty.
        let pool_empty = state
            .upstream_profile_secrets
            .read()
            .get(profile_id)
            .map(|p| p.is_empty())
            .unwrap_or(true);
        if !pool_empty {
            continue;
        }

        let secrets: Vec<UpstreamPoolSecret> = creds
            .iter()
            .map(|rec| {
                let account_id = rec
                    .metadata
                    .get("account_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                UpstreamPoolSecret {
                    id: rec
                        .metadata
                        .get("key_id")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| rec.id.clone()),
                    secret: rec.access_token.clone(),
                    enabled: !rec.disabled,
                    account_id,
                }
            })
            .collect();

        if secrets.is_empty() {
            continue;
        }

        let count = secrets.len();
        state
            .upstream_profile_secrets
            .write()
            .insert(profile_id.clone(), secrets.clone());
        recovered += count;
        tracing::info!(
            profile_id = %profile_id,
            count,
            "Recovered key pool from credential files"
        );
        if let Err(e) = crate::upstream_profiles::sync_profile_pool_to_gateway(
            state,
            profile_id,
            crate::types::UpstreamKeysPutMode::Replace,
        )
        .await
        {
            tracing::warn!(
                profile_id = %profile_id,
                error = %e,
                "Recovered Codex keys in admin state but failed to push to gateway"
            );
        }
    }

    if recovered > 0 {
        state.flush_persist();
    }

    recovered
}

/// True when profile uses Codex OAuth key pool semantics.
pub fn profile_is_codex_like(state: &AppState, profile_id: &str) -> bool {
    let providers = state.upstream_profile_providers.read();
    providers
        .get(profile_id)
        .map(|p| p == "codex" || p == "openai")
        .unwrap_or_else(|| profile_id == "codex")
}

/// Keep `auth_dir` credentials aligned with gateway key pool (one logical account per pool slot).
pub async fn reconcile_codex_credentials_with_pool(state: &AppState, profile_id: &str) -> usize {
    if !profile_is_codex_like(state, profile_id) {
        return 0;
    }

    let Ok(keys_view) = state.gateway.get_upstream_profile_keys(profile_id).await else {
        return 0;
    };

    let export_secrets: HashMap<String, String> = state
        .gateway
        .export_upstream_profile_keys(profile_id)
        .await
        .map(|export| export.keys.into_iter().map(|k| (k.id, k.secret)).collect())
        .unwrap_or_default();

    let cached_secrets: HashMap<String, String> = state
        .upstream_profile_secrets
        .read()
        .get(profile_id)
        .map(|pool| {
            pool.iter()
                .map(|s| (s.id.clone(), s.secret.clone()))
                .collect()
        })
        .unwrap_or_default();

    let store = FileTokenStore::new(&state.auth_dir);
    let existing = store.list().await.unwrap_or_default();
    let mut touched = 0usize;

    for key in keys_view.keys {
        let access_token = cached_secrets
            .get(&key.id)
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .or_else(|| {
                export_secrets
                    .get(&key.id)
                    .filter(|s| !s.trim().is_empty())
                    .cloned()
            });
        let Some(access_token) = access_token else {
            continue;
        };

        let (jwt_email, jwt_plan, jwt_account) = decode_codex_access_token_claims(&access_token);
        let account_id = jwt_account
            .as_deref()
            .map(normalize_pool_account_id)
            .filter(|a| a != "default")
            .unwrap_or_else(|| normalize_pool_account_id(&key.account_id));

        let mut record = find_credential_for_account(&existing, &account_id)
            .cloned()
            .unwrap_or_else(|| {
                let id = format!("codex-{account_id}").replace(['/', '\\', ':'], "_");
                TokenRecord {
                    id,
                    provider: Provider::Codex,
                    access_token: String::new(),
                    refresh_token: None,
                    id_token: None,
                    email: None,
                    expired_at: None,
                    last_refresh: None,
                    disabled: false,
                    metadata: HashMap::new(),
                    file_path: None,
                }
            });

        record.access_token = access_token;
        record.disabled = !key.enabled;
        if record
            .email
            .as_ref()
            .is_none_or(|e| e.trim().is_empty() || e == &account_id)
        {
            record.email = jwt_email.clone().or_else(|| record.email.clone());
        }
        record.metadata.insert(
            "profile_id".to_string(),
            serde_json::Value::String(profile_id.to_string()),
        );
        record.metadata.insert(
            "account_id".to_string(),
            serde_json::Value::String(account_id.clone()),
        );
        record.metadata.insert(
            "key_id".to_string(),
            serde_json::Value::String(key.id.clone()),
        );
        if let Some(plan) = jwt_plan {
            record
                .metadata
                .insert("plan_type".to_string(), serde_json::Value::String(plan));
        }

        if store.save(&record).await.is_ok() {
            touched += 1;
        }
    }

    touched
}

fn find_credential_for_account<'a>(
    records: &'a [TokenRecord],
    account_id: &str,
) -> Option<&'a TokenRecord> {
    let normalized = normalize_pool_account_id(account_id);
    records
        .iter()
        .filter(|r| r.provider == Provider::Codex)
        .filter(|r| {
            r.metadata
                .get("account_id")
                .and_then(|v| v.as_str())
                .map(normalize_pool_account_id)
                == Some(normalized.clone())
        })
        .max_by_key(|r| {
            u8::from(
                r.refresh_token
                    .as_ref()
                    .is_some_and(|t| !t.trim().is_empty()),
            )
        })
}

/// Fill key-pool card email/plan from unified credential store when probe data is missing.
pub async fn enrich_upstream_keys_from_credentials(
    state: &AppState,
    keys: &mut [crab_admin_types::upstream::UpstreamKeyView],
) {
    let store = FileTokenStore::new(&state.auth_dir);
    let Ok(records) = store.list().await else {
        return;
    };
    for key in keys.iter_mut() {
        let account_id = normalize_pool_account_id(&key.account_id);
        let Some(rec) = find_credential_for_account(&records, &account_id) else {
            continue;
        };
        if key.email.is_none() {
            key.email = rec.email.clone();
        }
        if key.plan_type.is_none() {
            key.plan_type = rec
                .metadata
                .get("plan_type")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }
}

fn codex_like_profile_ids(
    state: &AppState,
    profile_secrets: &HashMap<String, Vec<UpstreamPoolSecret>>,
) -> HashSet<String> {
    let providers = state.upstream_profile_providers.read();
    profile_secrets
        .keys()
        .filter(|id| {
            providers
                .get(*id)
                .map(|p| p == "codex" || p == "openai")
                .unwrap_or_else(|| *id == "codex")
        })
        .cloned()
        .collect()
}

fn normalize_pool_account_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn migrate_legacy_auth_dirs(target: &Path) -> usize {
    let mut legacy = vec![
        PathBuf::from("/app/.crabcache/auths"),
        PathBuf::from(".crabcache/auths"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        legacy.push(PathBuf::from(home).join(".crabcache").join("auths"));
    }
    let mut copied = 0usize;
    for src in legacy {
        if src == target {
            continue;
        }
        let Ok(mut entries) = tokio::fs::read_dir(&src).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let dest = target.join(name);
            if dest.exists() {
                continue;
            }
            if tokio::fs::copy(&path, &dest).await.is_ok() {
                copied += 1;
            }
        }
    }
    copied
}

/// Save credential (refresh if needed) and upsert into profile key pool.
async fn import_codex_record_to_profile(
    state: &Arc<AppState>,
    profile_id: &str,
    record: TokenRecord,
) -> Result<(String, bool, Option<String>, Option<String>), (axum::http::StatusCode, String)> {
    let proxy_url = resolve_profile_proxy_url(state, profile_id).await;
    let (fresh, refreshed) = ensure_fresh_codex_token(&record, proxy_url.as_deref())
        .await
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Token refresh failed: {e}"),
            )
        })?;

    let mut fresh = fresh;
    fresh.metadata.insert(
        "profile_id".to_string(),
        serde_json::Value::String(profile_id.to_string()),
    );

    let store = FileTokenStore::new(&state.auth_dir);
    let credential_id = store.save(&fresh).await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save credential: {e}"),
        )
    })?;

    let account_id = fresh
        .metadata
        .get("account_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let email = fresh.email.clone();

    upstream_profiles::put_profile_keys_upsert(state, profile_id, &fresh.access_token, &account_id)
        .await?;

    reconcile_codex_credentials_with_pool(state, profile_id).await;

    Ok((
        credential_id,
        refreshed,
        email,
        if account_id.is_empty() {
            None
        } else {
            Some(account_id)
        },
    ))
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
    AxumPath(profile_id): AxumPath<String>,
) -> Result<Json<CodexPkceStartResponse>, (axum::http::StatusCode, String)> {
    let (verifier, _challenge, state_param, auth_url) = crab_auth::oauth::codex::start_pkce_login();

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
                            match import_codex_record_to_profile(
                                &state_arc,
                                &profile_id_clone,
                                record,
                            )
                            .await
                            {
                                Ok((credential_id, _refreshed, email, account_id)) => {
                                    if let Some(mut entry) = sessions.get_mut(&sid) {
                                        entry.status = "completed".to_string();
                                        entry.credential_id = Some(credential_id);
                                        entry.email = email;
                                        entry.account_id = account_id;
                                    }
                                }
                                Err((_, err)) => {
                                    if let Some(mut entry) = sessions.get_mut(&sid) {
                                        entry.status = "failed".to_string();
                                        entry.error = Some(err);
                                    }
                                }
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
    AxumPath((profile_id, session_id_str)): AxumPath<(String, String)>,
    Json(req): Json<CodexPkceExchangeRequest>,
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

    if entry.status != "pending" || entry.is_expired() {
        return Ok(Json(entry.into_status_response()));
    }

    let (code, recv_state) = crab_auth::oauth::codex::extract_code_from_callback_url(
        &req.callback_url,
    )
    .map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid callback URL: {e}"),
        )
    })?;

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

    let (credential_id, _refreshed, email, account_id) =
        import_codex_record_to_profile(&state, &profile_id, record).await?;

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
    AxumPath((profile_id, session_id_str)): AxumPath<(String, String)>,
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
    AxumPath((profile_id, session_id_str)): AxumPath<(String, String)>,
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
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
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
    AxumPath(profile_id): AxumPath<String>,
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
    AxumPath((profile_id, session_id_str)): AxumPath<(String, String)>,
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
    let device_poll_url = crab_auth::oauth::codex::codex_device_token_endpoint();
    let oauth_token_url = crab_auth::oauth::codex::codex_oauth_token_endpoint();

    // Single poll — device auth uses JSON on the deviceauth/token endpoint.
    match CodexAuthenticator::poll_device_once_with_url_and_proxy(
        &device_poll_url,
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
            // Complete the exchange on the OAuth token endpoint (form-urlencoded).
            let record = CodexAuthenticator::complete_device_from_poll_with_url_and_proxy(
                &oauth_token_url,
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

            let (credential_id, _refreshed, email, account_id) =
                import_codex_record_to_profile(&state, &profile_id, record).await?;

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
    AxumPath((profile_id, session_id_str)): AxumPath<(String, String)>,
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

/// GET `/api/admin/oauth/codex/credentials` (legacy: all credentials)
pub async fn list_codex_credentials(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CodexCredentialListResponse>, (axum::http::StatusCode, String)> {
    list_codex_credentials_for_profile(&state, None).await
}

/// GET `/api/admin/upstream/profiles/:id/oauth/codex/credentials`
pub async fn list_profile_codex_credentials(
    State(state): State<Arc<AppState>>,
    AxumPath(profile_id): AxumPath<String>,
) -> Result<Json<CodexCredentialListResponse>, (axum::http::StatusCode, String)> {
    list_codex_credentials_for_profile(&state, Some(profile_id.as_str())).await
}

async fn list_codex_credentials_for_profile(
    state: &AppState,
    profile_id: Option<&str>,
) -> Result<Json<CodexCredentialListResponse>, (axum::http::StatusCode, String)> {
    if let Some(pid) = profile_id {
        reconcile_codex_credentials_with_pool(state, pid).await;

        let keys_view = state
            .gateway
            .get_upstream_profile_keys(pid)
            .await
            .map_err(|e| {
                (
                    axum::http::StatusCode::BAD_GATEWAY,
                    format!("Failed to list profile keys: {e}"),
                )
            })?;

        let store = FileTokenStore::new(&state.auth_dir);
        let records = store.list().await.map_err(|e| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list credentials: {e}"),
            )
        })?;

        let credentials: Vec<CodexCredentialSummary> = keys_view
            .keys
            .iter()
            .filter_map(|key| {
                let account_id = normalize_pool_account_id(&key.account_id);
                let record = find_credential_for_account(&records, &account_id)?;
                Some(credential_summary_from_record(record, Some(key.id.clone())))
            })
            .collect();

        return Ok(Json(CodexCredentialListResponse { credentials }));
    }

    let store = FileTokenStore::new(&state.auth_dir);
    let records = store.list().await.map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list credentials: {e}"),
        )
    })?;

    let credentials: Vec<CodexCredentialSummary> = records
        .into_iter()
        .filter(|r| r.provider == Provider::Codex)
        .map(|r| credential_summary_from_record(&r, None))
        .collect();

    Ok(Json(CodexCredentialListResponse { credentials }))
}

fn credential_summary_from_record(
    r: &TokenRecord,
    key_id: Option<String>,
) -> CodexCredentialSummary {
    let account_id = r
        .metadata
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let has_refresh_token = r
        .refresh_token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty());
    CodexCredentialSummary {
        id: r.id.clone(),
        email: r.email.clone(),
        plan_type: r
            .metadata
            .get("plan_type")
            .and_then(|v| v.as_str())
            .map(String::from),
        expired_at: r.expired_at.map(|dt| dt.to_rfc3339()),
        disabled: r.disabled,
        key_id: key_id.or_else(|| {
            r.metadata
                .get("key_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        }),
        account_id,
        has_refresh_token,
    }
}

/// POST `/api/admin/upstream/profiles/:id/oauth/codex/import`
pub async fn import_codex_credential(
    State(state): State<Arc<AppState>>,
    AxumPath(profile_id): AxumPath<String>,
    Json(req): Json<CodexImportRequest>,
) -> Result<Json<CodexImportResponse>, (axum::http::StatusCode, String)> {
    let store = FileTokenStore::new(&state.auth_dir);
    let record = store.get(&req.credential_id).await.map_err(|e| {
        (
            axum::http::StatusCode::NOT_FOUND,
            format!("Credential not found: {e}"),
        )
    })?;

    let (credential_id, refreshed, _, _) =
        import_codex_record_to_profile(&state, &profile_id, record).await?;

    Ok(Json(CodexImportResponse {
        credential_id,
        profile_id,
        refreshed,
    }))
}

/// POST `/api/admin/upstream/profiles/:id/oauth/codex/import/bulk`
pub async fn import_codex_bulk(
    State(state): State<Arc<AppState>>,
    AxumPath(profile_id): AxumPath<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<CodexBulkImportResponse>, (axum::http::StatusCode, String)> {
    let records = parse_codex_import_documents(&body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid import JSON: {e}"),
        )
    })?;

    let mut imported = Vec::new();
    let mut errors = Vec::new();

    for record in records {
        let name = record.email.clone().unwrap_or_else(|| record.id.clone());
        match import_codex_record_to_profile(&state, &profile_id, record).await {
            Ok((credential_id, refreshed, email, _)) => {
                imported.push(CodexBulkImportItem {
                    credential_id,
                    email,
                    refreshed,
                });
            }
            Err((_, err)) => {
                errors.push(CodexBulkImportError { name, error: err });
            }
        }
    }

    if imported.is_empty() && !errors.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_GATEWAY,
            format!("All imports failed: {}", errors[0].error),
        ));
    }

    Ok(Json(CodexBulkImportResponse {
        profile_id,
        imported,
        errors,
    }))
}
