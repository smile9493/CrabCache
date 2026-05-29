use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, warn};

use super::traits::{StoreError, TokenStore};
use crate::types::{Provider, TokenRecord};

pub struct FileTokenStore {
    dir: PathBuf,
}

impl FileTokenStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

/// Intermediate representation of a raw JSON file as CLIProxyAPI writes it.
#[derive(serde::Deserialize)]
struct RawFile {
    #[serde(rename = "type")]
    ty: Option<String>,
    #[serde(flatten)]
    rest: HashMap<String, Value>,
}

/// Intermediate for Gemini's nested `token` sub-object.
#[derive(serde::Deserialize)]
struct GeminiToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expiry: Option<String>,
    token_uri: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    scopes: Option<Vec<String>>,
}

fn parse_dt(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

fn record_from_json(id: &str, file_path: &Path, raw: &RawFile) -> Result<TokenRecord, StoreError> {
    let provider_str = raw
        .ty
        .as_deref()
        .or_else(|| id.split('_').next())
        .unwrap_or("claude");
    let provider = provider_str.parse::<Provider>().unwrap_or(Provider::Claude);

    match provider {
        Provider::Gemini => parse_gemini(id, file_path, &raw.rest),
        Provider::Codex => parse_codex(id, file_path, &raw.rest, provider),
        Provider::Xai => parse_xai(id, file_path, &raw.rest, provider),
        _ => parse_flat(id, file_path, &raw.rest, provider),
    }
}

fn extract_str(map: &HashMap<String, Value>, key: &str) -> Option<String> {
    match map.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn extract_bool(map: &HashMap<String, Value>, key: &str) -> bool {
    matches!(map.get(key), Some(Value::Bool(true)))
}

fn parse_flat(
    id: &str,
    file_path: &Path,
    map: &HashMap<String, Value>,
    provider: Provider,
) -> Result<TokenRecord, StoreError> {
    Ok(TokenRecord {
        id: id.to_owned(),
        provider,
        access_token: extract_str(map, "access_token").unwrap_or_default(),
        refresh_token: extract_str(map, "refresh_token"),
        id_token: extract_str(map, "id_token"),
        email: extract_str(map, "email"),
        expired_at: extract_str(map, "expired").as_deref().and_then(parse_dt),
        last_refresh: extract_str(map, "last_refresh")
            .as_deref()
            .and_then(parse_dt),
        disabled: extract_bool(map, "disabled"),
        metadata: HashMap::new(),
        file_path: Some(file_path.to_owned()),
    })
}

fn parse_codex(
    id: &str,
    file_path: &Path,
    map: &HashMap<String, Value>,
    provider: Provider,
) -> Result<TokenRecord, StoreError> {
    let mut metadata = HashMap::new();
    if let Some(v) = map.get("account_id").cloned() {
        metadata.insert("account_id".to_owned(), v);
    }
    if let Some(v) = map.get("plan_type").cloned() {
        metadata.insert("plan_type".to_owned(), v);
    }

    Ok(TokenRecord {
        id: id.to_owned(),
        provider,
        access_token: extract_str(map, "access_token").unwrap_or_default(),
        refresh_token: extract_str(map, "refresh_token"),
        id_token: extract_str(map, "id_token"),
        email: extract_str(map, "email"),
        expired_at: extract_str(map, "expired").as_deref().and_then(parse_dt),
        last_refresh: extract_str(map, "last_refresh")
            .as_deref()
            .and_then(parse_dt),
        disabled: extract_bool(map, "disabled"),
        metadata,
        file_path: Some(file_path.to_owned()),
    })
}

fn parse_xai(
    id: &str,
    file_path: &Path,
    map: &HashMap<String, Value>,
    provider: Provider,
) -> Result<TokenRecord, StoreError> {
    let mut metadata = HashMap::new();
    for key in ["base_url", "token_endpoint", "auth_kind", "token_type"] {
        if let Some(v) = map.get(key).cloned() {
            metadata.insert((*key).to_owned(), v);
        }
    }
    if let Some(v) = map.get("expires_in").cloned() {
        metadata.insert("expires_in".to_owned(), v);
    }

    Ok(TokenRecord {
        id: id.to_owned(),
        provider,
        access_token: extract_str(map, "access_token").unwrap_or_default(),
        refresh_token: extract_str(map, "refresh_token"),
        id_token: extract_str(map, "id_token"),
        email: extract_str(map, "email"),
        expired_at: extract_str(map, "expired").as_deref().and_then(parse_dt),
        last_refresh: extract_str(map, "last_refresh")
            .as_deref()
            .and_then(parse_dt),
        disabled: false,
        metadata,
        file_path: Some(file_path.to_owned()),
    })
}

fn parse_gemini(
    id: &str,
    file_path: &Path,
    map: &HashMap<String, Value>,
) -> Result<TokenRecord, StoreError> {
    let nested = map
        .get("token")
        .and_then(|v| serde_json::from_value::<GeminiToken>(v.clone()).ok());

    let (access_token, refresh_token, expired_at) = if let Some(t) = &nested {
        (
            t.access_token.clone().unwrap_or_default(),
            t.refresh_token.clone(),
            t.expiry.as_deref().and_then(parse_dt),
        )
    } else {
        (String::new(), None, None)
    };

    let mut metadata = HashMap::new();
    if let Some(t) = &nested {
        if let Some(v) = &t.token_type {
            metadata.insert("token_type".to_owned(), Value::String(v.clone()));
        }
        if let Some(v) = &t.token_uri {
            metadata.insert("token_uri".to_owned(), Value::String(v.clone()));
        }
        if let Some(v) = &t.client_id {
            metadata.insert("client_id".to_owned(), Value::String(v.clone()));
        }
        if let Some(v) = &t.client_secret {
            metadata.insert("client_secret".to_owned(), Value::String(v.clone()));
        }
        if let Some(v) = &t.scopes {
            metadata.insert(
                "scopes".to_owned(),
                Value::Array(v.iter().map(|s| Value::String(s.clone())).collect()),
            );
        }
    }
    if let Some(v) = map.get("project_id").cloned() {
        metadata.insert("project_id".to_owned(), v);
    }

    Ok(TokenRecord {
        id: id.to_owned(),
        provider: Provider::Gemini,
        access_token,
        refresh_token,
        id_token: None,
        email: extract_str(map, "email"),
        expired_at,
        last_refresh: None,
        disabled: false,
        metadata,
        file_path: Some(file_path.to_owned()),
    })
}

fn record_to_json(record: &TokenRecord) -> Value {
    match record.provider {
        Provider::Gemini => record_to_json_gemini(record),
        Provider::Codex => record_to_json_codex(record),
        Provider::Xai => record_to_json_xai(record),
        _ => record_to_json_flat(record),
    }
}

fn record_to_json_flat(record: &TokenRecord) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "type".to_owned(),
        Value::String(record.provider.as_str().to_owned()),
    );
    if !record.access_token.is_empty() {
        map.insert(
            "access_token".to_owned(),
            Value::String(record.access_token.clone()),
        );
    }
    if let Some(rt) = &record.refresh_token {
        map.insert("refresh_token".to_owned(), Value::String(rt.clone()));
    }
    if let Some(it) = &record.id_token {
        map.insert("id_token".to_owned(), Value::String(it.clone()));
    }
    if let Some(em) = &record.email {
        map.insert("email".to_owned(), Value::String(em.clone()));
    }
    if let Some(dt) = &record.expired_at {
        map.insert("expired".to_owned(), Value::String(dt.to_rfc3339()));
    }
    if let Some(dt) = &record.last_refresh {
        map.insert("last_refresh".to_owned(), Value::String(dt.to_rfc3339()));
    }
    if record.disabled {
        map.insert("disabled".to_owned(), Value::Bool(true));
    }
    Value::Object(map)
}

