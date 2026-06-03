//! Bounded streaming capture with tail retention.
//!
//! Limits total accumulated bytes to `max_bytes`. When exceeded, only the last
//! `tail_capacity` bytes are retained as a "tail buffer" for response.completed
//! synthesis, usage fallback, and log preview.

const DEFAULT_TAIL_CAPACITY: usize = 64 * 1024; // 64 KiB

/// Lightweight streaming capture with memory boundary.
///
/// Limits total accumulated bytes to `max_bytes`. When exceeded:
/// - Stops accepting new data (silently drops)
/// - Retains the last `tail_capacity` (default 64 KiB) of data as a "tail buffer"
///   for response.completed synthesis, usage fallback, and log preview.
pub struct StreamCapture {
    /// The retained data buffer.
    buffer: Vec<u8>,
    /// Total bytes seen (including dropped).
    total_seen: usize,
    /// Maximum total bytes to retain.
    max_bytes: usize,
    /// Maximum tail bytes to keep when over limit.
    tail_capacity: usize,
    /// Whether the capture has exceeded its limit.
    over_limit: bool,
}

impl StreamCapture {
    /// Create a new `StreamCapture` with the given `max_bytes` limit and default
    /// 64 KiB tail capacity.
    pub fn new(max_bytes: usize) -> Self {
        Self::with_tail_capacity(max_bytes, DEFAULT_TAIL_CAPACITY)
    }

    /// Create a new `StreamCapture` with the given `max_bytes` limit and custom
    /// `tail_capacity`.
    pub fn with_tail_capacity(max_bytes: usize, tail_capacity: usize) -> Self {
        Self {
            buffer: Vec::new(),
            total_seen: 0,
            max_bytes,
            tail_capacity,
            over_limit: false,
        }
    }

    /// Append data with memory boundary enforcement.
    ///
    /// - Under limit: appends fully.
    /// - Crossing the limit: sets `over_limit`, retains only the last `tail_capacity` bytes.
    /// - Already over limit: appends and trims to `tail_capacity`.
    /// - Always increments `total_seen`.
    pub fn extend_from_slice(&mut self, data: &[u8]) {
        self.total_seen += data.len();

        // max_bytes == 0 means no limit (unbounded, matching original Vec<u8> behavior).
        if self.max_bytes == 0 {
            self.buffer.extend_from_slice(data);
            return;
        }

        if self.over_limit {
            self.buffer.extend_from_slice(data);
            self.trim_to_tail();
            return;
        }

        if self.buffer.len() + data.len() <= self.max_bytes {
            // Still under limit: append fully.
            self.buffer.extend_from_slice(data);
        } else {
            // Crossing the limit.
            self.over_limit = true;
            self.buffer.extend_from_slice(data);
            self.trim_to_tail();
        }
    }

    /// Whether the capture has exceeded its memory limit.
    pub fn is_over_limit(&self) -> bool {
        self.over_limit
    }

    /// Current buffer length (may be less than `total_seen` when over limit).
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Total bytes seen (including dropped).
    pub fn total_seen(&self) -> usize {
        self.total_seen
    }

    /// Take the internal buffer, replacing it with an empty Vec.
    /// Resets `total_seen` and `over_limit` as well.
    pub fn take(&mut self) -> Vec<u8> {
        self.total_seen = 0;
        self.over_limit = false;
        std::mem::take(&mut self.buffer)
    }

    /// Access the buffer contents as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.buffer
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Reconfigure the memory limit after construction.
    ///
    /// If the current buffer already exceeds the new limit, it is trimmed
    /// to `tail_capacity` and `over_limit` is set.
    pub fn reconfigure(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes;
        if self.buffer.len() > self.max_bytes && !self.over_limit {
            self.over_limit = true;
        }
        if self.over_limit {
            self.trim_to_tail();
        }
    }

    /// Return the configured `max_bytes` limit (0 means unbounded).
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Trim the buffer to `tail_capacity` bytes (keeping the tail).
    fn trim_to_tail(&mut self) {
        if self.buffer.len() > self.tail_capacity {
            let drain_count = self.buffer.len() - self.tail_capacity;
            self.buffer.drain(0..drain_count);
        }
    }
}

