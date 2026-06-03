use maxminddb::Reader;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::net::IpAddr;
use std::path::Path;
use tracing::{info, warn};

/// Global GeoIP reader instance, loaded once at startup.
static GEOIP_READER: Lazy<Option<Reader<Vec<u8>>>> = Lazy::new(|| {
    if let Ok(custom) = std::env::var("CRABCACHE_GEODB_PATH") {
        if Path::new(&custom).exists() {
            match Reader::open_readfile(&custom) {
                Ok(reader) => {
                    info!("Loaded GeoIP database from env: {}", custom);
                    return Some(reader);
                }
                Err(e) => {
                    warn!("Failed to load GeoIP database from {}: {}", custom, e);
                }
            }
        }
    }
    let db_paths = [
        "models/dbip-city-lite.mmdb",
        "models/GeoLite2-City.mmdb",
        "/usr/share/GeoIP/GeoLite2-City.mmdb",
        "/var/lib/GeoIP/GeoLite2-City.mmdb",
        "GeoLite2-City.mmdb",
    ];
    for path in &db_paths {
        if Path::new(path).exists() {
            match Reader::open_readfile(path) {
                Ok(reader) => {
                    info!("Loaded GeoIP database from {}", path);
                    return Some(reader);
                }
                Err(e) => {
                    warn!("Failed to load GeoIP database from {}: {}", path, e);
                }
            }
        }
    }
    warn!("GeoIP database not found, IP geolocation disabled");
    None
});

/// Resolve an IP address to a location string (city, region, country).
/// Uses serde_json::Value for generic MMDB decoding (compatible with both
/// MaxMind GeoLite2 and DB-IP City Lite schemas).
pub fn resolve_ip_location(ip_str: &str) -> String {
    let reader = match GEOIP_READER.as_ref() {
        Some(r) => r,
        None => return String::new(),
    };
    let ip: IpAddr = match ip_str.parse() {
        Ok(ip) => ip,
        Err(_) => return String::new(),
    };
    let val: Value = match reader.lookup(ip) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let mut parts = Vec::new();
    // City name (prefer English, fallback to any language).
    if let Some(name) = extract_name(&val, "city") {
        parts.push(name);
    }
    // Subdivision (state/province).
    if let Some(name) = extract_subdivision_name(&val) {
        parts.push(name);
    }
    // Country name.
    if let Some(name) = extract_name(&val, "country") {
        parts.push(name);
    }
    // Fallback: registered_country (DB-IP sometimes uses this).
    if parts.is_empty() {
        if let Some(name) = extract_name(&val, "registered_country") {
            parts.push(name);
        }
    }
    parts.join(", ")
}

/// Extract English name from `val[key]["names"]["en"]`.
fn extract_name(val: &Value, key: &str) -> Option<String> {
    let obj = val.get(key)?;
    let names = obj.get("names")?;
    names
        .get("en")
        .and_then(|v| v.as_str())
        .or_else(|| {
            names.as_object().and_then(|m| m.values().next()?.as_str())
        })
        .map(|s| s.to_string())
}

/// Extract name from `val["subdivisions"][0]["names"]["en"]`.
fn extract_subdivision_name(val: &Value) -> Option<String> {
    let subs = val.get("subdivisions")?.as_array()?;
    let first = subs.first()?;
    let names = first.get("names")?;
    names
        .get("en")
        .and_then(|v| v.as_str())
        .or_else(|| {
            names.as_object().and_then(|m| m.values().next()?.as_str())
        })
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_invalid_ip() {
        assert_eq!(resolve_ip_location("not-an-ip"), "");
    }

    #[test]
    fn test_resolve_private_ip() {
        let result = resolve_ip_location("192.168.1.1");
        let _ = result;
    }
}
