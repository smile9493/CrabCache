use crate::keys::{portable_reasoning_keys, scoped_reasoning_keys};
use crate::pg_store::PgReasoningStore;
use crate::redis_store::RedisReasoningStore;
use crate::store::ReasoningStore;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

/// Shared reasoning cache (SQLite single-instance, Redis, or PostgreSQL).
pub enum ReasoningBackend {
    Sqlite(ReasoningStore),
    Redis(Arc<RedisReasoningStore>),
    Pg(Arc<PgReasoningStore>),
}

impl ReasoningBackend {
    pub fn open_sqlite(
        path: &str,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
    ) -> anyhow::Result<Self> {
        Ok(Self::Sqlite(ReasoningStore::new(
            path,
            max_age_seconds,
            max_rows,
        )?))
    }

    pub fn open_redis(
        redis_url: &str,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
        max_entry_bytes: usize,
    ) -> anyhow::Result<Self> {
        let store = Arc::new(RedisReasoningStore::new(
            redis_url,
            max_age_seconds,
            max_rows,
            max_entry_bytes,
        )?);
        store.clone().spawn_prune_task();
        Ok(Self::Redis(store))
    }

    /// Create a PG-backed reasoning store. Must be called from an async context.
    pub async fn open_pg(
        pg_url: &str,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
    ) -> anyhow::Result<Self> {
        let store = PgReasoningStore::connect(pg_url, max_age_seconds, max_rows).await?;
        Ok(Self::Pg(Arc::new(store)))
    }

    pub fn from_config(
        backend: &str,
        cache_db_path: &str,
        redis_url: Option<&str>,
        l1_redis_url: &str,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
        max_entry_bytes: usize,
    ) -> anyhow::Result<Self> {
        let effective = std::env::var("CRABCACHE_REASONING_BACKEND")
            .ok()
            .unwrap_or_else(|| backend.to_string());
        if effective == "redis" {
            let url = redis_url.filter(|s| !s.is_empty()).unwrap_or(l1_redis_url);
            Self::open_redis(url, max_age_seconds, max_rows, max_entry_bytes)
        } else if effective == "pg" {
            // PG backend requires async init — caller must use open_pg() directly.
            Err(anyhow::anyhow!(
                "PG reasoning backend requires async init; use ReasoningBackend::open_pg() instead"
            ))
        } else {
            Self::open_sqlite(cache_db_path, max_age_seconds, max_rows)
        }
    }

    pub fn put(&self, key: &str, reasoning: &str, message: &Value) {
        match self {
            Self::Sqlite(s) => s.put(key, reasoning, message),
            Self::Redis(s) => s.put(key, reasoning, message),
            Self::Pg(s) => {
                let s = Arc::clone(s);
                let key = key.to_string();
                let reasoning = reasoning.to_string();
                let message = message.clone();
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(s.put(&key, &reasoning, &message))
                });
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match self {
            Self::Sqlite(s) => s.get(key),
            Self::Redis(s) => s.get(key),
            Self::Pg(s) => {
                let s = Arc::clone(s);
                let key = key.to_string();
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(s.get(&key))
                })
            }
        }
    }

    pub fn store_assistant_message(
        &self,
        message: &Value,
        scope: &str,
        cache_namespace: &str,
        prior_messages: &[Value],
    ) -> usize {
        if message.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            return 0;
        }
        let reasoning = match message.get("reasoning_content").and_then(|r| r.as_str()) {
            Some(r) => r,
            None => return 0,
        };

        let mut keys = scoped_reasoning_keys(message, scope);
        if !prior_messages.is_empty() {
            keys.extend(portable_reasoning_keys(
                message,
                cache_namespace,
                prior_messages,
            ));
        }
        keys.dedup();

        for key in &keys {
            self.put(key, reasoning, message);
        }

        debug!(key_count = keys.len(), "Stored reasoning_content keys");
        keys.len()
    }

    pub fn lookup_for_message(
        &self,
        message: &Value,
        scope: &str,
        cache_namespace: &str,
        prior_messages: &[Value],
    ) -> Option<String> {
        let mut keys = scoped_reasoning_keys(message, scope);
        if !prior_messages.is_empty() {
            keys.extend(portable_reasoning_keys(
                message,
                cache_namespace,
                prior_messages,
            ));
        }
        for key in &keys {
            if let Some(reasoning) = self.get(key) {
                debug!(key = key, "Reasoning content cache hit");
                return Some(reasoning);
            }
        }
        None
    }

    pub fn backfill_portable_aliases(
        &self,
        message: &Value,
        reasoning: &str,
        cache_namespace: &str,
        prior_messages: &[Value],
    ) -> usize {
        let keys = portable_reasoning_keys(message, cache_namespace, prior_messages);
        if keys.is_empty() {
            return 0;
        }
        let mut message_with_reasoning = message.clone();
        if let Some(obj) = message_with_reasoning.as_object_mut() {
            obj.insert(
                "reasoning_content".into(),
                Value::String(reasoning.to_string()),
            );
        }
        let mut unique_keys: Vec<String> = keys;
        unique_keys.dedup();
        for key in &unique_keys {
            self.put(key, reasoning, &message_with_reasoning);
        }
        unique_keys.len()
    }

    pub fn clear(&self) -> anyhow::Result<usize> {
        match self {
            Self::Sqlite(s) => s.clear(),
            Self::Redis(s) => s.clear(),
            Self::Pg(s) => {
                let s = Arc::clone(s);
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(s.clear())
                })
            }
        }
    }
}
