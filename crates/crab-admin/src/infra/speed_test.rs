//! Async bandwidth test jobs (download / upload probe).

use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DEFAULT_DOWNLOAD_URL: &str = "https://speed.cloudflare.com/__down?bytes=10485760";
const DEFAULT_TEST_BYTES: u64 = 10 * 1024 * 1024;
const DEFAULT_JOB_TTL_SECS: u64 = 900;
const MAX_JOBS_RETAINED: usize = 64;
const DEFAULT_MAX_UPLOAD_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_UPLOAD_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedTestDirection {
    Download,
    Upload,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedTestStatus {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestJobView {
    pub job_id: String,
    pub status: SpeedTestStatus,
    pub direction: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub error: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
struct SpeedTestJob {
    view: SpeedTestJobView,
    upload_token: Option<String>,
    direction: SpeedTestDirection,
    expires_at: u64,
    upload_attempts: u32,
}

#[derive(Debug, Default)]
pub struct SpeedTestJobs {
    jobs: DashMap<String, SpeedTestJob>,
    running: Mutex<bool>,
}

impl SpeedTestJobs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, job_id: &str) -> Option<SpeedTestJobView> {
        self.jobs.get(job_id).and_then(|j| {
            if j.expires_at <= now_secs() {
                None
            } else {
                Some(j.view.clone())
            }
        })
    }

    pub fn try_start(
        &self,
        direction: SpeedTestDirection,
    ) -> Result<(String, Option<String>), &'static str> {
        self.purge_expired();

        let mut running = self.running.lock();
        if *running {
            return Err("speed_test_in_progress");
        }
        *running = true;

        let job_id = Uuid::new_v4().to_string();
        let upload_token = if matches!(
            direction,
            SpeedTestDirection::Upload | SpeedTestDirection::Both
        ) {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };

        let now = now_secs();
        let view = SpeedTestJobView {
            job_id: job_id.clone(),
            status: SpeedTestStatus::Pending,
            direction: direction_label(direction),
            download_mbps: None,
            upload_mbps: None,
            error: None,
            started_at: now,
            finished_at: None,
            upload_token: upload_token.clone(),
            upload_bytes: Some(speed_test_bytes()),
        };

        self.jobs.insert(
            job_id.clone(),
            SpeedTestJob {
                view,
                upload_token: upload_token.clone(),
                direction,
                expires_at: now.saturating_add(job_ttl_secs()),
                upload_attempts: 0,
            },
        );

        Ok((job_id, upload_token))
    }

    pub fn set_running(&self, job_id: &str) {
        if let Some(mut j) = self.jobs.get_mut(job_id) {
            j.view.status = SpeedTestStatus::Running;
        }
    }

    pub fn finish_download(&self, job_id: &str, mbps: Result<f64, String>) {
        if let Some(mut j) = self.jobs.get_mut(job_id) {
            match mbps {
                Ok(v) => j.view.download_mbps = Some(v),
                Err(e) => {
                    j.view.status = SpeedTestStatus::Failed;
                    j.view.error = Some(e);
                    j.view.finished_at = Some(now_secs());
                    *self.running.lock() = false;
                    return;
                }
            }
            if j.direction == SpeedTestDirection::Both {
                return;
            }
            if j.direction == SpeedTestDirection::Download {
                j.view.status = SpeedTestStatus::Done;
                j.view.finished_at = Some(now_secs());
                *self.running.lock() = false;
            }
        }
    }

    pub fn fail(&self, job_id: &str, error: String) {
        if let Some(mut j) = self.jobs.get_mut(job_id) {
            j.view.status = SpeedTestStatus::Failed;
            j.view.error = Some(error);
            j.view.finished_at = Some(now_secs());
        }
        *self.running.lock() = false;
    }

    pub fn validate_upload_token(&self, job_id: &str, token: &str) -> bool {
        self.jobs.get(job_id).is_some_and(|j| {
            j.expires_at > now_secs()
                && j.upload_token.as_deref() == Some(token)
                && matches!(
                    j.direction,
                    SpeedTestDirection::Upload | SpeedTestDirection::Both
                )
        })
    }

    pub fn try_record_upload(
        &self,
        job_id: &str,
        bytes: u64,
        elapsed_secs: f64,
    ) -> Result<(), &'static str> {
        let mut job = self.jobs.get_mut(job_id).ok_or("job_not_found")?;
        if job.expires_at <= now_secs() {
            return Err("job_expired");
        }
        job.upload_attempts += 1;
        if job.upload_attempts > max_upload_attempts() {
            return Err("too_many_upload_attempts");
        }
        let mbps = (bytes as f64 * 8.0) / (elapsed_secs.max(0.001) * 1_000_000.0);
        job.view.upload_mbps = Some(mbps);
        job.view.status = SpeedTestStatus::Done;
        job.view.finished_at = Some(now_secs());
        drop(job);
        *self.running.lock() = false;
        Ok(())
    }

    pub fn purge_expired(&self) {
        let now = now_secs();
        self.jobs.retain(|_, j| j.expires_at > now);
        if self.jobs.len() > MAX_JOBS_RETAINED {
            let mut ids: Vec<_> = self
                .jobs
                .iter()
                .map(|e| (e.key().clone(), e.value().view.started_at))
                .collect();
            ids.sort_by_key(|(_, started)| *started);
            let drop_n = self.jobs.len().saturating_sub(MAX_JOBS_RETAINED);
            for (id, _) in ids.into_iter().take(drop_n) {
                self.jobs.remove(&id);
            }
        }
        if self.jobs.is_empty() {
            *self.running.lock() = false;
        }
    }
}

