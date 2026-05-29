//! Optional gzip compression for upstream request bodies.

use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;

/// Compress `body` when `enabled` and `body.len() >= min_bytes`. Returns (bytes, was_compressed).
pub fn maybe_gzip_request_body(body: &[u8], enabled: bool, min_bytes: usize) -> (Vec<u8>, bool) {
    if !enabled || body.len() < min_bytes {
        return (body.to_vec(), false);
    }
    let mut enc = GzEncoder::new(Vec::with_capacity(body.len() / 3), Compression::fast());
    match enc.write_all(body) {
        Ok(()) => match enc.finish() {
            Ok(compressed) if compressed.len() < body.len() => (compressed, true),
            Ok(compressed) => (compressed, true),
            Err(_) => (body.to_vec(), false),
        },
        Err(_) => (body.to_vec(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_small_body() {
        let body = b"{}";
        let (out, gz) = maybe_gzip_request_body(body, true, 4096);
        assert!(!gz);
        assert_eq!(out, body);
    }

    #[test]
    fn compresses_large_body() {
        let body = vec![b'x'; 5000];
        let (out, gz) = maybe_gzip_request_body(&body, true, 4096);
        assert!(gz);
        assert!(out.len() < body.len());
    }
}
