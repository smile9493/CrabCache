use anyhow::Result;
use crab_cache::TtlConfig;
use crab_control::parse_backend_endpoints;
use crab_proxy::{
    ConnectionConfig, DomainPolicy, ProfileBuildInput, RuntimeConfig, StoredKey, UpstreamKeyPool,
    UpstreamKeySpec, build_profile_runtime, resolve_profile_key_specs,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredKeySnapshot {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub enabled: bool,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub pipeline: Option<String>,
    #[serde(default)]
    pub upstream_profile: Option<String>,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub rpm_limit: u32,
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
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub supported_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamProfileSnapshot {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    pub tls_sni: String,
    pub endpoints: Vec<BackendSnapshot>,
    pub keys: Vec<UpstreamKeySnapshot>,
    /// Profile ID to try when this profile's upstream fails (5xx, 429, timeout).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_profile_id: Option<String>,
    /// Maximum number of fallback attempts per request (default 2).
    #[serde(default = "default_fallback_max_retries")]
    pub fallback_max_retries: u32,
}

fn default_fallback_max_retries() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ControlPlaneSnapshot {
    pub keys: HashMap<String, StoredKeySnapshot>,
    pub runtime: Option<RuntimeSnapshot>,
    /// `None` when the Redis key was never written (legacy); `Some` applies even if empty.
    #[serde(default)]
    pub upstream_keys: Option<Vec<UpstreamKeySnapshot>>,
    /// Full multi-vendor upstream profiles (preferred over `upstream_keys` alone).
    #[serde(default)]
    pub upstream_profiles: Option<Vec<UpstreamProfileSnapshot>>,
    #[serde(default)]
    pub domain_policies: IndexMap<String, DomainPolicy>,
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
                    project_id: k.project_id.clone(),
                    pipeline: k.pipeline.clone(),
                    upstream_profile: k.upstream_profile.clone(),
                    max_concurrent: k.max_concurrent,
                    rpm_limit: k.rpm_limit,
                },
            )
        })
        .collect();

    let ttl = runtime.ttl.read().clone();

    let fingerprint = {
        let f = runtime.fingerprint.read();
        FingerprintSnapshot {
            version: f.version,
            normalize_content: f.normalize_content,
        }
    };

    let upstream_base_url = runtime.upstream_base_url.read().clone();

    let fallback_model = runtime.fallback_model.read().clone();

    let backends: Vec<BackendSnapshot> = runtime
        .router
        .read()
        .meta()
        .iter()
        .map(|(addr, m)| BackendSnapshot {
            name: m.name.clone(),
            addr: addr.to_string(),
            weight: 1,
            tls_sni: m.tls_sni.clone(),
        })
        .collect();

    let connection = (**runtime.conn_config.read()).clone();

    let upstream_keys: Vec<UpstreamKeySnapshot> = runtime
        .upstream_pool()
        .to_specs()
        .into_iter()
        .map(|s| UpstreamKeySnapshot {
            id: s.id,
            secret: s.secret,
            enabled: s.enabled,
            account_id: s.account_id,
            supported_models: s.supported_models,
        })
        .collect();

    let domain_policies: IndexMap<String, DomainPolicy> =
        runtime.list_domain_policies().into_iter().collect();

    let upstream_profiles: Vec<UpstreamProfileSnapshot> = {
        let map = runtime.upstream_profiles.read();
        let mut ids: Vec<String> = map.keys().cloned().collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|id| {
                let profile = map.get(&id)?;
                let endpoints: Vec<BackendSnapshot> = profile
                    .router
                    .meta()
                    .iter()
                    .map(|(addr, m)| BackendSnapshot {
                        name: m.name.clone(),
                        addr: addr.to_string(),
                        weight: 1,
                        tls_sni: m.tls_sni.clone(),
                    })
                    .collect();
                let keys: Vec<UpstreamKeySnapshot> = profile
                    .resolve_upstream_pool()
                    .to_specs()
                    .into_iter()
                    .map(|s| UpstreamKeySnapshot {
                        id: s.id,
                        secret: s.secret,
                        enabled: s.enabled,
                        account_id: s.account_id,
                        supported_models: s.supported_models,
                    })
                    .collect();
                Some(UpstreamProfileSnapshot {
                    id: profile.id.clone(),
                    provider: profile.provider.as_str().to_string(),
                    base_url: profile.base_url.clone(),
                    fallback_model: profile.fallback_model.clone(),
                    fallback_profile_id: profile.fallback_profile_id.clone(),
                    fallback_max_retries: profile.fallback_max_retries,
                    tls_sni: profile.tls_sni.clone(),
                    endpoints,
                    keys,
                })
            })
            .collect()
    };

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
        upstream_profiles: Some(upstream_profiles),
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
                project_id: k.project_id.clone(),
                pipeline: k.pipeline.clone(),
                upstream_profile: k.upstream_profile.clone(),
                max_concurrent: k.max_concurrent,
                rpm_limit: k.rpm_limit,
            },
        );
    }

    if let Some(rt) = &snap.runtime {
        *runtime.ttl.write() = rt.ttl.clone();
        {
            let old = runtime.fingerprint.read().clone();
            let mut new_fp = (*old).clone();
            new_fp.version = rt.fingerprint.version;
            new_fp.normalize_content = rt.fingerprint.normalize_content;
            *runtime.fingerprint.write() = std::sync::Arc::new(new_fp);
        }
        runtime.set_stream_cache_enabled(rt.stream_cache_enabled);
        let _ = runtime.set_pipeline_runtime(
            crab_pipeline::PipelineMode::from_str(&rt.pipeline_mode),
            &rt.default_upstream_profile,
        );
        *runtime.upstream_base_url.write() = rt.upstream_base_url.clone();
        *runtime.fallback_model.write() = rt.fallback_model.clone();
        *runtime.conn_config.write() = Arc::new(rt.connection.clone());

        if !rt.backends.is_empty() {
            let tls_sni = rt.backends[0].tls_sni.clone();
            let endpoints: Vec<String> = rt.backends.iter().map(|b| b.addr.clone()).collect();
            let route_backends = parse_backend_endpoints(&endpoints, 1, &tls_sni)
                .map_err(|errors| anyhow::anyhow!("{}", errors.join("; ")))?;
            let _backend_names: Vec<String> =
                route_backends.iter().map(|b| b.name.clone()).collect();
            runtime.router.write().rebuild(&route_backends)?;
        }
    }

    if let Some(profiles) = &snap.upstream_profiles {
        let snapshot_ids: std::collections::HashSet<String> =
            profiles.iter().map(|p| p.id.clone()).collect();

        for p in profiles {
            let endpoints: Vec<String> = p.endpoints.iter().map(|b| b.addr.clone()).collect();
            let specs: Vec<UpstreamKeySpec> = p
                .keys
                .iter()
                .map(|k| UpstreamKeySpec {
                    id: k.id.clone(),
                    secret: k.secret.clone(),
                    enabled: k.enabled,
                    account_id: k.account_id.clone(),
                    supported_models: k.supported_models.clone(),
                })
                .collect();
            let input = ProfileBuildInput {
                id: p.id.clone(),
                provider: p.provider.clone(),
                base_url: p.base_url.clone(),
                fallback_model: p.fallback_model.clone(),
                endpoints,
                tls_sni: Some(p.tls_sni.clone()),
                default_weight: 1,
                proxy_url: None,
                fallback_profile_id: p.fallback_profile_id.clone(),
                fallback_max_retries: p.fallback_max_retries,
            };
            // Apply key fallback: explicit -> default profile -> legacy global pool.
            let resolved_specs = resolve_profile_key_specs(specs, runtime, &p.id);
            let profile =
                build_profile_runtime(input, resolved_specs, upstream_cooldown_secs, None)
                    .map_err(|e| anyhow::anyhow!(e))?;
            runtime
                .upsert_profile(profile)
                .map_err(|e| anyhow::anyhow!(e))?;
        }

        if !snapshot_ids.is_empty() {
            let default_id = runtime.default_upstream_profile_id();
            let stale_ids: Vec<String> = runtime
                .upstream_profiles
                .read()
                .keys()
                .filter(|id| !snapshot_ids.contains(*id))
                .cloned()
                .collect();
            for id in stale_ids {
                if id == default_id {
                    tracing::warn!(
                        profile_id = %id,
                        "snapshot omitted default upstream profile; keeping runtime copy"
                    );
                    continue;
                }
                match runtime.remove_profile(&id) {
                    Ok(()) => {
                        tracing::info!(profile_id = %id, "Removed upstream profile not in control-plane snapshot")
                    }
                    Err(e) => {
                        tracing::warn!(profile_id = %id, error = %e, "Could not remove stale upstream profile")
                    }
                }
            }
        }

        let default_id = runtime.default_upstream_profile_id();
        let _ = runtime.sync_legacy_from_profile_id(&default_id);
    } else if let Some(keys) = &snap.upstream_keys {
        let specs: Vec<UpstreamKeySpec> = keys
            .iter()
            .map(|k| UpstreamKeySpec {
                id: k.id.clone(),
                secret: k.secret.clone(),
                enabled: k.enabled,
                account_id: k.account_id.clone(),
                supported_models: k.supported_models.clone(),
            })
            .collect();
        let pool = UpstreamKeyPool::new(specs, upstream_cooldown_secs, 0);
        let default_id = runtime.default_upstream_profile_id();
        runtime
            .replace_profile_pool(&default_id, pool)
            .map_err(|e| anyhow::anyhow!(e))?;
    }

    runtime.replace_domain_policies(snap.domain_policies.clone());

    Ok(())
}
