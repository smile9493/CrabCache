use crab_route::Backend;
use std::net::{SocketAddr, ToSocketAddrs};

fn resolve_endpoint(endpoint: &str) -> Result<SocketAddr, String> {
    if let Ok(addr) = endpoint.parse::<SocketAddr>() {
        return Ok(addr);
    }
    match endpoint.to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(addr) => Ok(addr),
            None => Err(format!("No addresses resolved for '{endpoint}'")),
        },
        Err(e) => Err(format!("Cannot resolve '{endpoint}': {e}")),
    }
}

pub fn parse_backend_endpoints(
    endpoints: &[String],
    default_weight: u32,
    tls_sni: &str,
) -> Result<Vec<Backend>, Vec<String>> {
    let mut errors = Vec::new();
    let mut backends = Vec::new();

    for (i, endpoint) in endpoints.iter().enumerate() {
        match resolve_endpoint(endpoint) {
            Ok(addr) => {
                backends.push(Backend::new(
                    format!("backend-{}", i + 1),
                    addr,
                    default_weight,
                    tls_sni.to_string(),
                ));
            }
            Err(e) => errors.push(format!("Invalid endpoint '{endpoint}': {e}")),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    if backends.is_empty() {
        return Err(vec!["At least one endpoint is required".to_string()]);
    }
    Ok(backends)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_endpoints() {
        let eps = vec!["127.0.0.1:443".to_string(), "192.168.1.2:8443".to_string()];
        let backends = parse_backend_endpoints(&eps, 2, "api.deepseek.com").unwrap();
        assert_eq!(backends.len(), 2);
        assert_eq!(backends[0].weight, 2);
    }

    #[test]
    fn parse_valid_domain_endpoint() {
        let eps = vec!["api.deepseek.com:443".to_string()];
        let backends = parse_backend_endpoints(&eps, 1, "api.deepseek.com").unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].addr.port(), 443);
    }

    #[test]
    fn parse_rejects_empty() {
        let err = parse_backend_endpoints(&[], 1, "api.deepseek.com").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn parse_rejects_invalid() {
        let eps = vec!["not-a-socket".to_string()];
        assert!(parse_backend_endpoints(&eps, 1, "api.deepseek.com").is_err());
    }
}
