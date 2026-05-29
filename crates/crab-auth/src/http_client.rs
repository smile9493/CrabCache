use std::time::Duration;
use wreq_util::Emulation;

use crate::oauth::AuthError;

const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Build a `wreq::Client` with Chrome TLS fingerprint, cookie store, and optional proxy.
///
/// Uses `Chrome130` emulation (uTLS via BoringSSL) to match real browser TLS/HTTP2 fingerprints,
/// bypassing Cloudflare and similar anti-bot protections on OAuth endpoints.
///
/// `proxy_url` accepts `socks5://`, `socks5h://`, `http://`, or `https://` proxy URIs.
pub fn build_anti_detect_client(proxy_url: Option<&str>) -> Result<wreq::Client, AuthError> {
    let mut builder = wreq::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .emulation(Emulation::Chrome130)
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
/// This is the "second layer" of anti-detection: `Emulation::Chrome130` handles TLS fingerprint,
/// while this function strips proxy-chain headers that might have leaked in transit.
pub fn scrub_request_headers(mut req: wreq::Request) -> wreq::Request {
    let headers = req.headers_mut();
    for name in SCRUB_HEADER_NAMES {
        headers.remove(*name);
    }
    req
}
