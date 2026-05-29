use crab_composition::{CompositionDebugEntry, RequestComposition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedLogEntry {
    pub timestamp_ms: u64,
    pub request_hash: String,
    pub content_length: usize,
    pub semantic_cluster: u32,
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub model: String,
    pub prompt_tokens: usize,
    /// End-to-end latency (client request start → logging).
    pub latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
    /// Request start → upstream response headers (MiMo prefill SLO).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefill_ms: Option<f64>,
    /// Legacy: `latency_ms - upstream_latency_ms` (first-byte window before stream segment ends).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_header_ms: Option<f64>,
    /// Response headers → first upstream body chunk (SSE TTFT).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retired_prefix_messages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_hit_ratio: Option<f64>,
    #[serde(default)]
    pub streaming_defer: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming_defer_reject_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_session_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_outbound_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<RequestComposition>,
    /// Truncated + sanitized request body (UTF-8 lossy). Only populated when
    /// `max_payload_bytes > 0` in TraceConfig.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_messages_snapshot: Option<String>,
    /// Truncated response preview (non-streaming accumulated body / streaming
    /// accumulated SSE body / cache-hit response body). Only populated when
    /// `max_response_preview_bytes > 0` in TraceConfig.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_preview: Option<String>,
    /// Selected upstream profile for this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_profile_id: Option<String>,
    /// Upstream key pool key_id used for this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_key_id: Option<String>,
    /// Request pipeline id (e.g. `cursor_deepseek_v4`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    /// Model name sent upstream after prepare.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    /// `user_id` from the client request body before gateway overwrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_body_user_id: Option<String>,
    /// `user_id` in the serialized upstream request body (authoritative).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_user_id: Option<String>,
    /// DeepSeek isolation audit: `injected` | `absent` | `stripped_client` | `mismatch` | `not_applicable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id_audit: Option<String>,
    /// Ketama affinity key used for backend selection (e.g. `conv:{uuid}`, `ip:{hash}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_key: Option<String>,
    /// Categorized affinity source: `conv` | `pck` | `user` | `ip` | `unknown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_kind: Option<String>,
    /// Selected upstream backend node name (for circuit-breaker tracking).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_name: Option<String>,
    /// Session fingerprint derived from the first user message (SHA-256 prefix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_fingerprint: Option<String>,
    /// Whether this request was a coalesced follower (waiting on a leader).
    #[serde(default)]
    pub is_coalesced: bool,
    /// Client API key ID (not the consumer name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_id: Option<String>,
}

impl SanitizedLogEntry {
    /// Build a sanitised log entry from a request body.
    ///
    /// When `max_payload_bytes > 0`, a truncated + sanitised UTF-8 copy of the
    /// request body is stored in `request_messages_snapshot`.  Empty trailing
    /// whitespace and `sk-` / `sk-cc-` bearer tokens are lightly masked.
    pub fn from_request(
        body: &[u8],
        conversation_id: Option<String>,
        consumer: Option<String>,
        domain: Option<String>,
        project_id: Option<String>,
        model: &str,
        prompt_tokens: usize,
        latency_ms: f64,
        cache_hit: bool,
        cache_tier: Option<String>,
        composition: Option<RequestComposition>,
        max_payload_bytes: usize,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(body);
        let full_hash = hex::encode(hasher.finalize());

        let request_hash = if full_hash.len() >= 16 {
            full_hash[..16].to_string()
        } else {
            full_hash.clone()
        };

        let semantic_cluster = if full_hash.len() >= 8 {
            u32::from_str_radix(&full_hash[..8], 16).unwrap_or(0) % 100
        } else {
            0
        };

        let request_messages_snapshot = if max_payload_bytes > 0 && !body.is_empty() {
            let raw = String::from_utf8_lossy(body);
            let raw = if raw.len() > max_payload_bytes {
                let safe_end = raw.floor_char_boundary(max_payload_bytes);
                format!("{}...<truncated>", &raw[..safe_end])
            } else {
                raw.to_string()
            };
            Some(crate::masking::mask_api_keys(&raw))
        } else {
            None
        };

        Self {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            request_hash,
            content_length: body.len(),
            semantic_cluster,
            conversation_id,
            consumer,
            domain,
            project_id,
            model: model.to_string(),
            prompt_tokens,
            latency_ms,
            upstream_latency_ms: None,
            prefill_ms: None,
            pre_header_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_hit,
            cache_tier,
            retired_prefix_messages: None,
            reasoning_strategy: None,
            prompt_cache_hit_ratio: None,
            streaming_defer: false,
            streaming_defer_reject_reason: None,
            session_store: None,
            stable_session_kind: None,
            upstream_outbound_bytes: None,
            composition,
            request_messages_snapshot,
            response_preview: None,
            upstream_profile_id: None,
            upstream_key_id: None,
            pipeline: None,
            upstream_model: None,
            client_body_user_id: None,
            upstream_user_id: None,
            user_id_audit: None,
            affinity_key: None,
            affinity_kind: None,
            backend_name: None,
            session_fingerprint: None,
            is_coalesced: false,
            client_key_id: None,
        }
    }
}

/// Configuration for the debug composition JSONL log file.
/// When enabled, stores full (unhashed) system message text and tools definitions
/// for composition analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionDebugConfig {
    pub enabled: bool,
    pub path: String,
    pub max_lines: usize,
    pub max_files: usize,
}

