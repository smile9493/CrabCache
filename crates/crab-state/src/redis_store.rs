use crate::snapshot::ControlPlaneSnapshot;
use anyhow::{Context, Result};
use bb8::Pool;
use bb8_redis::RedisConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct RedisStateConfig {
    pub redis_url: String,
    pub key_prefix: String,
}

impl RedisStateConfig {
    pub fn new(redis_url: impl Into<String>, key_prefix: impl Into<String>) -> Self {
        Self {
            redis_url: redis_url.into(),
            key_prefix: key_prefix.into(),
        }
    }
}

pub struct RedisStateStore {
    pool: Pool<RedisConnectionManager>,
    redis_url: String,
    prefix: String,
    rev_channel: String,
}

impl RedisStateStore {
    pub async fn connect(config: &RedisStateConfig) -> Result<Self> {
        let pool = Pool::builder()
            .max_size(8)
            .connection_timeout(Duration::from_secs(5))
            .idle_timeout(Some(Duration::from_secs(60)))
            .max_lifetime(Some(Duration::from_secs(300)))
            .build(RedisConnectionManager::new(config.redis_url.clone())?)
            .await
            .context("Redis state store pool")?;

        let prefix = config.key_prefix.trim_end_matches(':').to_string();
        let rev_channel = format!("{prefix}:rev");

        Ok(Self {
            pool,
            redis_url: config.redis_url.clone(),
            prefix,
            rev_channel,
        })
    }

    fn key(&self, suffix: &str) -> String {
        format!("{}:{}", self.prefix, suffix)
    }

    pub async fn current_version(&self) -> Result<u64> {
        let mut conn = self.pool.get().await?;
        let v: Option<u64> = conn.get(self.key("version")).await?;
        Ok(v.unwrap_or(0))
    }

    pub async fn load_all(&self) -> Result<(u64, ControlPlaneSnapshot)> {
        let mut conn = self.pool.get().await?;
        let version: u64 = conn
            .get::<_, Option<u64>>(self.key("version"))
            .await?
            .unwrap_or(0);

        let keys_json: Option<String> = conn.get(self.key("keys")).await?;
        let runtime_json: Option<String> = conn.get(self.key("runtime")).await?;
        let upstream_json: Option<String> = conn.get(self.key("upstream_keys")).await?;
        let upstream_profiles_json: Option<String> =
            conn.get(self.key("upstream_profiles")).await?;
        let domain_json: Option<String> = conn.get(self.key("domain_policies")).await?;

        let keys = keys_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();

        let runtime = runtime_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        let upstream_keys = upstream_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;

        let upstream_profiles = upstream_profiles_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;

        let domain_policies = domain_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();

        Ok((
            version,
            ControlPlaneSnapshot {
                keys,
                runtime,
                upstream_keys,
                upstream_profiles,
                domain_policies,
            },
        ))
    }

    pub async fn save_all(&self, snap: &ControlPlaneSnapshot) -> Result<u64> {
        let mut conn = self.pool.get().await?;
        let _: () = conn
            .set(self.key("keys"), serde_json::to_string(&snap.keys)?)
            .await?;
        if let Some(rt) = &snap.runtime {
            let _: () = conn
                .set(self.key("runtime"), serde_json::to_string(rt)?)
                .await?;
        }
        if let Some(upstream) = &snap.upstream_keys {
            let _: () = conn
                .set(self.key("upstream_keys"), serde_json::to_string(upstream)?)
                .await?;
        }
        if let Some(profiles) = &snap.upstream_profiles {
            let _: () = conn
                .set(
                    self.key("upstream_profiles"),
                    serde_json::to_string(profiles)?,
                )
                .await?;
        }
        let _: () = conn
            .set(
                self.key("domain_policies"),
                serde_json::to_string(&snap.domain_policies)?,
            )
            .await?;

        let version: u64 = conn.incr(self.key("version"), 1).await?;
        let _: u64 = conn.publish(&self.rev_channel, version).await?;
        debug!(version, "Persisted control plane state to Redis");
        Ok(version)
    }

    pub async fn is_empty(&self) -> Result<bool> {
        let mut conn = self.pool.get().await?;
        let exists: bool = conn.exists(self.key("keys")).await?;
        Ok(!exists)
    }

    pub fn rev_channel(&self) -> &str {
        &self.rev_channel
    }

    pub fn redis_url(&self) -> &str {
        &self.redis_url
    }

    pub async fn ping(&self) -> bool {
        match self.pool.get().await {
            Ok(mut conn) => conn.get::<_, String>("PING").await.is_ok(),
            Err(_) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateBackendConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default = "default_prefix")]
    pub key_prefix: String,
    #[serde(default = "default_refresh")]
    pub refresh_interval_secs: u64,
}

fn default_backend() -> String {
    "memory".to_string()
}

fn default_prefix() -> String {
    "crab:state".to_string()
}

fn default_refresh() -> u64 {
    5
}

impl Default for StateBackendConfig {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            redis_url: None,
            key_prefix: default_prefix(),
            refresh_interval_secs: default_refresh(),
        }
    }
}

impl StateBackendConfig {
    pub fn effective_backend(&self) -> String {
        std::env::var("CRABCACHE_STATE_BACKEND").unwrap_or_else(|_| self.backend.clone())
    }

    pub fn is_redis(&self) -> bool {
        self.effective_backend() == "redis"
    }
}