fn direction_label(direction: SpeedTestDirection) -> String {
    match direction {
        SpeedTestDirection::Download => "download".to_string(),
        SpeedTestDirection::Upload => "upload".to_string(),
        SpeedTestDirection::Both => "both".to_string(),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn job_ttl_secs() -> u64 {
    std::env::var("CRABCACHE_SPEED_TEST_JOB_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_JOB_TTL_SECS)
}

pub fn max_upload_bytes() -> usize {
    std::env::var("CRABCACHE_SPEED_TEST_MAX_UPLOAD_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_UPLOAD_BYTES)
}

fn max_upload_attempts() -> u32 {
    std::env::var("CRABCACHE_SPEED_TEST_MAX_UPLOAD_ATTEMPTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_UPLOAD_ATTEMPTS)
}

pub fn speed_test_bytes() -> u64 {
    std::env::var("CRABCACHE_SPEED_TEST_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TEST_BYTES)
}

pub fn speed_test_download_url() -> String {
    std::env::var("CRABCACHE_SPEED_TEST_DOWNLOAD_URL")
        .unwrap_or_else(|_| DEFAULT_DOWNLOAD_URL.to_string())
}

/// Validate download URL: HTTPS only and host in allowlist.
pub fn validate_download_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https URLs are allowed".into());
    }
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    let allowlist = std::env::var("CRABCACHE_SPEED_TEST_HOST_ALLOWLIST").unwrap_or_else(|_| {
        "speed.cloudflare.com,download.thinkbroadband.com".to_string()
    });
    let allowed: Vec<&str> = allowlist.split(',').map(str::trim).collect();
    if allowed
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
    {
        Ok(())
    } else {
        Err(format!("host {host} not in speed test allowlist"))
    }
}

pub async fn run_download_test(job_id: String, jobs: Arc<SpeedTestJobs>) {
    jobs.set_running(&job_id);
    let url = speed_test_download_url();
    if let Err(e) = validate_download_url(&url) {
        jobs.fail(&job_id, e);
        return;
    }

    let max_bytes = speed_test_bytes();
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            jobs.fail(&job_id, e.to_string());
            return;
        }
    };

    let start = Instant::now();
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            jobs.fail(&job_id, format!("download failed: {e}"));
            return;
        }
    };

    let mut read: u64 = 0;
    let mut stream = resp.bytes_stream();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(b) => {
                read += b.len() as u64;
                if read >= max_bytes {
                    break;
                }
            }
            Err(e) => {
                jobs.fail(&job_id, format!("download stream error: {e}"));
                return;
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    let mbps = (read as f64 * 8.0) / (elapsed * 1_000_000.0);
    jobs.finish_download(&job_id, Ok(mbps));
}

pub async fn run_both_test(job_id: String, jobs: Arc<SpeedTestJobs>) {
    run_download_test(job_id, jobs).await;
}