impl Default for CompositionDebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/var/log/crabcache/trace-debug.jsonl".to_string(),
            max_lines: 5000,
            max_files: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    pub enabled: bool,
    pub path: String,
    #[serde(default = "default_max_lines")]
    pub max_lines: usize,
    #[serde(default = "default_max_files")]
    pub max_files: usize,
    #[serde(default)]
    pub composition_debug: Option<CompositionDebugConfig>,
    /// Max bytes to capture for `request_messages_snapshot`. `0` = disabled (default).
    #[serde(default)]
    pub max_payload_bytes: usize,
    /// Max bytes to capture for `response_preview`. `0` = disabled (default).
    #[serde(default)]
    pub max_response_preview_bytes: usize,
    /// Optional PostgreSQL URL for trace log persistence. When set, trace
    /// entries are written to both JSONL (if enabled) and PG.
    #[serde(default)]
    pub pg_url: Option<String>,
}

fn default_max_lines() -> usize {
    10000
}

fn default_max_files() -> usize {
    5
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/var/log/crabcache/trace.jsonl".to_string(),
            max_lines: 10000,
            max_files: 5,
            composition_debug: None,
            max_payload_bytes: 0,
            max_response_preview_bytes: 0,
            pg_url: None,
        }
    }
}

// ── RotatingJsonlWriter (shared by trace + debug log) ────────────────

/// Flush interval: flush to disk every N lines to bound data loss on crash.
const FLUSH_INTERVAL_LINES: usize = 100;

struct RotatingJsonlWriter {
    file: File,
    path: PathBuf,
    max_lines: usize,
    line_count: usize,
    flush_counter: usize,
    max_files: usize,
}

