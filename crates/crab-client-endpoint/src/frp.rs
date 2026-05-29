use std::path::{Path, PathBuf};

/// Default frpc config paths on Linux deployments.
pub const DEFAULT_FRPC_PATHS: &[&str] = &["/etc/frp/frpc.toml", "/etc/frp/frpc.ini"];

/// Detect public client base URL from frpc config when a TCP proxy forwards to `local_port`.
pub fn detect_frp_gateway_url(gateway_local_port: u16) -> Option<String> {
    for path in DEFAULT_FRPC_PATHS {
        if let Some(url) = detect_frp_gateway_url_from_file(Path::new(path), gateway_local_port) {
            return Some(url);
        }
    }
    None
}

pub fn detect_frp_gateway_url_from_file(path: &Path, gateway_local_port: u16) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_frpc_content(&content, gateway_local_port)
}

fn parse_frpc_content(content: &str, gateway_local_port: u16) -> Option<String> {
    let doc: toml::Table = toml::from_str(content).ok()?;
    let server_addr = doc
        .get("serverAddr")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    let proxies = doc.get("proxies").and_then(|v| v.as_array())?;
    for proxy in proxies {
        let table = proxy.as_table()?;
        let local_port = table
            .get("localPort")
            .and_then(|v| v.as_integer())
            .and_then(|p| u16::try_from(p).ok())?;
        if local_port != gateway_local_port {
            continue;
        }
        let remote_port = table
            .get("remotePort")
            .and_then(|v| v.as_integer())
            .and_then(|p| u16::try_from(p).ok())?;
        let local_ip = table
            .get("localIP")
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1");
        if local_ip != "127.0.0.1" && local_ip != "::1" {
            continue;
        }
        // FRP exposes loopback TCP; public HTTPS is terminated on the frps host (OpenResty).
        // CrabCache convention: remotePort 10801/10810 → TLS ports 18000/18010.
        let public_port = match remote_port {
            10801 => 18000,
            10810 => 18010,
            other => other,
        };
        return Some(format!("https://{server_addr}:{public_port}"));
    }
    None
}

pub fn extra_frpc_paths() -> Vec<PathBuf> {
    DEFAULT_FRPC_PATHS.iter().map(PathBuf::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
serverAddr = "gateway.example.com"
serverPort = 17000

[[proxies]]
name = "crabcache-gateway"
type = "tcp"
localIP = "127.0.0.1"
localPort = 8080
remotePort = 10801

[[proxies]]
name = "crabcache-admin"
type = "tcp"
localIP = "127.0.0.1"
localPort = 18001
remotePort = 10810
"#;

    #[test]
    fn parses_gateway_proxy() {
        assert_eq!(
            parse_frpc_content(SAMPLE, 8080).as_deref(),
            Some("https://gateway.example.com:18000")
        );
    }

    #[test]
    fn admin_proxy_uses_admin_remote_port() {
        assert_eq!(
            parse_frpc_content(SAMPLE, 18001).as_deref(),
            Some("https://gateway.example.com:18010")
        );
    }

    #[test]
    fn unknown_local_port_returns_none() {
        assert_eq!(parse_frpc_content(SAMPLE, 9999), None);
    }
}
