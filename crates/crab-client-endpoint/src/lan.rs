use local_ip_address::list_afinet_netifas;
use std::net::{IpAddr, Ipv4Addr};

pub fn get_local_ip_addresses() -> Vec<(String, IpAddr)> {
    let mut addresses = Vec::new();
    if let Ok(network_interfaces) = list_afinet_netifas() {
        for (name, ip) in network_interfaces {
            if let IpAddr::V4(ipv4) = ip
                && !ipv4.is_loopback()
                && !ipv4.is_link_local()
            {
                addresses.push((name, IpAddr::V4(ipv4)));
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
