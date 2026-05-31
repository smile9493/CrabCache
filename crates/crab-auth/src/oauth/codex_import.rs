//! Parse external Codex OAuth JSON exports and ensure tokens are fresh before use.

use super::codex::{CodexAuthenticator, credential_filename, hash_account_id, parse_jwt_payload};
use super::{AuthError, Authenticator};
use crate::types::{Provider, TokenRecord};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use std::collections::HashMap;

const REFRESH_SKEW: Duration = Duration::seconds(60);

/// True when `record` is expired or within `lead` of expiry.
pub fn token_needs_refresh(record: &TokenRecord, lead: Duration) -> bool {
    match record.expired_at {
        None => false,
        Some(exp) => exp <= Utc::now() + lead + REFRESH_SKEW,
    }
}

/// Refresh when expired/near expiry; returns `(record, was_refreshed)`.
pub async fn ensure_fresh_codex_token(
    record: &TokenRecord,
    proxy_url: Option<&str>,
) -> Result<(TokenRecord, bool), AuthError> {
    let auth = CodexAuthenticator::new();
    let lead = auth
        .refresh_lead()
        .and_then(|d| Duration::from_std(d).ok())
        .unwrap_or(Duration::zero());

    if !token_needs_refresh(record, lead) {
        return Ok((record.clone(), false));
    }

    let refresh_token = record
        .refresh_token
        .as_ref()
        .ok_or_else(|| AuthError::OAuth("access token expired and no refresh_token".into()))?;

    let mut attempt = record.clone();
    attempt.refresh_token = Some(refresh_token.clone());
    let refreshed = super::codex::refresh_codex_token_with_proxy(&attempt, proxy_url).await?;
    Ok((refreshed, true))
}

/// Parse bulk export JSON (`accounts[]`) or a single CLIProxyAPI `codex-*.json` object.
pub fn parse_codex_import_documents(value: &Value) -> Result<Vec<TokenRecord>, AuthError> {
    if value.get("accounts").and_then(Value::as_array).is_some() {
        return parse_bulk_accounts_export(value);
    }
    if looks_like_codex_credential(value) {
        return parse_single_codex_value(value, None).map(|r| vec![r]);
    }
    if let Some(arr) = value.as_array() {
        let mut out = Vec::new();
        for (i, item) in arr.iter().enumerate() {
            if looks_like_codex_credential(item) {
                out.push(parse_single_codex_value(item, None)?);
            } else if item.get("credentials").is_some() {
                out.push(parse_account_entry(item, i)?);
            }
        }
        if out.is_empty() {
            return Err(AuthError::OAuth(
                "JSON array contained no Codex/OpenAI OAuth accounts".into(),
            ));
        }
        return Ok(out);
    }
    Err(AuthError::OAuth(
        "unsupported JSON: expected {accounts:[...]} or a codex credential object".into(),
    ))
}

fn parse_bulk_accounts_export(root: &Value) -> Result<Vec<TokenRecord>, AuthError> {
    let accounts = root["accounts"]
        .as_array()
        .ok_or_else(|| AuthError::OAuth("missing accounts array".into()))?;
    let mut out = Vec::new();
    for (i, account) in accounts.iter().enumerate() {
        if !account_matches_codex(account) {
            continue;
        }
        match parse_account_entry(account, i) {
            Ok(record) => out.push(record),
            Err(err) => {
                let name = account_label(account, i);
                return Err(AuthError::OAuth(format!("account '{name}': {err}")));
            }
        }
    }
    if out.is_empty() {
        return Err(AuthError::OAuth(
            "no OpenAI/Codex OAuth accounts found in export".into(),
        ));
    }
    Ok(out)
}

fn account_matches_codex(account: &Value) -> bool {
    let platform = account["platform"]
        .as_str()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ty = account["type"].as_str().unwrap_or("").to_ascii_lowercase();
    if matches!(platform.as_str(), "openai" | "codex") {
        return true;
    }
    if ty == "oauth" && account.get("credentials").is_some() {
        return account["credentials"]["access_token"].as_str().is_some();
    }
    false
}

fn parse_account_entry(account: &Value, index: usize) -> Result<TokenRecord, AuthError> {
    let creds = account
        .get("credentials")
        .ok_or_else(|| AuthError::OAuth("missing credentials object".into()))?;
    let name = account_label(account, index);
    parse_single_codex_value(creds, Some(&name))
}