fn record_to_json_codex(record: &TokenRecord) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("type".to_owned(), Value::String("codex".to_owned()));
    if !record.access_token.is_empty() {
        map.insert(
            "access_token".to_owned(),
            Value::String(record.access_token.clone()),
        );
    }
    if let Some(rt) = &record.refresh_token {
        map.insert("refresh_token".to_owned(), Value::String(rt.clone()));
    }
    if let Some(it) = &record.id_token {
        map.insert("id_token".to_owned(), Value::String(it.clone()));
    }
    if let Some(em) = &record.email {
        map.insert("email".to_owned(), Value::String(em.clone()));
    }
    if let Some(dt) = &record.expired_at {
        map.insert("expired".to_owned(), Value::String(dt.to_rfc3339()));
    }
    if let Some(dt) = &record.last_refresh {
        map.insert("last_refresh".to_owned(), Value::String(dt.to_rfc3339()));
    }
    if let Some(v) = record.metadata.get("account_id") {
        map.insert("account_id".to_owned(), v.clone());
    }
    if let Some(v) = record.metadata.get("plan_type") {
        map.insert("plan_type".to_owned(), v.clone());
    }
    Value::Object(map)
}

fn record_to_json_xai(record: &TokenRecord) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("type".to_owned(), Value::String("xai".to_owned()));
    if !record.access_token.is_empty() {
        map.insert(
            "access_token".to_owned(),
            Value::String(record.access_token.clone()),
        );
    }
    if let Some(rt) = &record.refresh_token {
        map.insert("refresh_token".to_owned(), Value::String(rt.clone()));
    }
    if let Some(it) = &record.id_token {
        map.insert("id_token".to_owned(), Value::String(it.clone()));
    }
    if let Some(em) = &record.email {
        map.insert("email".to_owned(), Value::String(em.clone()));
    }
    if let Some(dt) = &record.expired_at {
        map.insert("expired".to_owned(), Value::String(dt.to_rfc3339()));
    }
    if let Some(dt) = &record.last_refresh {
        map.insert("last_refresh".to_owned(), Value::String(dt.to_rfc3339()));
    }
    for key in [
        "base_url",
        "token_endpoint",
        "auth_kind",
        "token_type",
        "expires_in",
    ] {
        if let Some(v) = record.metadata.get(key) {
            map.insert((*key).to_owned(), v.clone());
        }
    }
    Value::Object(map)
}

