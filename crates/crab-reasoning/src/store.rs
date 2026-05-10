use crate::keys::{portable_reasoning_keys, scoped_reasoning_keys};
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

pub struct ReasoningStore {
    conn: Mutex<Connection>,
    max_age_seconds: Option<u64>,
    max_rows: Option<usize>,
}

impl ReasoningStore {
    pub fn new(path: &str, max_age_seconds: Option<u64>, max_rows: Option<usize>) -> anyhow::Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            let path = Path::new(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            Connection::open(path)?
        };

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS reasoning_cache (
                key TEXT PRIMARY KEY,
                reasoning TEXT NOT NULL,
                message_json TEXT NOT NULL,
                created_at REAL NOT NULL
            )",
        )?;

        let store = Self {
            conn: Mutex::new(conn),
            max_age_seconds,
            max_rows,
        };
        store.prune()?;
        Ok(store)
    }

    pub fn put(&self, key: &str, reasoning: &str, message: &Value) {
        let message_json = serde_json::to_string(message).unwrap_or_default();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(_) => return,
        };

        let _ = conn.execute(
            "INSERT INTO reasoning_cache(key, reasoning, message_json, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET
                 reasoning = excluded.reasoning,
                 message_json = excluded.message_json,
                 created_at = excluded.created_at",
            rusqlite::params![key, reasoning, message_json, now],
        );

        let _ = self.prune_locked(&conn);
        let _ = conn.execute_batch("COMMIT");
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn.prepare("SELECT reasoning FROM reasoning_cache WHERE key = ?1").ok()?;
        let mut rows = stmt.query(rusqlite::params![key]).ok()?;
        let row = rows.next().ok()??;
        row.get(0).ok()
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
            keys.extend(portable_reasoning_keys(message, cache_namespace, prior_messages));
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
            keys.extend(portable_reasoning_keys(message, cache_namespace, prior_messages));
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM reasoning_cache", [], |row| row.get(0))
            .unwrap_or(0);
        conn.execute("DELETE FROM reasoning_cache", [])?;
        Ok(count)
    }

    pub fn prune(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        self.prune_locked(&conn)?;
        Ok(())
    }

    fn prune_locked(&self, conn: &Connection) -> anyhow::Result<()> {
        let mut deleted = 0;

        if let Some(max_age) = self.max_age_seconds {
            if max_age > 0 {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();
                let cutoff = now - max_age as f64;
                let result = conn.execute(
                    "DELETE FROM reasoning_cache WHERE created_at < ?1",
                    rusqlite::params![cutoff],
                )?;
                deleted += result;
            }
        }

        if let Some(max_rows) = self.max_rows {
            if max_rows > 0 {
                let result = conn.execute(
                    "DELETE FROM reasoning_cache WHERE key NOT IN (
                        SELECT key FROM reasoning_cache ORDER BY created_at DESC LIMIT ?1
                    )",
                    rusqlite::params![max_rows as i64],
                )?;
                deleted += result;
            }
        }

        if deleted > 0 {
            debug!(deleted = deleted, "Pruned reasoning cache");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_store_and_retrieve() {
        let store = ReasoningStore::new(":memory:", None, None).unwrap();
        let msg = json!({"role": "assistant", "content": "test", "reasoning_content": "thinking..."});
        store.put("key1", "thinking...", &msg);

        let result = store.get("key1");
        assert_eq!(result, Some("thinking...".to_string()));
    }

    #[test]
    fn test_store_assistant_message() {
        let store = ReasoningStore::new(":memory:", None, None).unwrap();
        let msg = json!({
            "role": "assistant",
            "content": "result",
            "reasoning_content": "I thought about it",
            "tool_calls": [{"id": "tc1", "type": "function", "function": {"name": "foo", "arguments": "{}"}}]
        });
        let prior = vec![json!({"role": "user", "content": "hello"})];
        let count = store.store_assistant_message(&msg, "scope1", "ns1", &prior);
        assert!(count > 0);

        let lookup_msg = json!({
            "content": "result",
            "tool_calls": [{"id": "tc1", "type": "function", "function": {"name": "foo", "arguments": "{}"}}]
        });
        let result = store.lookup_for_message(&lookup_msg, "scope1", "ns1", &prior);
        assert_eq!(result, Some("I thought about it".to_string()));
    }

    #[test]
    fn test_non_assistant_message_ignored() {
        let store = ReasoningStore::new(":memory:", None, None).unwrap();
        let msg = json!({"role": "user", "content": "hello", "reasoning_content": "should not store"});
        let count = store.store_assistant_message(&msg, "scope1", "", &[]);
        assert_eq!(count, 0);
    }
}