impl RotatingJsonlWriter {
    fn new(path: PathBuf, max_lines: usize, max_files: usize) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let line_count = Self::count_existing_lines(&path);
        Ok(Self {
            file,
            path,
            max_lines,
            line_count,
            flush_counter: 0,
            max_files,
        })
    }

    /// Estimate existing line count so rotation respects `max_lines` after restart.
    fn count_existing_lines(path: &PathBuf) -> usize {
        std::fs::metadata(path)
            .map(|m| {
                let len = m.len() as usize;
                if len == 0 {
                    return 0;
                }
                let read_size = len.min(65_536);
                let start = len.saturating_sub(read_size);
                let Ok(mut file) = File::open(path) else {
                    return 0;
                };
                if file.seek(SeekFrom::Start(start as u64)).is_err() {
                    return 0;
                }
                let mut buf = vec![0u8; read_size];
                if file.read_exact(&mut buf).is_err() {
                    return 0;
                }
                let newlines = buf.iter().filter(|&&b| b == b'\n').count();
                if read_size < len {
                    (newlines as f64 * len as f64 / read_size as f64) as usize
                } else {
                    newlines
                }
            })
            .unwrap_or(0)
    }

    fn write_entry<T: Serialize>(&mut self, entry: &T) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)? + "\n";
        self.file.write_all(line.as_bytes())?;
        self.line_count += 1;
        self.flush_counter += 1;

        if self.flush_counter >= FLUSH_INTERVAL_LINES {
            self.file.flush()?;
            self.flush_counter = 0;
        }
        if self.line_count >= self.max_lines {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.sync_all()?;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let rotated = self.path.with_file_name(format!(
            "{}.{}",
            self.path
                .file_name()
                .expect("path has file_name — validated at init")
                .to_str()
                .expect("file_name is valid UTF-8"),
            timestamp
        ));

        std::fs::rename(&self.path, &rotated)?;

        self.cleanup_old_files()?;

        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.line_count = 0;
        self.flush_counter = 0;
        Ok(())
    }

    fn cleanup_old_files(&mut self) -> std::io::Result<()> {
        let parent = self
            .path
            .parent()
            .expect("path has parent — validated at init");
        let file_name = self
            .path
            .file_name()
            .expect("path has file_name — validated at init")
            .to_str()
            .expect("file_name is valid UTF-8");

        let mut log_files: Vec<PathBuf> = std::fs::read_dir(parent)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|name| name.starts_with(file_name) && name != file_name)
                    .unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();

        log_files.sort();

        while log_files.len() > self.max_files {
            let oldest = log_files.remove(0);
            std::fs::remove_file(oldest)?;
        }

        Ok(())
    }
}

// ── Global debug sender OnceLock ─────────────────────────────────────

static COMPOSITION_DEBUG_TX: OnceLock<Option<mpsc::Sender<CompositionDebugEntry>>> =
    OnceLock::new();

/// Set the composition debug sender (called once at startup).
pub fn set_composition_debug_tx(tx: Option<mpsc::Sender<CompositionDebugEntry>>) {
    let _ = COMPOSITION_DEBUG_TX.set(tx);
}

/// Get a clone of the composition debug sender, if one was set.
pub fn composition_debug_tx() -> Option<mpsc::Sender<CompositionDebugEntry>> {
    COMPOSITION_DEBUG_TX.get().cloned().unwrap_or(None)
}

// ── TraceLogger (main trace + optional debug + optional PG sink) ─────

pub struct TraceLogger {
    sender: mpsc::Sender<SanitizedLogEntry>,
    debug_sender: Option<mpsc::Sender<CompositionDebugEntry>>,
    max_payload_bytes: usize,
    max_response_preview_bytes: usize,
}

