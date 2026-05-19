use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
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
    pub model: String,
    pub prompt_tokens: usize,
    pub latency_ms: f64,
    pub cache_hit: bool,
    pub cache_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retired_prefix_messages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_hit_ratio: Option<f64>,
}

impl SanitizedLogEntry {
    pub fn from_request(
        body: &[u8],
        conversation_id: Option<String>,
        model: &str,
        prompt_tokens: usize,
        latency_ms: f64,
        cache_hit: bool,
        cache_tier: Option<String>,
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

        Self {
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            request_hash,
            content_length: body.len(),
            semantic_cluster,
            conversation_id,
            model: model.to_string(),
            prompt_tokens,
            latency_ms,
            cache_hit,
            cache_tier,
            retired_prefix_messages: None,
            reasoning_strategy: None,
            prompt_cache_hit_ratio: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub enabled: bool,
    pub path: String,
    pub max_lines: usize,
    pub max_files: usize,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: "/var/log/crabcache/trace.jsonl".to_string(),
            max_lines: 10000,
            max_files: 5,
        }
    }
}

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

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

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
            self.path.file_name().unwrap().to_str().unwrap(),
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
        let parent = self.path.parent().unwrap();
        let file_name = self.path.file_name().unwrap().to_str().unwrap();

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

pub struct TraceLogger {
    sender: mpsc::Sender<SanitizedLogEntry>,
}

impl TraceLogger {
    pub fn init(config: TraceConfig) -> Self {
        let (tx, rx) = mpsc::channel::<SanitizedLogEntry>();

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

        Self { sender: tx }
    }

    pub fn log(&self, entry: SanitizedLogEntry) {
        let _ = self.sender.send(entry);
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
            "deepseek-chat",
            100,
            150.5,
            false,
            None,
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
        let entry1 = SanitizedLogEntry::from_request(body, None, "model", 0, 0.0, false, None);
        let entry2 = SanitizedLogEntry::from_request(body, None, "model", 0, 0.0, false, None);

        assert_eq!(entry1.request_hash, entry2.request_hash);
        assert_eq!(entry1.semantic_cluster, entry2.semantic_cluster);
    }
}
