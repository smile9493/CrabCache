use crab_capture::{
    CaptureRequestMeta, PacketStructureSummary, RawCaptureEntry, affinity_kind_from_key,
    analyze_packet, diff_structure, session_fingerprint_from_payload,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

// ── Config ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCaptureConfig {
    pub enabled: bool,
    pub dir: String,
    #[serde(default = "default_max_index_lines")]
    pub max_index_lines: usize,
    #[serde(default = "default_max_body_files")]
    pub max_body_files: usize,
    /// Max client body bytes to write. `0` = no limit (64 MiB gateway limit applies).
    #[serde(default)]
    pub max_client_bytes: usize,
    /// Max upstream body bytes to write. `0` = no limit.
    #[serde(default)]
    pub max_upstream_bytes: usize,
    /// Mask API keys (`sk-*`) in captured bodies. Default: false (no masking).
    #[serde(default)]
    pub mask_api_keys: bool,
    /// Paths to skip (health checks, etc.).
    #[serde(default = "default_skip_paths")]
    pub skip_paths: Vec<String>,
}

fn default_max_index_lines() -> usize {
    5000
}
fn default_max_body_files() -> usize {
    5000
}
fn default_skip_paths() -> Vec<String> {
    vec![
        "/health".to_string(),
        "/healthz".to_string(),
        "/ready".to_string(),
    ]
}

impl Default for RawCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: "/var/log/crabcache/raw_capture".to_string(),
            max_index_lines: default_max_index_lines(),
            max_body_files: default_max_body_files(),
            max_client_bytes: 0,
            max_upstream_bytes: 0,
            mask_api_keys: false,
            skip_paths: default_skip_paths(),
        }
    }
}

// ── Index writer ─────────────────────────────────────────────────────

struct IndexWriter {
    file: File,
    path: PathBuf,
    max_lines: usize,
    line_count: usize,
}

impl IndexWriter {
    fn new(dir: &str, max_lines: usize) -> std::io::Result<Self> {
        let path = PathBuf::from(dir).join("index.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            file,
            path,
            max_lines,
            line_count: 0,
        })
    }

    fn write_entry(&mut self, entry: &RawCaptureEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?
            + "\n";
        self.file.write_all(line.as_bytes())?;
        self.file.flush()?;
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
                .expect("path has file_name")
                .to_str()
                .expect("file_name is valid UTF-8"),
            timestamp
        ));
        fs::rename(&self.path, &rotated)?;
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.line_count = 0;
        Ok(())
    }
}

// ── Body file cleanup ────────────────────────────────────────────────

