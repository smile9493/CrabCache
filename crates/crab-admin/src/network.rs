use local_ip_address::list_afinet_netifas;
use std::net::{IpAddr, Ipv4Addr};

pub fn get_local_ip_addresses() -> Vec<(String, IpAddr)> {
    let mut addresses = Vec::new();

    if let Ok(network_interfaces) = list_afinet_netifas() {
        for (name, ip) in network_interfaces {
            if let IpAddr::V4(ipv4) = ip {
                if !ipv4.is_loopback() && !ipv4.is_link_local() {
                    addresses.push((name, IpAddr::V4(ipv4)));
                }
            }
        }
    }

    addresses
}

pub fn is_private_ip(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();

    octets[0] == 10
        || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
        || (octets[0] == 192 && octets[1] == 168)
}

/// Returns true for virtual/container bridge interfaces that should not be used for LAN URLs.
pub fn is_virtual_interface(name: &str) -> bool {
    name == "lo"
        || name.starts_with("docker")
        || name.starts_with("br-")
        || name.starts_with("veth")
        || name.starts_with("virbr")
        || name.starts_with("tun")
        || name.starts_with("cni")
        || name.starts_with("flannel")
}

fn is_preferred_interface(name: &str) -> bool {
    name.starts_with("eth")
        || name.starts_with("en")
        || name.starts_with("wlan")
        || name.starts_with("wl")
        || name == "ens33"
        || name == "ens192"
}

fn ipv4_from_addr(ip: IpAddr) -> Option<Ipv4Addr> {
    match ip {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(_) => None,
    }
}

/// Select the best private IPv4 for LAN gateway URL display.
pub fn select_primary_private_ip(addresses: &[(String, IpAddr)]) -> Option<Ipv4Addr> {
    let candidates: Vec<_> = addresses
        .iter()
        .filter_map(|(name, ip)| {
            let v4 = ipv4_from_addr(*ip)?;
            if is_virtual_interface(name) || !is_private_ip(&v4) {
                return None;
            }
            Some((name.as_str(), v4))
        })
        .collect();

    candidates
        .iter()
        .find(|(name, _)| is_preferred_interface(name))
        .map(|(_, ip)| *ip)
        .or_else(|| candidates.first().map(|(_, ip)| *ip))
}

#[derive(Debug, Clone)]
pub struct NetworkInfoConfig {
    pub gateway_port: u16,
    pub use_https: bool,
    /// Explicit OpenResty / reverse-proxy client base URL (origin, no `/v1`).
    pub openresty_base_url: Option<String>,
    pub client_lan_host: Option<String>,
    /// Host path to scan `*.conf` for OpenResty (1Panel default when mounted).
    pub openresty_conf_dir: Option<String>,
    pub gateway_upstream: String,
}

impl Default for NetworkInfoConfig {
    fn default() -> Self {
        Self {
            gateway_port: 8080,
            use_https: false,
            openresty_base_url: None,
            client_lan_host: None,
            openresty_conf_dir: None,
            gateway_upstream: "127.0.0.1:8080".to_string(),
        }
    }
}