fn record_to_json_gemini(record: &TokenRecord) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("type".to_owned(), Value::String("gemini".to_owned()));

    let mut token_obj = serde_json::Map::new();
    token_obj.insert(
        "access_token".to_owned(),
        Value::String(record.access_token.clone()),
    );
    if let Some(rt) = &record.refresh_token {
        token_obj.insert("refresh_token".to_owned(), Value::String(rt.clone()));
    }
    for key in ["token_type", "token_uri", "client_id", "client_secret"] {
        if let Some(v) = record.metadata.get(key) {
            token_obj.insert((*key).to_owned(), v.clone());
        }
    }
    if let Some(dt) = &record.expired_at {
        token_obj.insert("expiry".to_owned(), Value::String(dt.to_rfc3339()));
    }
    if let Some(v) = record.metadata.get("scopes") {
        token_obj.insert("scopes".to_owned(), v.clone());
    }
    map.insert("token".to_owned(), Value::Object(token_obj));

    if let Some(v) = record.metadata.get("project_id") {
        map.insert("project_id".to_owned(), v.clone());
    }
    if let Some(em) = &record.email {
        map.insert("email".to_owned(), Value::String(em.clone()));
    }

    Value::Object(map)
}

async fn read_record(path: &Path, id: &str) -> Result<TokenRecord, StoreError> {
    let bytes = tokio::fs::read(path).await?;
    let raw: RawFile = serde_json::from_slice(&bytes)?;
    record_from_json(id, path, &raw)
}

#[async_trait]
impl TokenStore for FileTokenStore {
    async fn list(&self) -> Result<Vec<TokenRecord>, StoreError> {
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        let mut records = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let Some(ext) = path.extension() else {
                continue;
            };
            if ext != "json" {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let id = stem.to_owned();
            match read_record(&path, &id).await {
                Ok(r) => records.push(r),
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "skipping malformed token file")
                }
            }
        }

        Ok(records)
    }

    async fn save(&self, record: &TokenRecord) -> Result<String, StoreError> {
        let path = self.record_path(&record.id);
        let new_json = record_to_json(record);
        let new_pretty = serde_json::to_string_pretty(&new_json)?;

        if let Ok(existing_bytes) = tokio::fs::read(&path).await {
            if let Ok(existing_val) = serde_json::from_slice::<Value>(&existing_bytes) {
                if existing_val == new_json {
                    debug!(id = %record.id, "token unchanged, skipping write");
                    return Ok(record.id.clone());
                }
            }
        }

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&path, &new_pretty).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&path, perms).await?;
        }

        debug!(id = %record.id, path = %path.display(), "saved token");
        Ok(record.id.clone())
    }

    async fn delete(&self, id: &str) -> Result<(), StoreError> {
        let path = self.record_path(id);
        if !path.exists() {
            return Err(StoreError::NotFound(id.to_owned()));
        }
        tokio::fs::remove_file(&path).await?;
        debug!(id, "deleted token");
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<TokenRecord, StoreError> {
        let path = self.record_path(id);
        if !path.exists() {
            return Err(StoreError::NotFound(id.to_owned()));
        }
        read_record(&path, id).await
    }
}