fn account_label(account: &Value, index: usize) -> String {
    account["name"]
        .as_str()
        .or_else(|| account["extra"]["email"].as_str())
        .or_else(|| account["credentials"]["email"].as_str())
        .unwrap_or("unknown")
        .to_string()
        + &format!(" [#{index}]")
}

fn looks_like_codex_credential(value: &Value) -> bool {
    value.get("access_token").and_then(Value::as_str).is_some()
        && (value["type"].as_str() == Some("codex")
            || value.get("refresh_token").is_some()
            || value.get("id_token").is_some())
}

fn parse_single_codex_value(creds: &Value, label: Option<&str>) -> Result<TokenRecord, AuthError> {
    let access_token = creds["access_token"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AuthError::OAuth(format!(
                "{}missing access_token",
                label.map(|l| format!("{l}: ")).unwrap_or_default()
            ))
        })?
        .to_string();

    let refresh_token = creds["refresh_token"].as_str().map(String::from);
    let id_token = creds["id_token"].as_str().map(String::from);

    let mut email = creds["email"].as_str().map(String::from);
    let mut account_id = creds["account_id"].as_str().map(String::from);
    let mut plan_type = creds["plan_type"].as_str().map(String::from);

    if let Some(jwt) = &id_token {
        if let Ok(claims) = parse_jwt_payload(jwt) {
            email = email.or(claims.email);
            account_id = account_id.or(claims.account_id);
            plan_type = plan_type.or(claims.plan_type);
        }
    }

    let expired_at = parse_expiry(creds);
    let last_refresh =
        parse_dt_field(creds, "last_refresh").or_else(|| parse_dt_field(creds, "lastRefresh"));

    let account_id_hash = account_id.as_deref().map(hash_account_id);
    let id = email
        .as_deref()
        .map(|e| {
            credential_filename(e, plan_type.as_deref(), account_id_hash.as_deref())
                .trim_end_matches(".json")
                .to_string()
        })
        .unwrap_or_else(|| {
            label
                .map(|l| l.replace(['/', '\\', ':'], "_"))
                .unwrap_or_else(|| "codex-import".to_string())
        });

    let mut metadata = HashMap::new();
    if let Some(v) = account_id {
        metadata.insert("account_id".to_owned(), Value::String(v));
    }
    if let Some(v) = plan_type {
        metadata.insert("plan_type".to_owned(), Value::String(v));
    }

    Ok(TokenRecord {
        id,
        provider: Provider::Codex,
        access_token,
        refresh_token,
        id_token,
        email,
        expired_at,
        last_refresh,
        disabled: false,
        metadata,
        file_path: None,
    })
}

fn parse_expiry(creds: &Value) -> Option<DateTime<Utc>> {
    parse_dt_field(creds, "expires_at")
        .or_else(|| parse_dt_field(creds, "expired"))
        .or_else(|| parse_dt_field(creds, "expire"))
        .or_else(|| {
            creds["expires_in"]
                .as_i64()
                .map(|secs| Utc::now() + Duration::seconds(secs))
        })
}

fn parse_dt_field(map: &Value, key: &str) -> Option<DateTime<Utc>> {
    map.get(key)
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s.trim()).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bulk_accounts_export_json() {
        let raw: Value = serde_json::json!({
            "accounts": [{
                "name": "user@example.com",
                "platform": "openai",
                "type": "oauth",
                "credentials": {
                    "access_token": "at-123",
                    "refresh_token": "rt-456",
                    "email": "user@example.com",
                    "expires_at": "2030-01-01T00:00:00Z"
                }
            }]
        });
        let records = parse_codex_import_documents(&raw).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].access_token, "at-123");
        assert_eq!(records[0].email.as_deref(), Some("user@example.com"));
    }

    #[test]
    fn parse_cli_proxy_codex_object() {
        let raw: Value = serde_json::json!({
            "type": "codex",
            "access_token": "at-abc",
            "refresh_token": "rt-def",
            "email": "a@b.com",
            "expired": "2030-06-01T00:00:00Z"
        });
        let records = parse_codex_import_documents(&raw).unwrap();
        assert_eq!(records[0].id, "codex-a@b.com");
    }

    #[test]
    fn token_needs_refresh_when_expired() {
        let record = TokenRecord {
            id: "x".into(),
            provider: Provider::Codex,
            access_token: "t".into(),
            refresh_token: Some("r".into()),
            id_token: None,
            email: None,
            expired_at: Some(Utc::now() - Duration::hours(1)),
            last_refresh: None,
            disabled: false,
            metadata: HashMap::new(),
            file_path: None,
        };
        assert!(token_needs_refresh(&record, Duration::hours(4)));
    }
}
