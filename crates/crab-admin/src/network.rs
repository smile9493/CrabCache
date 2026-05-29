use crab_client_endpoint::{
    ClientEndpointSnapshot, DiscoveryConfig, discover, get_local_ip_addresses,
    select_primary_private_ip,
};
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct NetworkInfoConfig {
    pub discovery: DiscoveryConfig,
}

impl Default for NetworkInfoConfig {
    fn default() -> Self {
        Self {
            discovery: DiscoveryConfig::default(),
        }
    }
}

impl NetworkInfoConfig {
    /// Load discovery settings (optional env overrides only; FRP/OpenResty auto-scan by default).
    pub fn from_env() -> Self {
        Self {
            discovery: DiscoveryConfig::from_env(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkInfo {
    pub primary_ip: Option<String>,
    pub all_ips: Vec<NetworkInterface>,
    pub gateway_url: String,
    pub gateway_url_lan: Option<String>,
    /// Public client Base URL (FRP / OpenResty / Pingora-observed). Legacy field name for dashboard.
    pub gateway_url_openresty: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub ip: String,
    pub is_primary: bool,
}

impl NetworkInfo {
    pub fn from_snapshot(snap: ClientEndpointSnapshot) -> Self {
        let all_addresses = get_local_ip_addresses();
        let selected_private = select_primary_private_ip(&all_addresses);
        let primary_ip_str = selected_private.map(|ip| ip.to_string());
        let primary_for_flag = selected_private.map(IpAddr::V4);

        let all_ips = all_addresses
            .iter()
            .map(|(name, ip)| NetworkInterface {
                name: name.clone(),
                ip: ip.to_string(),
                is_primary: primary_for_flag == Some(*ip),
            })
            .collect();

        NetworkInfo {
            primary_ip: primary_ip_str,
            all_ips,
            gateway_url: snap.gateway_url,
            gateway_url_lan: snap.gateway_url_lan,
            gateway_url_openresty: snap.gateway_url_public,
        }
    }

    pub fn build(config: NetworkInfoConfig) -> Self {
        Self::from_snapshot(discover(&config.discovery))
    }

    /// Backward-compatible helper for tests.
    pub fn new(gateway_port: u16, use_https: bool) -> Self {
        Self::build(NetworkInfoConfig {
            discovery: DiscoveryConfig {
                gateway_port,
                use_https,
                ..DiscoveryConfig::default()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_has_loopback_gateway_url() {
        let info = NetworkInfo::new(8080, false);
        assert_eq!(info.gateway_url, "http://127.0.0.1:8080");
    }
}
