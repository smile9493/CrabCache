//! Build `UpstreamProfileRuntime` from Management API inputs.

use crate::runtime::RuntimeConfig;
use crate::upstream_pool::{UpstreamKeyPool, UpstreamKeySpec};
use crate::upstream_profile::UpstreamProfileRuntime;
use crab_control::parse_upstream_base_url;
use crab_pipeline::UpstreamProvider;
use crab_route::{Backend, LbRouter};
use parking_lot::RwLock;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct ProfileBuildInput {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub fallback_model: String,
    pub endpoints: Vec<String>,
    pub tls_sni: Option<String>,
    pub default_weight: u32,
    pub proxy_url: Option<String>,
    /// Profile ID to try when this profile's upstream fails (5xx, 429, timeout).
    pub fallback_profile_id: Option<String>,
    /// Maximum number of fallback attempts per request (default 2).
    pub fallback_max_retries: u32,
}

pub fn parse_profile_backends(input: &ProfileBuildInput) -> Result<Vec<Backend>, String> {
    let parsed = parse_upstream_base_url(&input.base_url)?;
    let mut endpoints = input.endpoints.clone();
    if endpoints.is_empty() {
        endpoints.push(parsed.endpoint);
    }
    let tls_sni = input
        .tls_sni
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or(parsed.tls_sni);
    let weight = input.default_weight.max(1);
    let mut backends = Vec::new();
    let mut errors = Vec::new();
    for (i, endpoint) in endpoints.iter().enumerate() {
        if let Ok(addr) = endpoint.parse::<SocketAddr>() {
            backends.push(Backend::new(
                format!("{}-backend-{}", input.id, i + 1),
                addr,
                weight,
                tls_sni.clone(),
            ));
            continue;
        }
        match endpoint.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    backends.push(Backend::new(
                        format!("{}-backend-{}", input.id, i + 1),
                        addr,
                        weight,
                        tls_sni.clone(),
                    ));
                } else {
                    errors.push(format!("No addresses for '{endpoint}'"));
                }
            }
            Err(e) => errors.push(format!("Cannot resolve '{endpoint}': {e}")),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if backends.is_empty() {
        return Err(format!("profile '{}' has no endpoints", input.id));
    }
    Ok(backends)
}

/// True when a profile may reuse the default / legacy DeepSeek key pool on empty explicit keys.
fn profile_may_share_default_key_pool(runtime: &RuntimeConfig, profile_id: &str) -> bool {
    let default_id = runtime.default_upstream_profile_id();
    if profile_id == default_id {
        return true;
    }
    let Some(profile) = runtime.profile(profile_id) else {
        return false;
    };
    let Some(default_profile) = runtime.profile(&default_id) else {
        return false;
    };
    profile.provider == default_profile.provider
}

/// Resolve key specs for a profile with fallback to existing/default/legacy pools.
///
/// Priority: `explicit` > existing profile pool > default profile pool > legacy `runtime.upstream_pool()` > empty.
/// Cross-provider inheritance (e.g. Codex inheriting DeepSeek `sk-*` keys) is never allowed.
pub fn resolve_profile_key_specs(
    explicit: Vec<UpstreamKeySpec>,
    runtime: &RuntimeConfig,
    profile_id: &str,
) -> Vec<UpstreamKeySpec> {
    if !explicit.is_empty() {
        return explicit;
    }
    // 1) Existing profile pool — preserve keys already configured for this profile.
    if let Some(existing_profile) = runtime.profile(profile_id) {
        let specs = existing_profile.resolve_upstream_pool().to_specs();
        if !specs.is_empty() {
            tracing::debug!(
                profile_id,
                key_count = specs.len(),
                "Profile preserving its existing keys"
            );
            return specs;
        }
    }
    if !profile_may_share_default_key_pool(runtime, profile_id) {
        warn!(
            profile_id,
            "No upstream keys for profile; skipping default/legacy pool inheritance (provider mismatch)"
        );
        return Vec::new();
    }
    // 2) Default profile pool (same provider family only).
    let default_id = runtime.default_upstream_profile_id();
    if let Some(default_profile) = runtime.profile(&default_id) {
        let specs = default_profile.resolve_upstream_pool().to_specs();
        if !specs.is_empty() {
            if profile_id != default_id {
                tracing::debug!(
                    profile_id,
                    default_profile_id = %default_id,
                    key_count = specs.len(),
                    "Profile inheriting keys from default profile"
                );
            }
            return specs;
        }
    }
    // 3) Legacy global upstream_pool.
    let legacy = runtime.upstream_pool().to_specs();
    if !legacy.is_empty() {
        tracing::debug!(
            profile_id,
            key_count = legacy.len(),
            "Profile inheriting keys from legacy global pool"
        );
        return legacy;
    }
    warn!(
        profile_id,
        "No upstream keys available for profile (explicit, existing, default, and legacy pools are all empty)"
    );
    Vec::new()
}

pub fn build_profile_runtime(
    input: ProfileBuildInput,
    key_specs: Vec<UpstreamKeySpec>,
    cooldown_secs: u64,
    existing_pool: Option<Arc<RwLock<Arc<UpstreamKeyPool>>>>,
) -> Result<Arc<UpstreamProfileRuntime>, String> {
    let backends = parse_profile_backends(&input)?;
    let router = LbRouter::new(&backends).map_err(|e| e.to_string())?;
    let parsed = parse_upstream_base_url(&input.base_url)?;
    let tls_sni = input
        .tls_sni
        .filter(|s| !s.is_empty())
        .unwrap_or(parsed.tls_sni);

    let pool_handle = match existing_pool {
        Some(existing) => existing,
        None => {
            let pool = if key_specs.is_empty() {
                UpstreamKeyPool::new(Vec::new(), cooldown_secs, 0)
            } else {
                UpstreamKeyPool::new(key_specs.clone(), cooldown_secs, 0)
            };
            Arc::new(RwLock::new(pool))
        }
    };

    if !key_specs.is_empty() {
        let current = pool_handle.read().clone();
        let new_pool = UpstreamKeyPool::hot_replace(&current, key_specs);
        *pool_handle.write() = new_pool;
    }

    Ok(Arc::new(UpstreamProfileRuntime {
        id: input.id.clone(),
        provider: UpstreamProvider::from_str(&input.provider),
        base_url: parsed.normalized,
        fallback_model: input.fallback_model.trim().to_string(),
        fallback_profile_id: input.fallback_profile_id.clone(),
        fallback_max_retries: input.fallback_max_retries,
        tls_sni,
        router,
        upstream_pool: pool_handle,
        proxy_url: input.proxy_url.filter(|s| !s.is_empty()),
    }))
}
