use crab_capture::{
    CaptureRequestMeta, PacketStructureSummary, RawCaptureEntry, affinity_kind_from_key,
    analyze_packet, diff_structure, session_fingerprint_from_payload,
};
use crab_metrics::global_metrics;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
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
    /// Sampling rate for normal requests (0.0 = skip all, 1.0 = capture all).
    /// Default: 1.0 (backward-compatible, capture every request).
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
    /// Always capture requests that result in upstream 4xx/5xx errors.
    /// Default: true (error requests are always captured regardless of `sample_rate`).
    #[serde(default = "default_true")]
    pub sample_always_on_error: bool,
    /// Always capture requests with body larger than `min_body_bytes_for_large`.
    /// Default: false (use `sample_rate` regardless of body size).
    #[serde(default)]
    pub sample_always_on_large_body: bool,
    /// Body size threshold (bytes) for `sample_always_on_large_body`. Default: 2 MiB.
    #[serde(default = "default_min_body_bytes_for_large")]
    pub min_body_bytes_for_large: usize,
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
fn default_sample_rate() -> f64 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_min_body_bytes_for_large() -> usize {
    2 * 1024 * 1024
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
            sample_rate: default_sample_rate(),
            sample_always_on_error: default_true(),
            sample_always_on_large_body: false,
            min_body_bytes_for_large: default_min_body_bytes_for_large(),
        }
    }
}

// ── Index writer ─────────────────────────────────────────────────────

/// Flush interval for IndexWriter: flush every N entries.
const RAW_FLUSH_INTERVAL: usize = 50;

struct IndexWriter {
    file: BufWriter<File>,
    path: PathBuf,
    max_lines: usize,
    line_count: usize,
    flush_counter: usize,
}

impl IndexWriter {
    fn new(dir: &str, max_lines: usize) -> std::io::Result<Self> {
        let path = PathBuf::from(dir).join("index.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            file: BufWriter::new(file),
            path,
            max_lines,
            line_count: 0,
            flush_counter: 0,
        })
    }

    fn write_entry(&mut self, entry: &RawCaptureEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?
            + "\n";
        self.file.write_all(line.as_bytes())?;
        self.line_count += 1;
        self.flush_counter += 1;

        if self.flush_counter >= RAW_FLUSH_INTERVAL {
            self.file.flush()?;
            self.flush_counter = 0;
        }
        if self.line_count >= self.max_lines {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
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
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.file = BufWriter::new(file);
        self.line_count = 0;
        self.flush_counter = 0;
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

/// Sampling decision for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleDecision {
    /// Capture this request (sampled or forced by error/large body).
    Capture,
    /// Skip this request (not sampled and no override applies).
    Skip,
}

/// Deterministic round-robin sampler using an atomic counter.
struct Sampler {
    counter: AtomicU64,
    /// Inverse of sample_rate: capture every Nth request (0 = always capture).
    every_n: u64,
    always_on_error: bool,
    always_on_large_body: bool,
    min_body_bytes_for_large: usize,
}

impl Sampler {
    fn from_config(config: &RawCaptureConfig) -> Self {
        let rate = config.sample_rate.clamp(0.0, 1.0);
        let every_n = if rate >= 1.0 {
            0
        } else if rate <= 0.0 {
            u64::MAX
        } else {
            (1.0 / rate).round() as u64
        };
        Self {
            counter: AtomicU64::new(0),
            every_n,
            always_on_error: config.sample_always_on_error,
            always_on_large_body: config.sample_always_on_large_body,
            min_body_bytes_for_large: config.min_body_bytes_for_large,
        }
    }

    fn decide(&self, has_error: bool, body_bytes: usize) -> SampleDecision {
        // Error override: always capture failing requests.
        if has_error && self.always_on_error {
            global_metrics().record_raw_capture_sample("always_error");
            return SampleDecision::Capture;
        }
        // Large body override.
        if self.always_on_large_body && body_bytes >= self.min_body_bytes_for_large {
            global_metrics().record_raw_capture_sample("always_large");
            return SampleDecision::Capture;
        }
        // Deterministic round-robin: capture every Nth request.
        if self.every_n == 0 {
            global_metrics().record_raw_capture_sample("sampled");
            return SampleDecision::Capture;
        }
        let seq = self.counter.fetch_add(1, Ordering::Relaxed);
        if seq.is_multiple_of(self.every_n) {
            global_metrics().record_raw_capture_sample("sampled");
            SampleDecision::Capture
        } else {
            global_metrics().record_raw_capture_sample("skipped");
            SampleDecision::Skip
        }
    }
}

pub struct RawCaptureLogger {
    sender: mpsc::Sender<RawCaptureMessage>,
    skip_paths: Vec<String>,
    max_client_bytes: usize,
    max_upstream_bytes: usize,
    mask_api_keys: bool,
    dir: String,
    sampler: Sampler,
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

        let sampler = Sampler::from_config(&config);

        Self {
            sender: tx,
            skip_paths,
            max_client_bytes,
            max_upstream_bytes,
            mask_api_keys,
            dir,
            sampler,
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

    /// Returns true if this request should be captured based on sampling rules.
    pub fn should_sample(&self, has_error: bool, body_bytes: usize) -> SampleDecision {
        self.sampler.decide(has_error, body_bytes)
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
        parsed_client_json: Option<&Value>,
        parsed_upstream_json: Option<&Value>,
        meta: CaptureRequestMeta,
    ) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Analyze client and upstream packets.
        let client_json_owned = parsed_client_json
            .is_none()
            .then(|| client_body.and_then(parse_json_safe))
            .flatten();
        let client_json = parsed_client_json.or(client_json_owned.as_ref());
        let client_analyze_start = Instant::now();
        let (client_summary, client_json_ok) = match client_json {
            Some(json) => (analyze_packet(json), true),
            None => (PacketStructureSummary::default(), false),
        };
        let client_analyze_elapsed = client_analyze_start.elapsed();
        global_metrics().record_request_body_stage(
            "raw_capture_client_analyze",
            client_analyze_elapsed,
            client_body.map_or(0, |b| b.len()),
            pipeline,
        );

        let mut session_fingerprint = meta.session_fingerprint.clone();
        let mut body_user = meta.body_user.clone();
        let mut conversation_id = meta.conversation_id.clone();
        let mut prompt_cache_key = meta.prompt_cache_key.clone();
        if let Some(json) = client_json {
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
        let upstream_json_owned = parsed_upstream_json
            .is_none()
            .then(|| upstream_body.and_then(parse_json_safe))
            .flatten();
        let upstream_json = parsed_upstream_json.or(upstream_json_owned.as_ref());
        let upstream_analyze_start = Instant::now();
        let (upstream_summary, upstream_json_ok) = match upstream_json {
            Some(json) => (analyze_packet(json), true),
            None => (PacketStructureSummary::default(), false),
        };
        let upstream_analyze_elapsed = upstream_analyze_start.elapsed();
        global_metrics().record_request_body_stage(
            "raw_capture_upstream_analyze",
            upstream_analyze_elapsed,
            upstream_body.map_or(0, |b| b.len()),
            pipeline,
        );

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
        crate::masking::mask_api_keys(&text).into_bytes()
    } else {
        limited.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
