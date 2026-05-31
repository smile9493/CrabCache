use std::time::Duration;
use wreq_util::Emulation;

use crate::oauth::AuthError;

const REQUEST_TIMEOUT_SECS: u64 = 30;

/// TLS fingerprint emulation profile for anti-detect HTTP clients.
///
/// Maps to `wreq_util::Emulation` variants. Controls the browser TLS/HTTP2
/// fingerprint used during OAuth token exchange and refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsEmulationProfile {
    /// Chrome 130 emulation (current default). Matches modern Chrome TLS/HTTP2 fingerprint.
    #[default]
    Chrome130,
    /// Chrome 124 emulation. Aligns with OmniRoute's `wreq-js` `browser: "chrome_124"`.
    Chrome124,
}

impl TlsEmulationProfile {
    /// Resolve from `CRABCACHE_OAUTH_TLS_EMULATION` env var, falling back to `Chrome130`.
    pub fn from_env() -> Self {
        match std::env::var("CRABCACHE_OAUTH_TLS_EMULATION")
            .ok()
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("chrome124") => Self::Chrome124,
            Some("chrome130") | None => Self::Chrome130,
            Some(other) => {
                tracing::warn!(
                    tls_emulation = other,
                    "Unknown CRABCACHE_OAUTH_TLS_EMULATION value, falling back to Chrome130"
                );
                Self::Chrome130
            }
        }
    }

    /// Map to the underlying `wreq_util::Emulation` variant.
    pub fn to_emulation(self) -> Emulation {
        match self {
            Self::Chrome130 => Emulation::Chrome130,
            Self::Chrome124 => Emulation::Chrome124,
        }
    }

    /// Human-readable label for logging.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chrome130 => "Chrome130",
            Self::Chrome124 => "Chrome124",
        }
    }
}

/// Build a `wreq::Client` with browser TLS fingerprint, cookie store, and optional proxy.
///
/// Uses Chrome emulation (uTLS via BoringSSL) to match real browser TLS/HTTP2 fingerprints,
/// bypassing Cloudflare and similar anti-bot protections on OAuth endpoints.
///
/// `proxy_url` accepts `socks5://`, `socks5h://`, `http://`, or `https://` proxy URIs.
pub fn build_anti_detect_client(proxy_url: Option<&str>) -> Result<wreq::Client, AuthError> {
    build_anti_detect_client_with_profile(proxy_url, TlsEmulationProfile::from_env())
}

/// Build a `wreq::Client` with a specific TLS emulation profile.
pub fn build_anti_detect_client_with_profile(
    proxy_url: Option<&str>,
    profile: TlsEmulationProfile,
) -> Result<wreq::Client, AuthError> {
    let mut builder = wreq::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .emulation(profile.to_emulation())
        .cookie_store(true);

    if let Some(proxy_url) = proxy_url {
        let proxy = wreq::Proxy::all(proxy_url)
            .map_err(|e| AuthError::OAuth(format!("invalid proxy URL '{proxy_url}': {e}")))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| AuthError::OAuth(format!("failed to build anti-detect HTTP client: {e}")))
}

/// Build a client with default settings (no proxy) — convenience shortcut.
pub fn build_default_client() -> Result<wreq::Client, AuthError> {
    build_anti_detect_client(None)
}

/// Headers to strip from outgoing OAuth requests to prevent client/proxy fingerprinting.
///
/// Mirrors CLIProxyAPI's `ScrubProxyAndFingerprintHeaders`:
/// - `X-Forwarded-*`, `X-Real-IP`, `Forwarded`, `Via`: proxy chain markers
/// - `Referer`: origin page leak
/// - `Accept-Encoding`: prevents mismatched zstd/gzip fingerprint
const SCRUB_HEADER_NAMES: &[&str] = &[
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip",
    "forwarded",
    "via",
    "referer",
    "accept-encoding",
];

/// Scrub proxy/fingerprint headers from a built `wreq::Request` before sending.
///
/// This is the "second layer" of anti-detection: the emulation profile handles TLS fingerprint,
/// while this function strips proxy-chain headers that might have leaked in transit.
pub fn scrub_request_headers(mut req: wreq::Request) -> wreq::Request {
    let headers = req.headers_mut();
    for name in SCRUB_HEADER_NAMES {
        headers.remove(*name);
    }
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_profile_default_is_chrome130() {
        let profile = TlsEmulationProfile::default();
        assert_eq!(profile, TlsEmulationProfile::Chrome130);
        assert_eq!(profile.as_str(), "Chrome130");
    }

    #[test]
    fn test_tls_profile_from_env_default() {
        // When env var is not set, should default to Chrome130
        unsafe { std::env::remove_var("CRABCACHE_OAUTH_TLS_EMULATION"); }
        assert_eq!(TlsEmulationProfile::from_env(), TlsEmulationProfile::Chrome130);
    }

    #[test]
    fn test_tls_profile_from_env_chrome124() {
        unsafe {
            std::env::set_var("CRABCACHE_OAUTH_TLS_EMULATION", "chrome124");
        }
        assert_eq!(TlsEmulationProfile::from_env(), TlsEmulationProfile::Chrome124);
        unsafe { std::env::remove_var("CRABCACHE_OAUTH_TLS_EMULATION"); }
    }

    #[test]
    fn test_tls_profile_from_env_case_insensitive() {
        unsafe {
            std::env::set_var("CRABCACHE_OAUTH_TLS_EMULATION", "Chrome130");
        }
        assert_eq!(TlsEmulationProfile::from_env(), TlsEmulationProfile::Chrome130);
        unsafe { std::env::remove_var("CRABCACHE_OAUTH_TLS_EMULATION"); }
    }

    #[test]
    fn test_tls_profile_from_env_unknown_falls_back() {
        unsafe {
            std::env::set_var("CRABCACHE_OAUTH_TLS_EMULATION", "firefox99");
        }
        assert_eq!(TlsEmulationProfile::from_env(), TlsEmulationProfile::Chrome130);
        unsafe { std::env::remove_var("CRABCACHE_OAUTH_TLS_EMULATION"); }
    }

    #[test]
    fn test_build_anti_detect_client_no_proxy() {
        let client = build_anti_detect_client(None);
        assert!(client.is_ok());
    }

    #[test]
    fn test_build_anti_detect_client_invalid_proxy() {
        let result = build_anti_detect_client(Some("not-a-valid-proxy"));
        assert!(result.is_err());
    }

    #[test]
    fn test_scrub_request_headers_removes_proxy_markers() {
        let client = build_default_client().unwrap();
        let req = client
            .get("https://example.com")
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "5.6.7.8")
            .header("referer", "https://evil.com")
            .header("accept-encoding", "gzip")
            .header("authorization", "Bearer token123")
            .build()
            .unwrap();

        let req = scrub_request_headers(req);
        let headers = req.headers();

        // Scrubbed headers should be absent
        assert!(!headers.contains_key("x-forwarded-for"));
        assert!(!headers.contains_key("x-real-ip"));
        assert!(!headers.contains_key("referer"));
        assert!(!headers.contains_key("accept-encoding"));

        // Non-scrubbed headers should remain
        assert!(headers.contains_key("authorization"));
    }
}
