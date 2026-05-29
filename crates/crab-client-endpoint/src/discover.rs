use crate::lan::{get_local_ip_addresses, select_primary_private_ip};
use crate::openresty::{DEFAULT_CONF_DIR, DEFAULT_GATEWAY_UPSTREAM, detect_gateway_base_url};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicUrlSource {
    Env,
    Observed,
    Frp,
    Openresty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientEndpointSnapshot {
    pub gateway_url: String,
    pub gateway_url_lan: Option<String>,
    /// Best URL for external clients (Cursor, OpenAI SDK). Serialized as `gateway_url_openresty` in Admin API.
    pub gateway_url_public: Option<String>,
    pub public_source: Option<PublicUrlSource>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub gateway_port: u16,
    pub use_https: bool,
    pub gateway_upstream: String,
    pub frp_config_paths: Vec<String>,
    pub openresty_conf_dirs: Vec<String>,
    /// Optional explicit override (env); highest priority when set.
    pub public_url_override: Option<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            gateway_port: 8080,
            use_https: false,
            gateway_upstream: DEFAULT_GATEWAY_UPSTREAM.to_string(),
            frp_config_paths: crate::frp::DEFAULT_FRPC_PATHS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            openresty_conf_dirs: vec![
                DEFAULT_CONF_DIR.to_string(),
                "/host/openresty/conf.d".to_string(),
            ],
            public_url_override: None,
        }
    }
}

impl DiscoveryConfig {
    pub fn from_env() -> Self {
        let gateway_port = std::env::var("CRABCACHE_GATEWAY_CLIENT_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);
        let use_https = std::env::var("CRABCACHE_HTTPS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let public_url_override = env_trimmed("CRABCACHE_GATEWAY_OPENRESTY_BASE_URL")
            .or_else(|| env_trimmed("CRABCACHE_GATEWAY_CLIENT_BASE_URL"));
        let gateway_upstream = env_trimmed("CRABCACHE_OPENRESTY_GATEWAY_UPSTREAM")
            .unwrap_or_else(|| DEFAULT_GATEWAY_UPSTREAM.to_string());
        let mut openresty_conf_dirs = vec![
            DEFAULT_CONF_DIR.to_string(),
            "/host/openresty/conf.d".to_string(),
        ];
        if let Some(dir) = env_trimmed("CRABCACHE_OPENRESTY_CONF_DIR") {
            openresty_conf_dirs.insert(0, dir);
        }
        let mut frp_config_paths: Vec<String> = crate::frp::DEFAULT_FRPC_PATHS
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let Some(path) = env_trimmed("CRABCACHE_FRPC_CONFIG") {
            frp_config_paths.insert(0, path);
        }
        Self {
            gateway_port,
            use_https,
            gateway_upstream,
            frp_config_paths,
            openresty_conf_dirs,
            public_url_override,
        }
    }
}

fn env_trimmed(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Discover client endpoint URLs without requiring env vars for FRP/OpenResty.
pub fn discover(config: &DiscoveryConfig) -> ClientEndpointSnapshot {
    let protocol = if config.use_https { "https" } else { "http" };
    let gateway_url = format!("{protocol}://127.0.0.1:{}", config.gateway_port);

    let selected_private = select_primary_private_ip(&get_local_ip_addresses());
    let gateway_url_lan =
        selected_private.map(|ip| format!("{protocol}://{ip}:{}", config.gateway_port));

    let (gateway_url_public, public_source) = resolve_public_url(config);

    ClientEndpointSnapshot {
        gateway_url,
        gateway_url_lan,
        gateway_url_public,
        public_source,
    }
}

fn resolve_public_url(config: &DiscoveryConfig) -> (Option<String>, Option<PublicUrlSource>) {
    if let Some(url) = config
        .public_url_override
        .as_ref()
        .map(|s| s.trim_end_matches('/').to_string())
    {
        return (Some(url), Some(PublicUrlSource::Env));
    }

    for path in &config.frp_config_paths {
        if let Some(url) =
            crate::frp::detect_frp_gateway_url_from_file(Path::new(path), config.gateway_port)
        {
            return (Some(url), Some(PublicUrlSource::Frp));
        }
    }

    for dir in &config.openresty_conf_dirs {
        if let Some(url) = detect_gateway_base_url(Path::new(dir), &config.gateway_upstream) {
            return (Some(url), Some(PublicUrlSource::Openresty));
        }
    }

    (None, None)
}

/// Re-apply observed URL from Pingora when it is more specific than static discovery.
pub fn refresh_public_from_observed(
    snapshot: &mut ClientEndpointSnapshot,
    observed_url: Option<String>,
) {
    let Some(url) = observed_url else {
        return;
    };
    // Prefer observed when we have no public URL, or current source is not env override.
    if snapshot.public_source != Some(PublicUrlSource::Env) {
        snapshot.gateway_url_public = Some(url);
        snapshot.public_source = Some(PublicUrlSource::Observed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_without_files_has_lan_only() {
        let snap = discover(&DiscoveryConfig {
            gateway_port: 8080,
            use_https: false,
            public_url_override: None,
            frp_config_paths: vec!["/nonexistent/frpc.toml".into()],
            openresty_conf_dirs: vec!["/nonexistent/conf.d".into()],
            ..Default::default()
        });
        assert_eq!(snap.gateway_url, "http://127.0.0.1:8080");
        assert!(snap.gateway_url_public.is_none());
    }
}
