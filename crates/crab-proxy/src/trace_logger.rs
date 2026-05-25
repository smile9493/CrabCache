use crab_composition::{CompositionDebugEntry, RequestComposition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
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
                format!("{}...<truncated>", &raw[..max_payload_bytes])
            } else {
                raw.to_string()
            };
            Some(mask_snapshot_sensitive(&raw))
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
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_hit,
            cache_tier,
            retired_prefix_messages: None,
            reasoning_strategy: None,
            prompt_cache_hit_ratio: None,
            composition,
            request_messages_snapshot,
            response_preview: None,
            upstream_profile_id: None,
            pipeline: None,
            upstream_model: None,
            client_body_user_id: None,
            upstream_user_id: None,
            user_id_audit: None,
        }
    }
}

/// Light masking for sensitive patterns in snapshot text.
/// Currently masks `sk-` / `sk-cc-` bearer tokens (partial reveal of last 4 chars).
fn mask_snapshot_sensitive(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        // Look for "sk-" pattern
        if i + 2 < len && bytes[i] == b's' && bytes[i + 1] == b'k' && bytes[i + 2] == b'-' {
            // Find the end of the token (non-alphanumeric or end)
            let start = i;
            i += 3; // skip "sk-"
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
            {
                i += 1;
            }
            let token = &text[start..i];
            if token.len() > 8 {
                result.push_str(&token[..4]);
                result.push_str("...");
                result.push_str(&token[token.len() - 4..]);
            } else {
                result.push_str(&token[..1]);
                result.push_str("***");
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
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

#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub enabled: bool,
    pub path: String,
    pub max_lines: usize,
    pub max_files: usize,
    pub composition_debug: Option<CompositionDebugConfig>,
    /// Max bytes to capture for `request_messages_snapshot`. `0` = disabled (default).
    pub max_payload_bytes: usize,
    /// Max bytes to capture for `response_preview`. `0` = disabled (default).
    pub max_response_preview_bytes: usize,
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
        }
    }
}

// ── LogWriter for SanitizedLogEntry ──────────────────────────────────

struct LogWriter {
    file: File,
    path: PathBuf,
    max_lines: usize,
    line_count: usize,
    max_files: usize,
}

impl LogWriter {
    fn new(config: &TraceConfig) -> std::io::Result<Self> {
        let path = PathBuf::from(&config.path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            file,
            path,
            max_lines: config.max_lines,
            line_count: 0,
            max_files: config.max_files,
        })
    }

    fn write_entry(&mut self, entry: &SanitizedLogEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)? + "\n";
        self.file.write_all(line.as_bytes())?;
        self.line_count += 1;

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

        while log_files.len() >= self.max_files {
            let oldest = log_files.remove(0);
            std::fs::remove_file(oldest)?;
        }

        Ok(())
    }
}

// ── DebugLogWriter for CompositionDebugEntry ─────────────────────────

struct DebugLogWriter {
    file: File,
    path: PathBuf,
    max_lines: usize,
    line_count: usize,
    max_files: usize,
}

impl DebugLogWriter {
    fn new(config: &CompositionDebugConfig) -> std::io::Result<Self> {
        let path = PathBuf::from(&config.path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            file,
            path,
            max_lines: config.max_lines,
            line_count: 0,
            max_files: config.max_files,
        })
    }

    fn write_entry(&mut self, entry: &CompositionDebugEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)? + "\n";
        self.file.write_all(line.as_bytes())?;
        self.line_count += 1;

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

        while log_files.len() >= self.max_files {
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

// ── TraceLogger (main trace + optional debug) ────────────────────────

pub struct TraceLogger {
    sender: mpsc::Sender<SanitizedLogEntry>,
    debug_sender: Option<mpsc::Sender<CompositionDebugEntry>>,
    max_payload_bytes: usize,
    max_response_preview_bytes: usize,
}

impl TraceLogger {
    pub fn init(config: TraceConfig) -> Self {
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
                        let mut writer = match DebugLogWriter::new(&debug_config_clone) {
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

        std::thread::Builder::new()
            .name("crab-trace-writer".into())
            .spawn(move || {
                let mut writer = match LogWriter::new(&config) {
                    Ok(w) => w,
                    Err(e) => {
                        warn!("Failed to initialize trace logger: {}", e);
                        return;
                    }
                };

                while let Ok(entry) = rx.recv() {
                    if let Err(e) = writer.write_entry(&entry) {
                        warn!("Shadow log write failed: {}", e);
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
}