fn env_trimmed(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

impl NetworkInfoConfig {
    pub fn from_env() -> Self {
        let gateway_port = std::env::var("CRABCACHE_GATEWAY_CLIENT_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);

        let use_https = std::env::var("CRABCACHE_HTTPS")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let openresty_base_url = env_trimmed("CRABCACHE_GATEWAY_OPENRESTY_BASE_URL")
            .or_else(|| env_trimmed("CRABCACHE_GATEWAY_CLIENT_BASE_URL"))
            .map(|s| s.trim_end_matches('/').to_string());

        let client_lan_host = env_trimmed("CRABCACHE_GATEWAY_CLIENT_LAN_HOST");

        let openresty_conf_dir = env_trimmed("CRABCACHE_OPENRESTY_CONF_DIR");

        let gateway_upstream = env_trimmed("CRABCACHE_OPENRESTY_GATEWAY_UPSTREAM")
            .unwrap_or_else(|| "127.0.0.1:8080".to_string());

        Self {
            gateway_port,
            use_https,
            openresty_base_url,
            client_lan_host,
            openresty_conf_dir,
            gateway_upstream,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkInfo {
    pub primary_ip: Option<String>,
    pub all_ips: Vec<NetworkInterface>,
    pub gateway_url: String,
    pub gateway_url_lan: Option<String>,
    /// Public client base URL via OpenResty / reverse proxy (when detected or configured).
    pub gateway_url_openresty: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub ip: String,
    pub is_primary: bool,
}

impl NetworkInfo {
    pub fn build(config: NetworkInfoConfig) -> Self {
        let all_addresses = get_local_ip_addresses();
        let selected_private = select_primary_private_ip(&all_addresses);

        let primary_ip_str = config
            .client_lan_host
            .clone()
            .or_else(|| selected_private.map(|ip| ip.to_string()));

        let protocol = if config.use_https { "https" } else { "http" };

        let gateway_url = format!("{protocol}://127.0.0.1:{}", config.gateway_port);

        let gateway_url_lan = if let Some(host) = config.client_lan_host {
            Some(format!("{protocol}://{host}:{}", config.gateway_port))
        } else {
            selected_private.map(|ip| format!("{protocol}://{ip}:{}", config.gateway_port))
        };

        let gateway_url_openresty = config.openresty_base_url.clone().or_else(|| {
            config.openresty_conf_dir.as_ref().and_then(|dir| {
                crate::openresty::detect_gateway_base_url(
                    std::path::Path::new(dir),
                    &config.gateway_upstream,
                )
            })
        });

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
            gateway_url,
            gateway_url_lan,
            gateway_url_openresty,
        }
    }

    /// Backward-compatible helper for tests and simple callers.
    pub fn new(gateway_port: u16, use_https: bool) -> Self {
        Self::build(NetworkInfoConfig {
            gateway_port,
            use_https,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: &str, ip: &str) -> (String, IpAddr) {
        (
            name.to_string(),
            IpAddr::V4(ip.parse().expect("valid ipv4")),
        )
    }

    #[test]
    fn is_virtual_interface_detects_docker_and_bridges() {
        assert!(is_virtual_interface("docker0"));
        assert!(is_virtual_interface("br-abc123"));
        assert!(is_virtual_interface("veth123"));
        assert!(!is_virtual_interface("eth0"));
        assert!(!is_virtual_interface("enp0s3"));
    }

    #[test]
    fn select_primary_prefers_lan_over_docker() {
        let addrs = vec![
            addr("docker0", "172.17.0.1"),
            addr("enp0s3", "192.168.2.152"),
        ];
        let selected = select_primary_private_ip(&addrs);
        assert_eq!(selected, Some("192.168.2.152".parse().unwrap()));
    }

    #[test]
    fn select_primary_none_when_only_docker_bridge() {
        let addrs = vec![addr("docker0", "172.17.0.1")];
        assert_eq!(select_primary_private_ip(&addrs), None);
    }

    #[test]
    fn build_uses_lan_host_override() {
        let info = NetworkInfo::build(NetworkInfoConfig {
            gateway_port: 8080,
            use_https: false,
            openresty_base_url: None,
            client_lan_host: Some("192.168.1.100".to_string()),
            openresty_conf_dir: None,
            gateway_upstream: "127.0.0.1:8080".to_string(),
        });
        assert_eq!(info.gateway_url, "http://127.0.0.1:8080");
        assert_eq!(
            info.gateway_url_lan.as_deref(),
            Some("http://192.168.1.100:8080")
        );
        assert_eq!(info.primary_ip.as_deref(), Some("192.168.1.100"));
        assert!(info.gateway_url_openresty.is_none());
    }

    #[test]
    fn build_uses_openresty_base_url_override() {
        let info = NetworkInfo::build(NetworkInfoConfig {
            gateway_port: 8080,
            use_https: true,
            openresty_base_url: Some("https://v4.example.com:18000".to_string()),
            client_lan_host: None,
            openresty_conf_dir: None,
            gateway_upstream: "127.0.0.1:8080".to_string(),
        });
        assert_eq!(info.gateway_url, "https://127.0.0.1:8080");
        assert_eq!(
            info.gateway_url_openresty.as_deref(),
            Some("https://v4.example.com:18000")
        );
    }
}
