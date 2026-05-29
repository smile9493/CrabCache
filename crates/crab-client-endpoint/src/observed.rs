use crate::PublicUrlSource;
use std::net::IpAddr;

/// Build a public client base URL from reverse-proxy / client request headers (Pingora).
pub fn url_from_forwarded_headers(
    host: Option<&str>,
    forwarded_host: Option<&str>,
    forwarded_proto: Option<&str>,
    forwarded_port: Option<&str>,
) -> Option<String> {
    let host = forwarded_host.or(host)?;
    let (hostname, port_from_host) = split_host_port(host)?;
    if !is_public_hostname(hostname) {
        return None;
    }

    let scheme = forwarded_proto
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| p == "http" || p == "https")
        .unwrap_or_else(|| {
            if port_from_host == Some(443) {
                "https".to_string()
            } else {
                "http".to_string()
            }
        });

    let port = forwarded_port
        .and_then(|p| p.trim().parse::<u16>().ok())
        .or(port_from_host);

    let default_port = if scheme == "https" { 443 } else { 80 };
    if port == Some(default_port) {
        Some(format!("{scheme}://{hostname}"))
    } else if let Some(port) = port {
        Some(format!("{scheme}://{hostname}:{port}"))
    } else {
        Some(format!("{scheme}://{hostname}"))
    }
}

fn split_host_port(host: &str) -> Option<(&str, Option<u16>)> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    if host.starts_with('[') {
        let end = host.find(']')?;
        let name = &host[1..end];
        let port = host[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse().ok());
        return Some((name, port));
    }
    if let Some((name, port)) = host.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
        && host.matches(':').count() == 1
    {
        return Some((name, Some(port)));
    }
    Some((host, None))
}

pub fn is_public_hostname(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host == "_" {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_public_ip(&ip);
    }
    // Domain name: treat as public when not a bare private IP string.
    !host.starts_with("127.") && !host.starts_with("192.168.") && !host.starts_with("10.")
}

fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_loopback() && !v4.is_private() && !v4.is_link_local() && !v4.is_unspecified()
        }
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified(),
    }
}

/// Read Pingora/http request headers and update snapshot public URL when appropriate.
pub fn update_snapshot_from_request(
    snapshot: &mut crate::ClientEndpointSnapshot,
    get_header: impl Fn(&str) -> Option<&str>,
) {
    let observed = url_from_forwarded_headers(
        get_header("host"),
        get_header("x-forwarded-host"),
        get_header("x-forwarded-proto"),
        get_header("x-forwarded-port"),
    );
    if let Some(url) = observed {
        snapshot.gateway_url_public = Some(url);
        snapshot.public_source = Some(PublicUrlSource::Observed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_host_with_port() {
        assert_eq!(
            url_from_forwarded_headers(None, Some("gateway.example.com:10801"), Some("http"), None)
                .as_deref(),
            Some("http://gateway.example.com:10801")
        );
    }

    #[test]
    fn rejects_private_host() {
        assert!(
            url_from_forwarded_headers(Some("192.168.1.100:8080"), None, Some("http"), None)
                .is_none()
        );
    }
}
