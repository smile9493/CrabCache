use anyhow::Result;
use crab_cache::TtlConfig;
use crab_control::parse_backend_endpoints;
use crab_proxy::{
    ConnectionConfig, DomainPolicy, RuntimeConfig, StoredKey, UpstreamKeyPool, UpstreamKeySpec,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredKeySnapshot {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub enabled: bool,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintSnapshot {
    pub version: u32,
    pub normalize_content: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendSnapshot {
    pub name: String,
    pub addr: String,
    pub weight: u32,
    pub tls_sni: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub ttl: TtlConfig,
    pub fingerprint: FingerprintSnapshot,
    pub stream_cache_enabled: bool,
    pub upstream_base_url: String,
    pub fallback_model: String,
    pub backends: Vec<BackendSnapshot>,
    pub connection: ConnectionConfig,
    #[serde(default = "default_pipeline_mode_snapshot")]
    pub pipeline_mode: String,
    #[serde(default = "default_upstream_profile_snapshot")]
    pub default_upstream_profile: String,
}

fn default_pipeline_mode_snapshot() -> String {
    "auto".to_string()
}

fn default_upstream_profile_snapshot() -> String {
    "deepseek".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamKeySnapshot {
    pub id: String,
    pub secret: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ControlPlaneSnapshot {
    pub keys: HashMap<String, StoredKeySnapshot>,
    pub runtime: Option<RuntimeSnapshot>,
    /// `None` when the Redis key was never written (legacy); `Some` applies even if empty.
    #[serde(default)]
    pub upstream_keys: Option<Vec<UpstreamKeySnapshot>>,
    #[serde(default)]
    pub domain_policies: HashMap<String, DomainPolicy>,
}

pub fn build_snapshot_from_runtime(runtime: &RuntimeConfig) -> ControlPlaneSnapshot {
    let keys: HashMap<String, StoredKeySnapshot> = runtime
        .keys
        .iter()
        .map(|entry| {
            let k = entry.value();
            (
                entry.key().clone(),
                StoredKeySnapshot {
                    id: k.id.clone(),
                    name: k.name.clone(),
                    key_hash: k.key_hash.clone(),
                    enabled: k.enabled,
                    domain: k.domain.clone(),
                    pipeline: k.pipeline.clone(),
                    upstream_profile: k.upstream_profile.clone(),
                },
            )
        })
        .collect();

    let ttl = runtime
        .ttl
        .read()
        .map(|t| t.clone())
        .unwrap_or_else(|_| TtlConfig::new(3600));

    let fingerprint = runtime
        .fingerprint
        .read()
        .map(|f| FingerprintSnapshot {
            version: f.version,
            normalize_content: f.normalize_content,
        })
        .unwrap_or(FingerprintSnapshot {
            version: 1,
            normalize_content: true,
        });

    let upstream_base_url = runtime
        .upstream_base_url
        .read()
        .map(|u| u.clone())
        .unwrap_or_else(|_| "https://api.deepseek.com".to_string());

    let fallback_model = runtime
        .fallback_model
        .read()
        .map(|m| m.clone())
        .unwrap_or_else(|_| "deepseek-v4-pro".to_string());

    let backends: Vec<BackendSnapshot> = runtime
        .router
        .read()
        .map(|router| {
            router
                .backends()
                .iter()
                .map(|b| BackendSnapshot {
                    name: b.name.clone(),
                    addr: b.addr.to_string(),
                    weight: b.weight,
                    tls_sni: b.tls_sni.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let connection = runtime
        .conn_config
        .read()
        .map(|c| c.clone())
        .unwrap_or_default();

    let upstream_keys: Vec<UpstreamKeySnapshot> = runtime
        .upstream_pool()
        .to_specs()
        .into_iter()
        .map(|s| UpstreamKeySnapshot {
            id: s.id,
            secret: s.secret,
            enabled: s.enabled,
        })
        .collect();

    let domain_policies: HashMap<String, DomainPolicy> = runtime
        .list_domain_policies()
        .into_iter()
        .collect();

    let pipeline_globals = runtime.pipeline_globals();
    ControlPlaneSnapshot {
        keys,
        runtime: Some(RuntimeSnapshot {
            ttl,
            fingerprint,
            stream_cache_enabled: runtime.stream_cache_enabled(),
            upstream_base_url,
            fallback_model,
            backends,
            connection,
            pipeline_mode: pipeline_globals.pipeline_mode.as_str().to_string(),
            default_upstream_profile: runtime.default_upstream_profile_id(),
        }),
        upstream_keys: Some(upstream_keys),
        domain_policies,
    }
}

pub fn apply_snapshot_to_runtime(
    runtime: &RuntimeConfig,
    snap: &ControlPlaneSnapshot,
    upstream_cooldown_secs: u64,
) -> Result<()> {
    runtime.keys.clear();
    for (token, k) in &snap.keys {
        runtime.keys.insert(
            token.clone(),
            StoredKey {
                id: k.id.clone(),
                name: k.name.clone(),
                key_hash: k.key_hash.clone(),
                enabled: k.enabled,
                domain: k.domain.clone(),
                pipeline: k.pipeline.clone(),
                upstream_profile: k.upstream_profile.clone(),
            },
        );
    }

    if let Some(rt) = &snap.runtime {
        if let Ok(mut ttl) = runtime.ttl.write() {
            *ttl = rt.ttl.clone();
        }
        if let Ok(mut fp) = runtime.fingerprint.write() {
            fp.version = rt.fingerprint.version;
            fp.normalize_content = rt.fingerprint.normalize_content;
        }
        runtime.set_stream_cache_enabled(rt.stream_cache_enabled);
        let _ = runtime.set_pipeline_runtime(
            crab_pipeline::PipelineMode::from_str(&rt.pipeline_mode),
            &rt.default_upstream_profile,
        );
        if let Ok(mut base) = runtime.upstream_base_url.write() {
            *base = rt.upstream_base_url.clone();
        }
        if let Ok(mut model) = runtime.fallback_model.write() {
            *model = rt.fallback_model.clone();
        }
        if let Ok(mut conn) = runtime.conn_config.write() {
            *conn = rt.connection.clone();
        }

        if !rt.backends.is_empty() {
            let tls_sni = rt.backends[0].tls_sni.clone();
            let endpoints: Vec<String> = rt.backends.iter().map(|b| b.addr.clone()).collect();
            let route_backends = parse_backend_endpoints(&endpoints, 1, &tls_sni)
                .map_err(|errors| anyhow::anyhow!("{}", errors.join("; ")))?;
            let backend_names: Vec<String> = route_backends.iter().map(|b| b.name.clone()).collect();
            if let Ok(mut router) = runtime.router.write() {
                router.update(&route_backends)?;
            }
            if let Ok(mut health) = runtime.backend_health.write() {
                health.clear();
                for name in backend_names {
                    health.insert(name, crab_route::BackendHealth::new_healthy());
                }
            }
        }
    }

    if let Some(keys) = &snap.upstream_keys {
        let specs: Vec<UpstreamKeySpec> = keys
            .iter()
            .map(|k| UpstreamKeySpec {
                id: k.id.clone(),
                secret: k.secret.clone(),
                enabled: k.enabled,
            })
            .collect();
        let pool = UpstreamKeyPool::new(specs, upstream_cooldown_secs);
        runtime.replace_upstream_pool(pool);
    }

    runtime.replace_domain_policies(snap.domain_policies.clone());

    Ok(())
}
