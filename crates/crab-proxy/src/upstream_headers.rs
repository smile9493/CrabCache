//! Normalize upstream HTTP/1 request headers when the gateway replaces the body.

use http::header;
use pingora_http::RequestHeader;

/// Upstream `User-Agent` aligned with curl / Go `net/http` (not Cursor/Pingora defaults).
pub const UPSTREAM_USER_AGENT: &str = "curl/8.7.1";

/// Negotiate compressed upstream responses (gateway decompresses in `upstream_response_decompress`).
pub const UPSTREAM_ACCEPT_ENCODING: &str = "gzip, deflate, br";

/// Clear client/H2-proxy framing headers and set a single `Content-Length` body.
///
/// Pingora may add `Transfer-Encoding: chunked` when proxying HTTP/2 clients without
/// `Content-Length`. If we then set `Content-Length` without removing `TE`, the H1
/// client writer prefers chunked encoding while the wire headers list both — upstream
/// servers often stall until timeout (~15s).
pub fn normalize_replaced_body_headers(req: &mut RequestHeader, body_len: usize) {
    let _ = req.remove_header(&header::TRANSFER_ENCODING);
    let _ = req.remove_header(&header::CONTENT_LENGTH);
    let _ = req.remove_header(&header::CONTENT_ENCODING);
    let _ = req.remove_header(&header::EXPECT);
    let _ = req.insert_header(header::CONTENT_LENGTH, body_len.to_string());
    if req.headers.get(header::CONTENT_TYPE).is_none() {
        let _ = req.insert_header(header::CONTENT_TYPE, "application/json");
    }
}

/// Replace client/Cursor headers so the upstream request resembles curl or deepseek-cursor-proxy.
pub fn smooth_upstream_client_headers(req: &mut RequestHeader, is_streaming: bool) {
    let _ = req.remove_header(&header::USER_AGENT);
    let _ = req.remove_header(&header::ACCEPT);
    let _ = req.remove_header(&header::ACCEPT_ENCODING);
    let accept = if is_streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    let _ = req.insert_header(header::USER_AGENT, UPSTREAM_USER_AGENT);
    let _ = req.insert_header(header::ACCEPT, accept);
    // SSE streams: identity avoids per-chunk gzip decompress with negligible compression gain.
    let encoding = if is_streaming {
        "identity"
    } else {
        UPSTREAM_ACCEPT_ENCODING
    };
    let _ = req.insert_header(header::ACCEPT_ENCODING, encoding);
}

/// Set `Content-Encoding` on upstream requests after `normalize_replaced_body_headers`.
pub fn apply_upstream_request_content_encoding(req: &mut RequestHeader, encoding: &str) {
    let _ = req.insert_header(header::CONTENT_ENCODING, encoding);
}

/// Headers for `streaming_body_forward` before the full body exists at `upstream_request_filter`.
///
/// Intentionally omits `Content-Length` (chunked transfer); MiMo accepts chunked bodies.
/// Must not send `Content-Length: 0` (upstream would parse empty JSON). The real guard against
/// empty upstream bodies is in `request_body_filter` (`suppress_upstream` / empty-body checks).
/// Body is sent once at client EOS via `request_body_filter` with the prepared payload.
pub fn prepare_streaming_deferred_upstream_headers(req: &mut RequestHeader, is_streaming: bool) {
    let _ = req.remove_header(&header::TRANSFER_ENCODING);
    let _ = req.remove_header(&header::CONTENT_LENGTH);
    let _ = req.remove_header(&header::CONTENT_ENCODING);
    let _ = req.remove_header(&header::EXPECT);
    if req.headers.get(header::CONTENT_TYPE).is_none() {
        let _ = req.insert_header(header::CONTENT_TYPE, "application/json");
    }
    smooth_upstream_client_headers(req, is_streaming);
}

