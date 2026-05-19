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

pub fn get_primary_local_ip() -> Option<IpAddr> {
    let addresses = get_local_ip_addresses();

    addresses
        .into_iter()
        .find(|(name, _)| {
            name.starts_with("eth")
                || name.starts_with("en")
                || name.starts_with("wlan")
                || name.starts_with("wl")
                || name == "ens33"
                || name == "ens192"
        })
        .map(|(_, ip)| ip)
        .or_else(|| get_local_ip_addresses().first().map(|(_, ip)| *ip))
}

pub fn is_private_ip(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();

    octets[0] == 10
        || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
        || (octets[0] == 192 && octets[1] == 168)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkInfo {
    pub primary_ip: Option<String>,
    pub all_ips: Vec<NetworkInterface>,
    pub gateway_url: String,
    pub gateway_url_lan: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetworkInterface {
    pub name: String,
    pub ip: String,
    pub is_primary: bool,
}

impl NetworkInfo {
    pub fn new(gateway_port: u16, use_https: bool) -> Self {
        let primary_ip = get_primary_local_ip();
        let all_addresses = get_local_ip_addresses();

        let primary_ip_str = primary_ip.map(|ip| ip.to_string());

        let all_ips = all_addresses
            .iter()
            .map(|(name, ip)| NetworkInterface {
                name: name.clone(),
                ip: ip.to_string(),
                is_primary: Some(*ip) == primary_ip,
            })
            .collect();

        let protocol = if use_https { "https" } else { "http" };

        let gateway_url = format!("{protocol}://127.0.0.1:{gateway_port}");

        let gateway_url_lan =
            primary_ip.map(|ip| format!("{protocol}://{ip}:{gateway_port}"));

        NetworkInfo {
            primary_ip: primary_ip_str,
            all_ips,
            gateway_url,
            gateway_url_lan,
        }
    }
}
