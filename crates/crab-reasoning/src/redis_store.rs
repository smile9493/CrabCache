use crab_metrics::global_metrics;
use redis::Commands;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

#[derive(Serialize, Deserialize, Clone)]
struct ReasoningEntry {
    reasoning: String,
    message_json: String,
    created_at: f64,
}

pub struct RedisReasoningStore {
    client: Arc<redis::Client>,
    prefix: String,
    max_age_seconds: Option<u64>,
    max_rows: Option<usize>,
    max_entry_bytes: usize,
}

impl RedisReasoningStore {
    pub fn new(
        redis_url: &str,
        max_age_seconds: Option<u64>,
        max_rows: Option<usize>,
        max_entry_bytes: usize,
    ) -> anyhow::Result<Self> {
        let client = Arc::new(redis::Client::open(redis_url)?);
        Ok(Self {
            client,
            prefix: "crab:reasoning".to_string(),
            max_age_seconds,
            max_rows,
            max_entry_bytes,
        })
    }

    pub fn client(&self) -> Arc<redis::Client> {
        Arc::clone(&self.client)
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn max_rows(&self) -> Option<usize> {
        self.max_rows
    }

    fn redis_key(&self, key: &str) -> String {
        format!("{}:{}", self.prefix, key)
    }

    fn blocking_put(
        client: Arc<redis::Client>,
        redis_key: String,
        payload: String,
        max_age_seconds: Option<u64>,
    ) {
        let Ok(mut conn) = client.get_connection() else {
            return;
        };
        let _: Result<(), _> = conn.set(&redis_key, &payload);
        if let Some(ttl) = max_age_seconds.filter(|&t| t > 0) {
            let _: Result<(), _> = conn.expire(&redis_key, ttl as i64);
        }
    }

    fn blocking_get(client: Arc<redis::Client>, redis_key: String) -> Option<String> {
        let mut conn = client.get_connection().ok()?;
        let payload: Option<String> = conn.get(&redis_key).ok()?;
        payload
            .as_deref()
            .and_then(|s| serde_json::from_str::<ReasoningEntry>(s).ok())
            .map(|e| e.reasoning)
    }

    pub fn put(&self, key: &str, reasoning: &str, message: &Value) {
        let message_json = serde_json::to_string(message).unwrap_or_default();
        let entry = ReasoningEntry {
            reasoning: reasoning.to_string(),
            message_json,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        };
        let payload = match serde_json::to_string(&entry) {
            Ok(p) => p,
            Err(_) => return,
        };
        if payload.len() > self.max_entry_bytes {
            warn!(
                key = key,
                bytes = payload.len(),
                max = self.max_entry_bytes,
                "Reasoning entry exceeds max_reasoning_entry_bytes, skipping"
            );
            global_metrics().record_reasoning_store_rejected();
            return;
        }
        let client = Arc::clone(&self.client);
        let redis_key = self.redis_key(key);
        let ttl = self.max_age_seconds;
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| {
                Self::blocking_put(client, redis_key, payload, ttl);
            });
        } else {
            Self::blocking_put(client, redis_key, payload, ttl);
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let client = Arc::clone(&self.client);
        let redis_key = self.redis_key(key);
        let value = if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| Self::blocking_get(client, redis_key))
        } else {
            Self::blocking_get(client, redis_key)
        };
        global_metrics().record_reasoning_store_lookup(value.is_some());
        value
    }

    pub fn clear(&self) -> anyhow::Result<usize> {
        let client = Arc::clone(&self.client);
        let prefix = self.prefix.clone();
        let run = move || -> anyhow::Result<usize> {
            let mut conn = client.get_connection()?;
            let pattern = format!("{prefix}:*");
            let mut cursor: u64 = 0;
            let mut total = 0usize;
            loop {
                let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .arg("COUNT")
                    .arg(500)
                    .query(&mut conn)?;
                if !keys.is_empty() {
                    let n: usize = redis::cmd("DEL").arg(&keys).query(&mut conn)?;
                    total += n;
                }
                if next == 0 {
                    break;
                }
                cursor = next;
            }
            Ok(total)
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(run)
        } else {
            run()
        }
    }

    /// Background prune when `max_rows` is set: SCAN entries and delete oldest by `created_at`.
    pub fn spawn_prune_task(self: Arc<Self>) {
        let Some(max_rows) = self.max_rows.filter(|n| *n > 0) else {
            return;
        };
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("reasoning prune runtime");
            rt.block_on(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(300));
                loop {
                    interval.tick().await;
                    if let Err(e) = self.prune_to_max_rows(max_rows) {
                        warn!(error = %e, "Reasoning Redis max_rows prune failed");
                    }
                }
            });
        });
    }

    fn prune_to_max_rows(&self, max_rows: usize) -> anyhow::Result<()> {
        let client = Arc::clone(&self.client);
        let prefix = self.prefix.clone();
        let mut conn = client.get_connection()?;
        let pattern = format!("{prefix}:*");
        let mut cursor: u64 = 0;
        let mut entries: Vec<(String, f64)> = Vec::new();
        loop {
            let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(500)
                .query(&mut conn)?;
            for k in keys {
                let Ok(Some(s)) = conn.get::<_, Option<String>>(&k) else {
                    continue;
                };
                if let Ok(entry) = serde_json::from_str::<ReasoningEntry>(&s) {
                    entries.push((k, entry.created_at));
                }
            }
            if next == 0 {
                break;
            }
            cursor = next;
        }
        if entries.len() <= max_rows {
            return Ok(());
        }
        entries.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let to_delete = entries.len() - max_rows;
        for (k, _) in entries.into_iter().take(to_delete) {
            let _: () = conn.del(k)?;
        }
        Ok(())
    }
}