impl TraceLogger {
    /// Initialize the trace logger.
    ///
    /// `pg_sink`: optional external sender for PG batch insertion.
    /// When provided, entries are forwarded to this sender after JSONL write.
    /// The gateway creates the PG writer task and passes its sender here.
    pub fn init(config: TraceConfig, pg_sink: Option<mpsc::SyncSender<SanitizedLogEntry>>) -> Self {
        let max_payload_bytes = config.max_payload_bytes;
        let max_response_preview_bytes = config.max_response_preview_bytes;
        let (tx, rx) = mpsc::channel::<SanitizedLogEntry>();

        let debug_sender = if let Some(ref debug_config) = config.composition_debug {
            if debug_config.enabled {
                let (debug_tx, debug_rx) = mpsc::channel::<CompositionDebugEntry>();
                let debug_config_clone = debug_config.clone();

                std::thread::Builder::new()
                    .name("crab-debug-writer".into())
                    .spawn(move || {
                        let mut writer = match RotatingJsonlWriter::new(
                            PathBuf::from(&debug_config_clone.path),
                            debug_config_clone.max_lines,
                            debug_config_clone.max_files,
                        ) {
                            Ok(w) => w,
                            Err(e) => {
                                warn!("Failed to initialize composition debug logger: {}", e);
                                return;
                            }
                        };

                        while let Ok(entry) = debug_rx.recv() {
                            if let Err(e) = writer.write_entry(&entry) {
                                warn!("Composition debug log write failed: {}", e);
                            }
                        }
                    })
                    .expect("Failed to spawn debug logger thread");

                // Set the global static for proxy.rs access
                set_composition_debug_tx(Some(debug_tx.clone()));
                Some(debug_tx)
            } else {
                set_composition_debug_tx(None);
                None
            }
        } else {
            set_composition_debug_tx(None);
            None
        };

        // Spawn writer thread. When pg_sink is provided, entries are forwarded
        // to the PG writer after JSONL write.
        std::thread::Builder::new()
            .name("crab-trace-writer".into())
            .spawn(move || {
                let mut jsonl_writer = match RotatingJsonlWriter::new(
                    PathBuf::from(&config.path),
                    config.max_lines,
                    config.max_files,
                ) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        warn!("Failed to initialize trace logger: {}", e);
                        None
                    }
                };

                while let Ok(entry) = rx.recv() {
                    if let Some(ref mut w) = jsonl_writer {
                        if let Err(e) = w.write_entry(&entry) {
                            warn!("Shadow log write failed: {}", e);
                        }
                    }
                    if let Some(ref pg_tx) = pg_sink {
                        match pg_tx.try_send(entry) {
                            Ok(()) => {}
                            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                                static PG_DROP_COUNT: std::sync::atomic::AtomicU64 =
                                    std::sync::atomic::AtomicU64::new(0);
                                let count = PG_DROP_COUNT
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                if count.is_multiple_of(1000) {
                                    warn!(
                                        dropped = count,
                                        "PG trace sink buffer full, entries dropped"
                                    );
                                }
                            }
                            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                                static PG_DISCONNECTED: std::sync::atomic::AtomicBool =
                                    std::sync::atomic::AtomicBool::new(false);
                                if !PG_DISCONNECTED.swap(true, std::sync::atomic::Ordering::Relaxed)
                                {
                                    warn!("PG trace sink disconnected, entries will be lost");
                                }
                            }
                        }
                    }
                }
            })
            .expect("Failed to spawn trace logger thread");

        Self {
            sender: tx,
            debug_sender,
            max_payload_bytes,
            max_response_preview_bytes,
        }
    }

    pub fn log(&self, entry: SanitizedLogEntry) {
        let _ = self.sender.send(entry);
    }

    /// Maximum request payload bytes configured for the snapshot field.
    pub fn max_payload_bytes(&self) -> usize {
        self.max_payload_bytes
    }

    /// Maximum response preview bytes configured.
    pub fn max_response_preview_bytes(&self) -> usize {
        self.max_response_preview_bytes
    }

    /// Returns a clone of the debug sender for use by proxy.rs via the global static.
    pub fn debug_sender(&self) -> Option<mpsc::Sender<CompositionDebugEntry>> {
        self.debug_sender.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitized_entry_creation() {
        let body = b"test request body";
        let entry = SanitizedLogEntry::from_request(
            body,
            Some("conv-123".to_string()),
            Some("test-consumer".to_string()),
            Some("backend-team".to_string()),
            Some("proj-a".to_string()),
            "deepseek-chat",
            100,
            150.5,
            false,
            None,
            None,
            0,
        );

        assert_eq!(entry.content_length, 17);
        assert_eq!(entry.model, "deepseek-chat");
        assert_eq!(entry.prompt_tokens, 100);
        assert_eq!(entry.latency_ms, 150.5);
        assert!(!entry.cache_hit);
        assert!(entry.request_hash.len() <= 16);
        assert!(entry.semantic_cluster < 100);
    }

    #[test]
    fn test_hash_consistency() {
        let body = b"identical request";
        let entry1 = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None, 0,
        );
        let entry2 = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None, 0,
        );

        assert_eq!(entry1.request_hash, entry2.request_hash);
        assert_eq!(entry1.semantic_cluster, entry2.semantic_cluster);
    }

    #[test]
    fn test_composition_debug_config_default() {
        let cfg = CompositionDebugConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.path, "/var/log/crabcache/trace-debug.jsonl");
        assert_eq!(cfg.max_lines, 5000);
        assert_eq!(cfg.max_files, 3);
    }

    #[test]
    fn test_snapshot_disabled_by_default() {
        let body = b"test request body";
        let entry = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None,
            0, // max_payload_bytes = 0 → no snapshot
        );
        assert!(entry.request_messages_snapshot.is_none());
        assert!(entry.response_preview.is_none());
    }

    #[test]
    fn test_snapshot_with_payload() {
        let body = b"Hello, this is a test request body";
        let entry = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None,
            100, // max_payload_bytes = 100
        );
        let snap = entry
            .request_messages_snapshot
            .expect("snapshot should be present");
        assert!(snap.contains("Hello"));
        assert!(snap.len() <= 100 + 15); // allow "...<truncated>" suffix
    }

    #[test]
    fn test_snapshot_truncation() {
        let body = vec![b'A'; 200];
        let entry = SanitizedLogEntry::from_request(
            &body, None, None, None, None, "model", 0, 0.0, false, None, None,
            50, // truncate to 50 bytes
        );
        let snap = entry
            .request_messages_snapshot
            .expect("snapshot should be present");
        assert!(
            snap.contains("<truncated>"),
            "should indicate truncation: {snap}"
        );
        // Original AAAA... should be truncated
        assert!(snap.len() < 120, "snapshot too long: {}", snap.len());
    }

    #[test]
    fn test_snapshot_sk_masking() {
        let body = b"api_key=sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2";
        let entry = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None, 200,
        );
        let snap = entry
            .request_messages_snapshot
            .expect("snapshot should be present");
        assert!(
            !snap.contains("sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2"),
            "key should be masked: {snap}"
        );
        assert!(snap.contains("sk-c"), "partial reveal expected: {snap}");
    }

    #[test]
    fn test_hash_consistent_with_snapshot() {
        let body = b"test body for hash consistency";
        // Same body, same hash regardless of max_payload_bytes
        let entry1 = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None, 0,
        );
        let entry2 = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None, 50,
        );
        assert_eq!(entry1.request_hash, entry2.request_hash);
        assert!(entry2.request_messages_snapshot.is_some());
        assert_eq!(entry1.content_length, entry2.content_length);
    }

    #[test]
    fn test_snapshot_utf8_safe_truncation() {
        let body = "你好世界你好世界".as_bytes();
        let entry = SanitizedLogEntry::from_request(
            body, None, None, None, None, "model", 0, 0.0, false, None, None,
            7, // truncate inside a multi-byte character
        );
        let snap = entry
            .request_messages_snapshot
            .expect("snapshot should be present");
        assert!(
            !snap.contains('\u{FFFD}'),
            "truncation must not split UTF-8: {snap}"
        );
        assert!(snap.contains("<truncated>"));
    }

    #[test]
    fn test_writer_respects_existing_line_count() {
        let dir = std::env::temp_dir().join(format!("crab_rot_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trace.jsonl");

        for i in 0..3 {
            let line = format!(
                r#"{{"timestamp_ms":{i},"request_hash":"h{i}","content_length":1,"semantic_cluster":0,"model":"m","prompt_tokens":1,"latency_ms":1.0,"cache_hit":false}}"#
            );
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(format!("{line}\n").as_bytes())
                .unwrap();
        }

        let logger = TraceLogger::init(
            TraceConfig {
                enabled: true,
                path: path.to_string_lossy().into_owned(),
                max_lines: 5,
                max_files: 3,
                ..Default::default()
            },
            None,
        );

        for _ in 0..3 {
            logger.log(SanitizedLogEntry::from_request(
                b"body", None, None, None, None, "m", 1, 1.0, false, None, None, 0,
            ));
        }

        std::thread::sleep(std::time::Duration::from_millis(300));

        let rotated = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("trace.jsonl.")
            });
        assert!(rotated, "expected rotation when existing lines + new writes reach max_lines");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