fn cleanup_body_files(dir: &str, max_files: usize) {
    let bodies_dir = PathBuf::from(dir).join("bodies");
    if !bodies_dir.exists() {
        return;
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&bodies_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .collect();

    files.sort();
    while files.len() > max_files {
        let oldest = files.remove(0);
        let _ = fs::remove_file(oldest);
    }
}

// ── Writer message ───────────────────────────────────────────────────

struct RawCaptureMessage {
    entry: RawCaptureEntry,
    client_body: Option<Vec<u8>>,
    upstream_body: Option<Vec<u8>>,
}

// ── RawCaptureLogger ─────────────────────────────────────────────────

pub struct RawCaptureLogger {
    sender: mpsc::Sender<RawCaptureMessage>,
    skip_paths: Vec<String>,
    max_client_bytes: usize,
    max_upstream_bytes: usize,
    mask_api_keys: bool,
    dir: String,
}

impl RawCaptureLogger {
    pub fn init(config: RawCaptureConfig) -> Self {
        let dir = config.dir.clone();
        let max_client_bytes = config.max_client_bytes;
        let max_upstream_bytes = config.max_upstream_bytes;
        let mask_api_keys = config.mask_api_keys;
        let skip_paths = config.skip_paths.clone();
        let dir_clone = config.dir.clone();
        let max_index_lines = config.max_index_lines;
        let max_body_files = config.max_body_files;

        let (tx, rx) = mpsc::channel::<RawCaptureMessage>();

        std::thread::Builder::new()
            .name("crab-raw-capture".into())
            .spawn(move || {
                let mut writer = match IndexWriter::new(&dir_clone, max_index_lines) {
                    Ok(w) => w,
                    Err(e) => {
                        warn!("Failed to initialize raw capture index writer: {}", e);
                        return;
                    }
                };

                let bodies_dir = PathBuf::from(&dir_clone).join("bodies");
                if let Err(e) = fs::create_dir_all(&bodies_dir) {
                    warn!("Failed to create raw capture bodies directory: {}", e);
                    return;
                }

                let mut msg_count: u64 = 0;
                while let Ok(msg) = rx.recv() {
                    // Write body files.
                    if let Some(body) = &msg.client_body {
                        let path = bodies_dir.join(format!("{}.client.json", msg.entry.request_id));
                        if let Err(e) = fs::write(&path, body) {
                            warn!("Failed to write client body file: {}", e);
                        }
                    }
                    if let Some(body) = &msg.upstream_body {
                        let path =
                            bodies_dir.join(format!("{}.upstream.json", msg.entry.request_id));
                        if let Err(e) = fs::write(&path, body) {
                            warn!("Failed to write upstream body file: {}", e);
                        }
                    }

                    // Write index entry.
                    if let Err(e) = writer.write_entry(&msg.entry) {
                        warn!("Raw capture index write failed: {}", e);
                    }

                    // Throttled body file cleanup (every 100 messages).
                    msg_count += 1;
                    if msg_count.is_multiple_of(100) {
                        cleanup_body_files(&dir_clone, max_body_files);
                    }
                }
            })
            .expect("Failed to spawn raw capture logger thread");

        Self {
            sender: tx,
            skip_paths,
            max_client_bytes,
            max_upstream_bytes,
            mask_api_keys,
            dir,
        }
    }

    /// Returns true if the given path should be skipped.
    pub fn should_skip(&self, path: &str) -> bool {
        self.skip_paths
            .iter()
            .any(|sp| path == sp || path.starts_with(sp))
    }

    /// Directory where raw capture data is written.
    pub fn dir(&self) -> &str {
        &self.dir
    }

    /// Attempt to capture a request. This is the main entry point called from
    /// the proxy logging phase.
    ///
    /// `client_body` = `original_request_body` (client → gateway)
    /// `upstream_body` = prepared upstream JSON (`upstream_body_for_capture` snapshot)
    pub fn capture(
        &self,
        request_id: &str,
        request_hash: Option<&str>,
        model: &str,
        consumer: Option<&str>,
        project_id: Option<&str>,
        pipeline: Option<&str>,
        stream: bool,
        retired_prefix_messages: Option<usize>,
        reasoning_strategy: Option<&str>,
        client_body: Option<&[u8]>,
        upstream_body: Option<&[u8]>,
        meta: CaptureRequestMeta,
    ) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Analyze client and upstream packets.
        let client_json = client_body.and_then(parse_json_safe);
        let (client_summary, client_json_ok) = match client_json.as_ref() {
            Some(json) => (analyze_packet(json), true),
            None => (PacketStructureSummary::default(), false),
        };

        let mut session_fingerprint = meta.session_fingerprint.clone();
        let mut body_user = meta.body_user.clone();
        let mut conversation_id = meta.conversation_id.clone();
        let mut prompt_cache_key = meta.prompt_cache_key.clone();
        if let Some(ref json) = client_json {
            if session_fingerprint.is_none() {
                session_fingerprint = session_fingerprint_from_payload(json);
            }
            if body_user.is_none() {
                body_user = json
                    .get("user")
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string());
            }
            if conversation_id.is_none() {
                conversation_id = json
                    .get("conversation_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
            if prompt_cache_key.is_none() {
                prompt_cache_key = json
                    .get("prompt_cache_key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }
        let affinity_kind = meta
            .affinity_key
            .as_deref()
            .map(affinity_kind_from_key)
            .map(str::to_string)
            .or(meta.affinity_kind.clone());
        let (upstream_summary, upstream_json_ok) = match upstream_body.and_then(parse_json_safe) {
            Some(json) => (analyze_packet(&json), true),
            None => (PacketStructureSummary::default(), false),
        };

        let diff = diff_structure(&client_summary, &upstream_summary);

        let client_body_bytes = client_body.map(|b| b.len() as u64).unwrap_or(0);
        let upstream_body_bytes = upstream_body.map(|b| b.len() as u64).unwrap_or(0);
        let delta_bytes = upstream_body_bytes as i64 - client_body_bytes as i64;

        // Build body file contents (optionally masked).
        let client_file_body =
            client_body.map(|b| apply_body_limit(b, self.max_client_bytes, self.mask_api_keys));
        let upstream_file_body =
            upstream_body.map(|b| apply_body_limit(b, self.max_upstream_bytes, self.mask_api_keys));

        // Determine whether upstream body differs from client (compare file-body bytes).
        let upstream_path =
            if upstream_file_body.is_some() && upstream_file_body != client_file_body {
                Some(format!("{}.upstream.json", request_id))
            } else {
                None
            };
        let client_path = if client_body.is_some() {
            Some(format!("{}.client.json", request_id))
        } else {
            None
        };

        // Capture errors (concatenate both if both fail).
        let mut errors = Vec::new();
        if client_body.is_some() && !client_json_ok {
            errors.push("client JSON parse failed");
        }
        if upstream_body.is_some() && !upstream_json_ok {
            errors.push("upstream JSON parse failed");
        }
        let capture_error = if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        };

        let entry = RawCaptureEntry {
            timestamp_ms,
            timestamp_beijing: None,
            request_id: request_id.to_string(),
            request_hash: request_hash.map(|s| s.to_string()),
            model: model.to_string(),
            consumer: consumer.map(|s| s.to_string()),
            project_id: project_id.map(|s| s.to_string()),
            pipeline: pipeline.map(|s| s.to_string()),
            stream,
            client_body_bytes,
            upstream_body_bytes,
            delta_bytes,
            structure: diff,
            retired_prefix_messages,
            reasoning_strategy: reasoning_strategy.map(|s| s.to_string()),
            client_path,
            upstream_path,
            capture_error,
            conversation_id,
            prompt_cache_key,
            session_fingerprint,
            body_user,
            affinity_kind,
            affinity_key: meta.affinity_key.clone(),
            backend_name: meta.backend_name.clone(),
            upstream_host: meta.upstream_host.clone(),
            client_key_fingerprint: meta.client_key_fingerprint.clone(),
            upstream_key_id: meta.upstream_key_id.clone(),
            upstream_profile_id: meta.upstream_profile_id.clone(),
            domain: meta.domain.clone(),
            cache_tier: meta.cache_tier.clone(),
            cache_hit: meta.cache_hit,
            coalesced_follower: meta.coalesced_follower,
            coalesce_leader: meta.coalesce_leader,
            duration_ms: meta.duration_ms,
            ttft_ms: meta.ttft_ms,
            upstream_latency_ms: meta.upstream_latency_ms,
        };

        let _ = self.sender.send(RawCaptureMessage {
            entry,
            client_body: client_file_body,
            upstream_body: upstream_file_body,
        });
    }
}

/// Parse JSON safely, returning None on failure.
fn parse_json_safe(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

/// Apply byte limit and optional API key masking to a body.
fn apply_body_limit(body: &[u8], max_bytes: usize, mask: bool) -> Vec<u8> {
    let limited: &[u8] = if max_bytes > 0 && body.len() > max_bytes {
        &body[..max_bytes]
    } else {
        body
    };

    if mask {
        let text = String::from_utf8_lossy(limited);
        mask_body_api_keys(&text).into_bytes()
    } else {
        limited.to_vec()
    }
}

/// Mask `sk-*` / `sk-cc-*` tokens in body text.
fn mask_body_api_keys(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if i + 2 < len && chars[i] == 's' && chars[i + 1] == 'k' && chars[i + 2] == '-' {
            let start = i;
            i += 3;
            while i < len
                && (chars[i].is_ascii_alphanumeric() || chars[i] == '-' || chars[i] == '_')
            {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if token.len() > 8 {
                result.push_str(&token[..4]);
                result.push_str("...");
                result.push_str(&token[token.len() - 4..]);
            } else {
                result.push_str(&token[..1]);
                result.push_str("***");
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_body_api_keys() {
        let text = r#"{"api_key": "sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2"}"#;
        let masked = mask_body_api_keys(text);
        assert!(!masked.contains("sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2"));
        assert!(masked.contains("sk-c"));
    }

    #[test]
    fn test_apply_body_limit() {
        let body = b"this is a long body content here";
        let limited = apply_body_limit(body, 10, false);
        assert_eq!(limited.len(), 10);
    }

    #[test]
    fn test_apply_body_limit_no_limit() {
        let body = b"short";
        let limited = apply_body_limit(body, 0, false);
        assert_eq!(limited, body);
    }
}
