use crab_metrics::global_metrics;
use redis::Commands;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize)]
struct ReasoningEntry {
    reasoning: String,
    message_json: String,
    created_at: f64,
}

pub struct RedisReasoningStore {
    client: redis::Client,
    prefix: String,
    max_age_seconds: Option<u64>,
    max_entry_bytes: usize,
}

impl RedisReasoningStore {
    pub fn new(
        redis_url: &str,
        max_age_seconds: Option<u64>,
        max_entry_bytes: usize,
    ) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self {
            client,
            prefix: "crab:reasoning".to_string(),
            max_age_seconds,
            max_entry_bytes,
        })
    }

    fn redis_key(&self, key: &str) -> String {
        format!("{}:{}", self.prefix, key)
    }

    fn conn(&self) -> anyhow::Result<redis::Connection> {
        Ok(self.client.get_connection()?)
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
            tracing::warn!(
                key = key,
                bytes = payload.len(),
                max = self.max_entry_bytes,
                "Reasoning entry exceeds max_reasoning_entry_bytes, skipping"
            );
            return;
        }
        let mut conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return,
        };
        let redis_key = self.redis_key(key);
        let _: Result<(), _> = conn.set(&redis_key, &payload);
        if let Some(ttl) = self.max_age_seconds.filter(|&t| t > 0) {
            let _: Result<(), _> = conn.expire(&redis_key, ttl as i64);
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let mut conn = self.conn().ok()?;
        let payload: Option<String> = conn.get(self.redis_key(key)).ok()?;
        let value = payload
            .as_deref()
            .and_then(|s| serde_json::from_str::<ReasoningEntry>(s).ok())
            .map(|e| e.reasoning);
        global_metrics().record_reasoning_store_lookup(value.is_some());
        value
    }

    pub fn clear(&self) -> anyhow::Result<usize> {
        let mut conn = self.conn()?;
        let pattern = format!("{}:*", self.prefix);
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query(&mut conn)
            .unwrap_or_default();
        let count = keys.len();
        if count > 0 {
            let _: () = redis::cmd("DEL").arg(&keys).query(&mut conn)?;
        }
        Ok(count)
    }
}
