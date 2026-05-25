//! Parse upstream relay `base_url` (OpenAI-compatible origin, no `/v1` suffix).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamBaseUrl {
    /// Trimmed origin without trailing slash (e.g. `https://api.deepseek.com`).
    pub normalized: String,
    pub host: String,
    /// Value for HTTP `Host` header (includes non-default port).
    pub host_header: String,
    /// Ketama peer address (`host:port`).
    pub endpoint: String,
    pub tls_sni: String,
    pub use_tls: bool,
}

/// Normalize and parse an upstream base URL for relay routing.
pub fn parse_upstream_base_url(raw: &str) -> Result<UpstreamBaseUrl, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("base_url cannot be empty".to_string());
    }

    let (use_tls, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("base_url must start with http:// or https://".to_string());
    };

    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        return Err("base_url must include a host".to_string());
    }
    if rest.ends_with("/v1") {
        return Err(
            "base_url must not end with /v1; use the API origin only (paths are appended by the gateway)"
                .to_string(),
        );
    }

    let (authority, path_suffix) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if !path_suffix.is_empty() && path_suffix != "/" {
        return Err(format!(
            "base_url must not include a path suffix ({path_suffix}); use the API origin only"
        ));
    }
    if authority.is_empty() {
        return Err("base_url must include a host".to_string());
    }

    let (host, port) = parse_authority(authority, use_tls)?;

    let host_header = if (use_tls && port == 443) || (!use_tls && port == 80) {
        host.clone()
    } else {
        format!("{host}:{port}")
    };

    let endpoint = format!("{host}:{port}");
    let normalized = if use_tls {
        if port == 443 {
            format!("https://{host}")
        } else {
            format!("https://{host}:{port}")
        }
    } else if port == 80 {
        format!("http://{host}")
    } else {
        format!("http://{host}:{port}")
    };

    Ok(UpstreamBaseUrl {
        tls_sni: host.clone(),
        normalized,
        host,
        host_header,
        endpoint,
        use_tls,
    })
}

fn parse_authority(authority: &str, use_tls: bool) -> Result<(String, u16), String> {
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| format!("invalid IPv6 authority in base_url: [{authority}"))?;
        let host = authority[1..end].to_string();
        let port = if let Some(rest) = authority.get(end + 1..) {
            if let Some(p) = rest.strip_prefix(':') {
                p.parse::<u16>()
                    .map_err(|_| format!("invalid port in base_url: {authority}"))?
            } else if rest.is_empty() {
                default_port(use_tls)
            } else {
                return Err(format!("invalid authority in base_url: {authority}"));
            }
        } else {
            default_port(use_tls)
        };
        return Ok((host, port));
    }

    if let Some((host, port_str)) = authority.rsplit_once(':')
        && !host.is_empty()
        && port_str.chars().all(|c| c.is_ascii_digit())
    {
        let port = port_str
            .parse::<u16>()
            .map_err(|_| format!("invalid port in base_url: {authority}"))?;
        return Ok((host.to_string(), port));
    }

    Ok((authority.to_string(), default_port(use_tls)))
}

fn default_port(use_tls: bool) -> u16 {
    if use_tls { 443 } else { 80 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_deepseek_default() {
        let u = parse_upstream_base_url("https://api.deepseek.com").unwrap();
        assert_eq!(u.host, "api.deepseek.com");
        assert_eq!(u.endpoint, "api.deepseek.com:443");
        assert_eq!(u.host_header, "api.deepseek.com");
        assert_eq!(u.tls_sni, "api.deepseek.com");
    }

    #[test]
    fn rejects_v1_suffix() {
        assert!(parse_upstream_base_url("https://api.openai.com/v1").is_err());
    }

    #[test]
    fn rejects_path_suffix() {
        assert!(parse_upstream_base_url("https://relay.example.com/openai").is_err());
    }

    #[test]
    fn custom_port() {
        let u = parse_upstream_base_url("http://127.0.0.1:11434").unwrap();
        assert_eq!(u.endpoint, "127.0.0.1:11434");
        assert_eq!(u.host_header, "127.0.0.1:11434");
        assert!(!u.use_tls);
    }
}