/// Header field names present on the upstream request (for debug logging only).
pub fn upstream_header_names(req: &RequestHeader) -> Vec<String> {
    req.headers.keys().map(|k| k.as_str().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Version;
    use pingora_http::RequestHeader;

    fn sample_req() -> RequestHeader {
        let mut req = RequestHeader::build("POST", b"/v1/chat/completions", None).unwrap();
        req.set_version(Version::HTTP_11);
        req.insert_header(header::TRANSFER_ENCODING, "chunked")
            .unwrap();
        req.insert_header(header::CONTENT_LENGTH, "99999").unwrap();
        req.insert_header(header::CONTENT_ENCODING, "gzip").unwrap();
        req.insert_header(header::EXPECT, "100-continue").unwrap();
        req
    }

    #[test]
    fn strips_chunked_and_sets_content_length() {
        let mut req = sample_req();
        normalize_replaced_body_headers(&mut req, 100_652);
        assert!(req.headers.get(header::TRANSFER_ENCODING).is_none());
        assert_eq!(
            req.headers
                .get(header::CONTENT_LENGTH)
                .map(|v| v.to_str().unwrap()),
            Some("100652")
        );
        assert!(req.headers.get(header::CONTENT_ENCODING).is_none());
        assert!(req.headers.get(header::EXPECT).is_none());
        assert_eq!(
            req.headers
                .get(header::CONTENT_TYPE)
                .map(|v| v.to_str().unwrap()),
            Some("application/json")
        );
    }

    #[test]
    fn smooth_sets_curl_like_headers() {
        let mut req = sample_req();
        req.insert_header(header::USER_AGENT, "Cursor/1.0").unwrap();
        req.insert_header(header::ACCEPT_ENCODING, "gzip, deflate, br")
            .unwrap();
        smooth_upstream_client_headers(&mut req, true);
        assert_eq!(
            req.headers
                .get(header::USER_AGENT)
                .map(|v| v.to_str().unwrap()),
            Some(UPSTREAM_USER_AGENT)
        );
        assert_eq!(
            req.headers.get(header::ACCEPT).map(|v| v.to_str().unwrap()),
            Some("text/event-stream")
        );
        assert_eq!(
            req.headers
                .get(header::ACCEPT_ENCODING)
                .map(|v| v.to_str().unwrap()),
            Some("identity")
        );
    }

    #[test]
    fn smooth_non_streaming_keeps_gzip_accept_encoding() {
        let mut req = sample_req();
        smooth_upstream_client_headers(&mut req, false);
        assert_eq!(
            req.headers
                .get(header::ACCEPT_ENCODING)
                .map(|v| v.to_str().unwrap()),
            Some(UPSTREAM_ACCEPT_ENCODING)
        );
    }

    #[test]
    fn deferred_headers_omit_zero_content_length() {
        let mut req = sample_req();
        prepare_streaming_deferred_upstream_headers(&mut req, true);
        assert!(req.headers.get(header::CONTENT_LENGTH).is_none());
        assert!(req.headers.get(header::TRANSFER_ENCODING).is_none());
        assert_eq!(
            req.headers
                .get(header::ACCEPT_ENCODING)
                .map(|v| v.to_str().unwrap()),
            Some("identity")
        );
    }

    #[test]
    fn apply_request_gzip_encoding() {
        let mut req = sample_req();
        normalize_replaced_body_headers(&mut req, 128);
        apply_upstream_request_content_encoding(&mut req, "gzip");
        assert_eq!(
            req.headers
                .get(header::CONTENT_ENCODING)
                .map(|v| v.to_str().unwrap()),
            Some("gzip")
        );
    }

    #[test]
    fn preserves_existing_content_type() {
        let mut req = sample_req();
        req.insert_header(header::CONTENT_TYPE, "application/json; charset=utf-8")
            .unwrap();
        normalize_replaced_body_headers(&mut req, 42);
        assert_eq!(
            req.headers
                .get(header::CONTENT_TYPE)
                .map(|v| v.to_str().unwrap()),
            Some("application/json; charset=utf-8")
        );
    }
}
