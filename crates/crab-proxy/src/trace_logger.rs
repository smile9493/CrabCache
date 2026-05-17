use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
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
    async fn new(config: &TraceConfig) -> std::io::Result<Self> {
        let path = PathBuf::from(&config.path);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        Ok(Self {
            file,
            path,
            max_lines: config.max_lines,
            line_count: 0,
            max_files: config.max_files,
        })
    }

    async fn write_entry(&mut self, entry: &SanitizedLogEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)? + "\n";
        self.file.write_all(line.as_bytes()).await?;
        self.line_count += 1;

        if self.line_count >= self.max_lines {
            self.rotate().await?;
        }
        Ok(())
    }

    async fn rotate(&mut self) -> std::io::Result<()> {
        self.file.sync_all().await?;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let rotated = self.path.with_file_name(format!(
            "{}.{}",
            self.path.file_name().unwrap().to_str().unwrap(),
            timestamp
        ));

        tokio::fs::rename(&self.path, &rotated).await?;

        self.cleanup_old_files().await?;

        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        self.line_count = 0;
        Ok(())
    }

    async fn cleanup_old_files(&mut self) -> std::io::Result<()> {
        let parent = self.path.parent().unwrap();
        let file_name = self.path.file_name().unwrap().to_str().unwrap();

        let mut entries = tokio::fs::read_dir(parent).await?;
        let mut log_files = vec![];

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            if let Some(name) = name.to_str() {
                if name.starts_with(file_name) && name != file_name {
                    log_files.push(entry.path());
                }
            }
        }

        log_files.sort();

        while log_files.len() >= self.max_files {
            let oldest = log_files.remove(0);
            tokio::fs::remove_file(oldest).await?;
        }

        Ok(())
    }
}

pub struct TraceLogger {
    sender: UnboundedSender<SanitizedLogEntry>,
}

impl TraceLogger {
    pub fn init(config: TraceConfig) -> (Self, tokio::task::JoinHandle<()>) {
        let (tx, mut rx): (
            UnboundedSender<SanitizedLogEntry>,
            UnboundedReceiver<SanitizedLogEntry>,
        ) = unbounded_channel();

        let handle = tokio::spawn(async move {
            let mut writer = match LogWriter::new(&config).await {
                Ok(w) => w,
                Err(e) => {
                    warn!("Failed to initialize trace logger: {}", e);
                    return;
                }
            };

            while let Some(entry) = rx.recv().await {
                if let Err(e) = writer.write_entry(&entry).await {
                    warn!("Shadow log write failed: {}", e);
                }
            }
        });

        (Self { sender: tx }, handle)
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