impl Default for StreamCapture {
    /// Default: no limit (max_bytes = 0 means unbounded, matching original Vec<u8> behavior).
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            total_seen: 0,
            max_bytes: 0, // 0 = no limit
            tail_capacity: DEFAULT_TAIL_CAPACITY,
            over_limit: false,
        }
    }
}

/// Support `&StreamCapture` as `&[u8]` via `AsRef<[u8]>`.
impl AsRef<[u8]> for StreamCapture {
    fn as_ref(&self) -> &[u8] {
        &self.buffer
    }
}

/// Support `&StreamCapture` auto-coercing to `&[u8]` for read contexts.
///
/// **Note:** `DerefMut` is intentionally NOT implemented to prevent bypassing
/// the memory-bound enforcement in `extend_from_slice`.
impl std::ops::Deref for StreamCapture {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buffer
    }
}

/// Support `std::io::Read` for the buffer contents.
impl std::io::Read for StreamCapture {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::Read as _;
        self.buffer.as_slice().read(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_limit_appends_fully() {
        let mut cap = StreamCapture::new(100);
        cap.extend_from_slice(b"hello");
        assert_eq!(cap.as_slice(), b"hello");
        assert_eq!(cap.len(), 5);
        assert_eq!(cap.total_seen(), 5);
        assert!(!cap.is_over_limit());
    }

    #[test]
    fn crossing_limit_retains_tail() {
        let mut cap = StreamCapture::with_tail_capacity(10, 4);
        // First write: within limit
        cap.extend_from_slice(b"0123456789"); // 10 bytes, exactly at limit
        assert!(!cap.is_over_limit());
        assert_eq!(cap.len(), 10);

        // Second write: crosses limit
        cap.extend_from_slice(b"AB"); // 12 total, over 10 limit
        assert!(cap.is_over_limit());
        // Should keep last 4 bytes: "89AB"
        assert_eq!(cap.as_slice(), b"89AB");
        assert_eq!(cap.total_seen(), 12);
    }

    #[test]
    fn already_over_limit_keeps_tail() {
        let mut cap = StreamCapture::with_tail_capacity(5, 3);
        cap.extend_from_slice(b"0123456789"); // 10 bytes, over 5 limit
        assert!(cap.is_over_limit());
        // Tail of 3: "789"
        assert_eq!(cap.as_slice(), b"789");

        cap.extend_from_slice(b"AB"); // more data
        assert_eq!(cap.as_slice(), b"9AB");
        assert_eq!(cap.total_seen(), 12);
    }

    #[test]
    fn take_returns_buffer_and_resets() {
        let mut cap = StreamCapture::new(100);
        cap.extend_from_slice(b"hello");
        let buf = cap.take();
        assert_eq!(buf, b"hello");
        assert!(cap.is_empty());
        assert_eq!(cap.total_seen(), 0);
        assert!(!cap.is_over_limit());
    }

    #[test]
    fn default_no_limit() {
        let mut cap = StreamCapture::default();
        // Write a lot of data — should not be limited
        let data = vec![0u8; 1_000_000];
        cap.extend_from_slice(&data);
        assert!(!cap.is_over_limit());
        assert_eq!(cap.len(), 1_000_000);
    }

    #[test]
    fn reconfigure_applies_new_limit() {
        let mut cap = StreamCapture::default(); // no limit
        cap.extend_from_slice(b"0123456789"); // 10 bytes
        assert!(!cap.is_over_limit());

        cap.reconfigure(5); // new limit is 5
        assert!(cap.is_over_limit());
        // tail_capacity is default 64 KiB, so all 10 bytes are retained
        assert_eq!(cap.len(), 10);
    }

    #[test]
    fn reconfigure_with_small_tail() {
        let mut cap = StreamCapture::default(); // no limit
        cap.extend_from_slice(b"0123456789"); // 10 bytes

        let mut cap2 = StreamCapture::with_tail_capacity(5, 3);
        cap2.buffer = cap.take();
        cap2.total_seen = 10;
        cap2.reconfigure(5);
        assert!(cap2.is_over_limit());
        assert_eq!(cap2.as_slice(), b"789");
    }

    #[test]
    fn zero_max_bytes_means_no_limit() {
        let mut cap = StreamCapture::new(0);
        let data = vec![0u8; 1_000_000];
        cap.extend_from_slice(&data);
        assert!(!cap.is_over_limit());
        assert_eq!(cap.len(), 1_000_000);
    }
}
