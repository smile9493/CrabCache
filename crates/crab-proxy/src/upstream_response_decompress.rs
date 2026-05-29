//! Decompress upstream response bodies when `Content-Encoding` is set (R7 fallback).
//!
//! Pingora may or may not strip compression before `upstream_response_body_filter`;
//! this module pass-throughs plaintext and decompresses gzip/deflate/br when needed.

use bytes::Bytes;
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use std::io::Read;
use tracing::warn;

/// Parsed `Content-Encoding` token (first supported codec wins).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseContentEncoding {
    Identity,
    Gzip,
    Deflate,
    Br,
}

/// Incremental buffer for gzip/deflate/br bodies (decompress at EOS or when complete).
#[derive(Default)]
pub struct UpstreamDecompressState {
    pub encoding: Option<ResponseContentEncoding>,
    compressed: Vec<u8>,
    /// Plaintext emitted incrementally for streaming paths.
    pub plaintext_pending: Vec<u8>,
    finished: bool,
}

impl UpstreamDecompressState {
    pub fn reset(&mut self) {
        self.encoding = None;
        self.compressed.clear();
        self.plaintext_pending.clear();
        self.finished = false;
    }

    pub fn push_compressed(&mut self, chunk: &[u8]) {
        self.compressed.extend_from_slice(chunk);
    }

    /// Decompress full buffered body; returns plaintext or original bytes on failure.
    pub fn finish(&mut self) -> Result<Vec<u8>, DecompressError> {
        if self.finished {
            return Ok(std::mem::take(&mut self.plaintext_pending));
        }
        self.finished = true;
        let enc = self.encoding.unwrap_or(ResponseContentEncoding::Identity);
        if enc == ResponseContentEncoding::Identity || self.compressed.is_empty() {
            return Ok(std::mem::take(&mut self.compressed));
        }
        match decompress_bytes(enc, &self.compressed) {
            Ok(v) => {
                self.compressed.clear();
                Ok(v)
            }
            Err(e) => {
                warn!(
                    encoding = ?enc,
                    compressed_len = self.compressed.len(),
                    error = %e,
                    "upstream response decompress failed; passing compressed bytes through"
                );
                Ok(std::mem::take(&mut self.compressed))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecompressError {
    #[error("unsupported content encoding")]
    Unsupported,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Parse `Content-Encoding` header value (may list multiple codecs).
pub fn parse_content_encoding(value: &str) -> ResponseContentEncoding {
    for part in value.split(',') {
        let token = part.trim().to_ascii_lowercase();
        if token.contains("gzip") {
            return ResponseContentEncoding::Gzip;
        }
        if token == "deflate" || token.contains("deflate") {
            return ResponseContentEncoding::Deflate;
        }
        if token == "br" || token.contains("br") {
            return ResponseContentEncoding::Br;
        }
        if token == "identity" {
            return ResponseContentEncoding::Identity;
        }
    }
    ResponseContentEncoding::Identity
}

/// True when body looks like gzip despite missing/wrong header (JSON/SSE should not start with 0x1f8b).
pub fn looks_gzip_magic(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
}

pub fn decompress_bytes(
    encoding: ResponseContentEncoding,
    data: &[u8],
) -> Result<Vec<u8>, DecompressError> {
    match encoding {
        ResponseContentEncoding::Identity => Ok(data.to_vec()),
        ResponseContentEncoding::Gzip => gzip_decode(data),
        ResponseContentEncoding::Deflate => deflate_decode(data),
        ResponseContentEncoding::Br => brotli_decode(data),
    }
}

/// Decompress a single upstream body chunk for `upstream_response_body_filter`.
pub fn decompress_upstream_chunk(
    state: &mut UpstreamDecompressState,
    chunk: Bytes,
    end_of_stream: bool,
) -> Result<Bytes, DecompressError> {
    let enc = state.encoding.unwrap_or(ResponseContentEncoding::Identity);
    if enc == ResponseContentEncoding::Identity && !looks_gzip_magic(chunk.as_ref()) {
        return Ok(chunk);
    }
    if enc == ResponseContentEncoding::Identity && looks_gzip_magic(chunk.as_ref()) {
        state.encoding = Some(ResponseContentEncoding::Gzip);
    }
    state.push_compressed(chunk.as_ref());
    if !end_of_stream {
        // SSE/JSON gzip is typically one stream; wait for EOS unless chunk is tiny identity passthrough.
        return Ok(Bytes::new());
    }
    let plain = state.finish()?;
    Ok(Bytes::from(plain))
}

fn gzip_decode(data: &[u8]) -> Result<Vec<u8>, DecompressError> {
    let mut decoder = GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

fn deflate_decode(data: &[u8]) -> Result<Vec<u8>, DecompressError> {
    let mut decoder = DeflateDecoder::new(data);
    let mut out = Vec::new();
    if decoder.read_to_end(&mut out).is_ok() && !out.is_empty() {
        return Ok(out);
    }
    let mut zlib = ZlibDecoder::new(data);
    out.clear();
    zlib.read_to_end(&mut out)?;
    Ok(out)
}

fn brotli_decode(data: &[u8]) -> Result<Vec<u8>, DecompressError> {
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut std::io::Cursor::new(data), &mut out)
        .map_err(|_| DecompressError::Unsupported)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    #[test]
    fn parse_gzip_encoding() {
        assert_eq!(
            parse_content_encoding("gzip, deflate"),
            ResponseContentEncoding::Gzip
        );
    }

    #[test]
    fn gzip_roundtrip() {
        let raw = br#"{"choices":[{"delta":{"content":"hi"}}]}"#;
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        let gz = enc.finish().unwrap();
        let out = decompress_bytes(ResponseContentEncoding::Gzip, &gz).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn incremental_finish_at_eos() {
        let raw = b"data: {}\n\n";
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(raw).unwrap();
        let gz = enc.finish().unwrap();
        let mut state = UpstreamDecompressState {
            encoding: Some(ResponseContentEncoding::Gzip),
            ..Default::default()
        };
        let out = decompress_upstream_chunk(&mut state, Bytes::from(gz), true).unwrap();
        assert_eq!(out.as_ref(), raw);
    }
}
