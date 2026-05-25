//! Logic for injecting a prepared upstream body when Pingora's retry buffer overflows.

use bytes::Bytes;

/// Decide whether `request_body_filter` should emit the prepared body now.
pub fn should_emit_prepared_upstream_body(
    end_of_stream: bool,
    retry_buffer_truncated: bool,
) -> bool {
    end_of_stream || retry_buffer_truncated
}

/// Apply prepared body to the chunk Pingora will send upstream.
pub fn apply_prepared_upstream_body(
    prepared: Vec<u8>,
    downstream_chunk: &mut Option<Bytes>,
    end_of_stream: bool,
    retry_buffer_truncated: bool,
) -> Option<Vec<u8>> {
    if should_emit_prepared_upstream_body(end_of_stream, retry_buffer_truncated) {
        *downstream_chunk = Some(Bytes::from(prepared));
        None
    } else {
        *downstream_chunk = None;
        Some(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_on_end_of_stream() {
        let mut chunk = Some(Bytes::from_static(b"client"));
        let rest = apply_prepared_upstream_body(b"prep".to_vec(), &mut chunk, true, false);
        assert!(rest.is_none());
        assert_eq!(chunk.as_ref().unwrap().as_ref(), b"prep");
    }

    #[test]
    fn emits_when_retry_buffer_truncated() {
        let mut chunk = None;
        let rest = apply_prepared_upstream_body(b"big".to_vec(), &mut chunk, false, true);
        assert!(rest.is_none());
        assert_eq!(chunk.as_ref().unwrap().as_ref(), b"big");
    }

    #[test]
    fn holds_until_end_when_buffer_ok() {
        let mut chunk = Some(Bytes::from_static(b"client"));
        let rest = apply_prepared_upstream_body(b"prep".to_vec(), &mut chunk, false, false);
        assert_eq!(rest.as_deref(), Some(b"prep".as_slice()));
        assert!(chunk.is_none());
    }
}
