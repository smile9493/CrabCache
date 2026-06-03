use maxminddb::geoip2::City;
use maxminddb::Reader;
use once_cell::sync::Lazy;
use std::net::IpAddr;
use std::path::Path;
use tracing::{info, warn};

/// Global GeoIP reader instance, loaded once at startup.
static GEOIP_READER: Lazy<Option<Reader<Vec<u8>>>> = Lazy::new(|| {
    // Environment override takes precedence.
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

/// Resolve an IP address to a location string (city, country).
/// Returns empty string if resolution fails or database is not available.
pub fn resolve_ip_location(ip_str: &str) -> String {
    let reader = match GEOIP_READER.as_ref() {
        Some(r) => r,
        None => return String::new(),
    };
    let ip: IpAddr = match ip_str.parse() {
        Ok(ip) => ip,
        Err(_) => return String::new(),
    };
    let city: City = match reader.lookup(ip) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut parts = Vec::new();
    // City name (prefer English, fallback to any language).
    if let Some(city_data) = &city.city {
        if let Some(names) = &city_data.names {
            if let Some(name) = names.get("en").or_else(|| names.values().next()) {
                parts.push(name.to_string());
            }
        }
    }
    // Subdivision (state/province) name.
    if let Some(subs) = &city.subdivisions {
        if let Some(first) = subs.first() {
            if let Some(names) = &first.names {
                if let Some(name) = names.get("en").or_else(|| names.values().next()) {
                    parts.push(name.to_string());
                }
            }
        }
    }
    // Country name.
    if let Some(country) = &city.country {
        if let Some(names) = &country.names {
            if let Some(name) = names.get("en").or_else(|| names.values().next()) {
                parts.push(name.to_string());
            }
        }
    }
    parts.join(", ")
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
        // Private IPs may not be in the database, should return empty.
        let result = resolve_ip_location("192.168.1.1");
        // Just ensure it doesn't panic.
        let _ = result;
    }
}
