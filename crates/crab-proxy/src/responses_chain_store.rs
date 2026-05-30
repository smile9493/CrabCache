//! Tiered Responses API chain store: Moka L0 + Redis L1.
//!
//! Persists completed Responses `output[]` so that follow-up requests using
//! `previous_response_id` can expand their chain even after a gateway restart
//! or cross-instance load balancing.

use bb8_redis::RedisConnectionManager;
use crab_metrics::global_metrics;
use redis::AsyncCommands;
use serde_json::Value;
use std::sync::Arc;
use tracing::warn;

const KEY_PREFIX: &str = "crab:responses_chain:";

/// Tiered store: process-local Moka (L0) with optional Redis L1 persistence.
pub struct ResponsesChainStore {
    l0: moka::sync::Cache<String, Arc<Vec<Value>>>,
    redis: Option<bb8::Pool<RedisConnectionManager>>,
    /// Pingora body filters run on sync worker threads; use the startup handle for Redis persist.
    spawn_handle: tokio::runtime::Handle,
    ttl_secs: u64,
    max_value_bytes: usize,
    max_output_items: usize,
}

impl ResponsesChainStore {
    /// Build a store backed by Moka L0 only (no Redis).
    pub fn new_l0_only(
        max_capacity: u64,
        ttl_secs: u64,
        spawn_handle: tokio::runtime::Handle,
    ) -> Arc<Self> {
        Arc::new(Self {
            l0: moka::sync::Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(std::time::Duration::from_secs(ttl_secs))
                .build(),
            redis: None,
            spawn_handle,
            ttl_secs,
            max_value_bytes: default_max_value_bytes(),
            max_output_items: default_max_output_items(),
        })
    }

    /// Build a tiered store (Moka L0 + Redis L1).
    pub async fn new_tiered(
        max_capacity: u64,
        ttl_secs: u64,
        redis_url: &str,
        max_value_bytes: usize,
        max_output_items: usize,
        spawn_handle: tokio::runtime::Handle,
    ) -> anyhow::Result<Arc<Self>> {
        let manager = RedisConnectionManager::new(redis_url)?;
        let pool = bb8::Pool::builder()
            .max_size(8)
            .connection_timeout(std::time::Duration::from_secs(5))
            .build(manager)
            .await?;
        Ok(Arc::new(Self {
            l0: moka::sync::Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(std::time::Duration::from_secs(ttl_secs))
                .build(),
            redis: Some(pool),
            spawn_handle,
            ttl_secs,
            max_value_bytes,
            max_output_items,
        }))
    }

    fn redis_key(response_id: &str) -> String {
        format!("{KEY_PREFIX}{response_id}")
    }

    /// Read: L0 first (sync), then L1 Redis (async) with L0 backfill.
    pub async fn get(&self, response_id: &str) -> Option<Arc<Vec<Value>>> {
        if let Some(entry) = self.l0.get(response_id) {
            global_metrics().record_responses_chain_l0_hit();
            return Some(entry);
        }
        global_metrics().record_responses_chain_l0_miss();

        let Some(pool) = &self.redis else {
            return None;
        };
        let key = Self::redis_key(response_id);
        let result: Result<Option<String>, _> = {
            let Ok(mut conn) = pool.get().await else {
                warn!("responses_chain: Redis pool get failed");
                return None;
            };
            conn.get(&key).await
        };
        match result {
            Ok(Some(raw)) => match serde_json::from_str::<Vec<Value>>(&raw) {
                Ok(output) => {
                    let arc = Arc::new(output);
                    self.l0.insert(response_id.to_string(), arc.clone());
                    global_metrics().record_responses_chain_redis_hit();
                    Some(arc)
                }
                Err(e) => {
                    warn!(error = %e, "responses_chain: Redis value deserialization failed");
                    global_metrics().record_responses_chain_redis_miss();
                    None
                }
            },
            Ok(None) => {
                global_metrics().record_responses_chain_redis_miss();
                None
            }
            Err(e) => {
                warn!(error = %e, "responses_chain: Redis GET failed");
                global_metrics().record_responses_chain_redis_miss();
                None
            }
        }
    }

    /// Write: sync L0 insert + async Redis SETEX in a spawned task.
    pub fn put(&self, response_id: &str, output: Vec<Value>) {
        if response_id.is_empty() || output.is_empty() {
            return;
        }
        let output = self.cap_output(output);
        let arc = Arc::new(output);
        self.l0.insert(response_id.to_string(), arc.clone());

        if let Some(pool) = self.redis.clone() {
            let key = Self::redis_key(response_id);
            let ttl = self.ttl_secs;
            let max_bytes = self.max_value_bytes;
            self.spawn_handle.spawn(async move {
                match serde_json::to_string(&*arc) {
                    Ok(json) if json.len() > max_bytes => {
                        warn!(
                            response_id = %key,
                            bytes = json.len(),
                            max = max_bytes,
                            "responses_chain: value too large, skipping Redis persist"
                        );
                        global_metrics().record_responses_chain_persist_skip();
                    }
                    Ok(json) => {
                        let result: Result<(), _> = {
                            let Ok(mut conn) = pool.get().await else {
                                warn!("responses_chain: Redis pool get failed on persist");
                                global_metrics().record_responses_chain_persist_error();
                                return;
                            };
                            conn.set_ex(&key, &json, ttl).await
                        };
                        if let Err(e) = result {
                            warn!(error = %e, key = %key, "responses_chain: Redis SETEX failed");
                            global_metrics().record_responses_chain_persist_error();
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "responses_chain: serialization failed");
                        global_metrics().record_responses_chain_persist_error();
                    }
                }
            });
        }
    }

    fn cap_output(&self, mut output: Vec<Value>) -> Vec<Value> {
        if self.max_output_items > 0 && output.len() > self.max_output_items {
            warn!(
                original = output.len(),
                cap = self.max_output_items,
                "responses_chain: truncating output items"
            );
            output.truncate(self.max_output_items);
        }
        output
    }
}

fn default_max_value_bytes() -> usize {
    256 * 1024
}

fn default_max_output_items() -> usize {
    64
}
