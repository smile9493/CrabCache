use crab_auth::store::{FileTokenStore, TokenStore};
use crab_auth::types::{Provider, TokenRecord};
use std::collections::HashMap;
use tempfile::TempDir;

fn claude_record(id: &str) -> TokenRecord {
    TokenRecord {
        id: id.to_owned(),
        provider: Provider::Claude,
        access_token: "sk-ant-api123".to_owned(),
        refresh_token: Some("refresh-abc".to_owned()),
        id_token: None,
        email: Some("user@example.com".to_owned()),
        expired_at: Some(
            chrono::DateTime::parse_from_rfc3339("2025-06-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        ),
        last_refresh: Some(
            chrono::DateTime::parse_from_rfc3339("2025-05-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        ),
        disabled: false,
        metadata: HashMap::new(),
        file_path: None,
    }
}

fn gemini_record(id: &str) -> TokenRecord {
    let mut metadata = HashMap::new();
    metadata.insert(
        "token_type".to_owned(),
        serde_json::Value::String("Bearer".to_owned()),
    );
    metadata.insert(
        "token_uri".to_owned(),
        serde_json::Value::String("https://oauth2.googleapis.com/token".to_owned()),
    );
    metadata.insert(
        "client_id".to_owned(),
        serde_json::Value::String("681255809395-abc".to_owned()),
    );
    metadata.insert(
        "client_secret".to_owned(),
        serde_json::Value::String("GOCSPX-xyz".to_owned()),
    );
    metadata.insert(
        "scopes".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String(
            "https://www.googleapis.com/auth/cloud-platform".to_owned(),
        )]),
    );
    metadata.insert(
        "project_id".to_owned(),
        serde_json::Value::String("my-gcp-project".to_owned()),
    );

    TokenRecord {
        id: id.to_owned(),
        provider: Provider::Gemini,
        access_token: "gemini-access-token".to_owned(),
        refresh_token: Some("gemini-refresh".to_owned()),
        id_token: None,
        email: Some("user@gmail.com".to_owned()),
        expired_at: Some(
            chrono::DateTime::parse_from_rfc3339("2025-06-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        ),
        last_refresh: None,
        disabled: false,
        metadata,
        file_path: None,
    }
}

#[tokio::test]
async fn test_save_and_load_claude_token() {
    let dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(dir.path());
    let record = claude_record("alice");

    let id = store.save(&record).await.unwrap();
    assert_eq!(id, "alice");

    let loaded = store.get("alice").await.unwrap();
    assert_eq!(loaded.provider, Provider::Claude);
    assert_eq!(loaded.access_token, "sk-ant-api123");
    assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-abc"));
    assert_eq!(loaded.email.as_deref(), Some("user@example.com"));
    assert!(loaded.expired_at.is_some());
    assert!(loaded.last_refresh.is_some());
    assert!(!loaded.disabled);
}

#[tokio::test]
async fn test_save_and_load_gemini_token() {
    let dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(dir.path());
    let record = gemini_record("gemini1");

    store.save(&record).await.unwrap();

    let loaded = store.get("gemini1").await.unwrap();
    assert_eq!(loaded.provider, Provider::Gemini);
    assert_eq!(loaded.access_token, "gemini-access-token");
    assert_eq!(loaded.email.as_deref(), Some("user@gmail.com"));
    assert_eq!(
        loaded.metadata.get("project_id").and_then(|v| v.as_str()),
        Some("my-gcp-project")
    );
    assert_eq!(
        loaded.metadata.get("client_id").and_then(|v| v.as_str()),
        Some("681255809395-abc")
    );
}

#[tokio::test]
async fn test_list_multiple_tokens() {
    let dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(dir.path());

    store.save(&claude_record("a")).await.unwrap();
    store.save(&claude_record("b")).await.unwrap();
    store.save(&gemini_record("c")).await.unwrap();

    let all = store.list().await.unwrap();
    assert_eq!(all.len(), 3);
    let ids: Vec<&str> = all.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"b"));
    assert!(ids.contains(&"c"));
}

#[tokio::test]
async fn test_delete_token() {
    let dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(dir.path());

    store.save(&claude_record("delme")).await.unwrap();
    assert!(store.get("delme").await.is_ok());

    store.delete("delme").await.unwrap();
    assert!(store.get("delme").await.is_err());
}

#[tokio::test]
async fn test_get_nonexistent_returns_not_found() {
    let dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(dir.path());

    let err = store.get("nope").await.unwrap_err();
    assert!(matches!(err, crab_auth::store::StoreError::NotFound(_)));
}

#[tokio::test]
async fn test_migration_compat_claude_format() {
    let dir = TempDir::new().unwrap();
    let json = r#"{
        "id_token": "",
        "access_token": "sk-ant-apikey123",
        "refresh_token": "refresh-token-xyz",
        "last_refresh": "2025-01-15T10:30:00Z",
        "email": "alice@example.com",
        "type": "claude",
        "expired": "2025-01-15T11:30:00Z",
        "disabled": false
    }"#;

    let file_path = dir.path().join("claude_alice.json");
    std::fs::write(&file_path, json).unwrap();

    let store = FileTokenStore::new(dir.path());
    let records = store.list().await.unwrap();
    assert_eq!(records.len(), 1);

    let r = &records[0];
    assert_eq!(r.id, "claude_alice");
    assert_eq!(r.provider, Provider::Claude);
    assert_eq!(r.access_token, "sk-ant-apikey123");
    assert_eq!(r.refresh_token.as_deref(), Some("refresh-token-xyz"));
    assert_eq!(r.email.as_deref(), Some("alice@example.com"));
    assert!(r.expired_at.is_some());
    assert!(r.last_refresh.is_some());
    assert!(!r.disabled);
}

#[tokio::test]
async fn test_migration_compat_gemini_format() {
    let dir = TempDir::new().unwrap();
    let json = r#"{
        "token": {
            "access_token": "gem-access-123",
            "refresh_token": "gem-refresh-456",
            "token_type": "Bearer",
            "expiry": "2025-03-01T00:00:00Z",
            "token_uri": "https://oauth2.googleapis.com/token",
            "client_id": "681255809395-xyz.apps.googleusercontent.com",
            "client_secret": "GOCSPX-secret123",
            "scopes": ["https://www.googleapis.com/auth/cloud-platform"]
        },
        "project_id": "my-project-123",
        "email": "dev@gmail.com",
        "type": "gemini"
    }"#;

    let file_path = dir.path().join("gemini_dev.json");
    std::fs::write(&file_path, json).unwrap();

    let store = FileTokenStore::new(dir.path());
    let records = store.list().await.unwrap();
    assert_eq!(records.len(), 1);

    let r = &records[0];
    assert_eq!(r.id, "gemini_dev");
    assert_eq!(r.provider, Provider::Gemini);
    assert_eq!(r.access_token, "gem-access-123");
    assert_eq!(r.refresh_token.as_deref(), Some("gem-refresh-456"));
    assert_eq!(r.email.as_deref(), Some("dev@gmail.com"));
    assert_eq!(
        r.metadata.get("project_id").and_then(|v| v.as_str()),
        Some("my-project-123")
    );
    assert_eq!(
        r.metadata.get("client_id").and_then(|v| v.as_str()),
        Some("681255809395-xyz.apps.googleusercontent.com")
    );
    assert!(r.expired_at.is_some());
}
