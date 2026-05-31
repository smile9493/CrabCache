//! OAuth / upstream credential **cold storage** (PostgreSQL authoritative).
//!
//! Policy: all accounts, credentials, and profile key pools are long-lived in PG.
//! `auth_dir` JSON files are a local runtime cache only; Redis is Gateway hot pipe.

use crate::state::AppState;
use crab_auth::store::{FileTokenStore, TokenStore};
use crab_auth::types::{Provider, TokenRecord};
use std::path::Path;
use tracing::{info, warn};

/// Persist one credential: **PG commit first**, then mirror to `auth_dir`.
pub async fn save_credential(
    state: &AppState,
    record: &TokenRecord,
) -> Result<String, crab_auth::store::StoreError> {
    if let Err(e) = persist_credential_to_pg(state, record).await {
        warn!(credential_id = %record.id, error = %e, "PG persist oauth credential failed (non-fatal)");
    }
    FileTokenStore::new(&state.auth_dir).save(record).await
}

pub async fn list_credentials(
    state: &AppState,
) -> Result<Vec<TokenRecord>, crab_auth::store::StoreError> {
    let pg = { state.pg_store.read().clone() };
    if let Some(pg) = pg {
        match pg.load_oauth_credentials().await {
            Ok(records) if !records.is_empty() => return Ok(records),
            Ok(_) => {}
            Err(e) => {
                warn!(error = %e, "PG load oauth credentials failed; falling back to auth_dir")
            }
        }
    }
    FileTokenStore::new(&state.auth_dir).list().await
}

pub async fn get_credential(
    state: &AppState,
    id: &str,
) -> Result<TokenRecord, crab_auth::store::StoreError> {
    let pg = { state.pg_store.read().clone() };
    if let Some(pg) = pg {
        if let Ok(Some(record)) = pg.load_oauth_credential(id).await {
            return Ok(record);
        }
    }
    FileTokenStore::new(&state.auth_dir).get(id).await
}

/// Restore credentials from PG into `auth_dir` (PG wins). Bootstrap PG from disk when empty.
pub async fn hydrate_credentials_from_pg(state: &AppState) -> bool {
    let pg = { state.pg_store.read().clone() };
    let Some(pg) = pg else {
        return false;
    };

    match pg.load_oauth_credentials().await {
        Ok(records) if !records.is_empty() => {
            let store = FileTokenStore::new(&state.auth_dir);
            let mut mirrored = 0usize;
            for rec in &records {
                if store.save(rec).await.is_ok() {
                    mirrored += 1;
                }
            }
            info!(
                count = records.len(),
                mirrored, "OAuth credentials restored from PostgreSQL (cold store)"
            );
            true
        }
        Ok(_) => {
            let imported = bootstrap_auth_dir_to_pg(state, &pg).await;
            if imported > 0 {
                info!(
                    imported,
                    "Bootstrapped OAuth credentials from auth_dir into PostgreSQL"
                );
            }
            imported > 0
        }
        Err(e) => {
            warn!(error = %e, "Failed to hydrate OAuth credentials from PostgreSQL");
            false
        }
    }
}

async fn persist_credential_to_pg(state: &AppState, record: &TokenRecord) -> anyhow::Result<()> {
    let pg = { state.pg_store.read().clone() };
    let Some(pg) = pg else {
        return Ok(());
    };
    let _guard = state.pg_write_lock.lock().await;
    pg.upsert_oauth_credential(record).await
}

async fn bootstrap_auth_dir_to_pg(state: &AppState, pg: &crate::pg::PgStore) -> usize {
    let store = FileTokenStore::new(&state.auth_dir);
    let Ok(records) = store.list().await else {
        return 0;
    };
    let codex: Vec<&TokenRecord> = records
        .iter()
        .filter(|r| r.provider == Provider::Codex)
        .collect();
    if codex.is_empty() {
        return 0;
    }
    let _guard = state.pg_write_lock.lock().await;
    let mut imported = 0usize;
    for rec in codex {
        if pg.upsert_oauth_credential(rec).await.is_ok() {
            imported += 1;
        }
    }
    imported
}

/// One-shot import of legacy auth JSON files into PG (startup / migration helper).
pub async fn import_auth_dir_into_pg(state: &AppState, auth_dir: &Path) -> usize {
    let pg = { state.pg_store.read().clone() };
    let Some(pg) = pg else {
        return 0;
    };
    let store = FileTokenStore::new(auth_dir);
    let Ok(records) = store.list().await else {
        return 0;
    };
    let _guard = state.pg_write_lock.lock().await;
    let mut imported = 0usize;
    for rec in records.iter().filter(|r| r.provider == Provider::Codex) {
        if pg.upsert_oauth_credential(rec).await.is_ok() {
            imported += 1;
        }
    }
    imported
}
