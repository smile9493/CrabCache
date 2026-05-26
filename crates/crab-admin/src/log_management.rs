use crab_admin_types::{
    ClearLogsRequest, ClearLogsResponse, ClearTarget, LogDiskUsage, RetentionPolicy,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

use crate::state::AppState;

/// Resolve the trace log base path (active file, without rotated suffixes).
pub fn trace_log_base_path() -> String {
    crate::trace_log::trace_log_path()
}

/// Resolve the composition debug trace log path.
fn debug_trace_log_path() -> String {
    std::env::var("CRABCACHE_DEBUG_TRACE_LOG_PATH")
        .unwrap_or_else(|_| "/var/log/crabcache/trace-debug.jsonl".to_string())
}

/// Resolve the raw capture directory.
fn raw_capture_dir() -> String {
    crate::raw_capture::raw_capture_dir()
}

/// Scan log directories and compute disk usage.
pub fn compute_disk_usage() -> LogDiskUsage {
    let trace_base = trace_log_base_path();
    let debug_base = debug_trace_log_path();
    let capture = raw_capture_dir();

    let (trace_bytes, trace_file_count) = scan_rotated_files(&trace_base);
    let (debug_bytes, debug_file_count) = scan_rotated_files(&debug_base);
    let (cap_idx_bytes, cap_body_bytes, cap_body_count) = scan_capture_dir(&capture);

    LogDiskUsage {
        trace_bytes,
        trace_file_count,
        debug_trace_bytes: debug_bytes,
        debug_trace_file_count: debug_file_count,
        capture_index_bytes: cap_idx_bytes,
        capture_body_bytes: cap_body_bytes,
        capture_body_file_count: cap_body_count,
        total_bytes: trace_bytes + debug_bytes + cap_idx_bytes + cap_body_bytes,
    }
}

/// Scan rotated files for a given base path.
/// Returns (total_bytes, file_count) for rotated files only (excludes active file).
fn scan_rotated_files(base_path: &str) -> (u64, usize) {
    let path = Path::new(base_path);
    let Some(parent) = path.parent() else {
        return (0, 0);
    };
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return (0, 0);
    };

    let Ok(entries) = fs::read_dir(parent) else {
        return (0, 0);
    };

    let mut total: u64 = 0;
    let mut count = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Rotated files: {base_name}.{timestamp}
        if name_str.starts_with(file_name)
            && name_str != file_name
            && let Ok(meta) = entry.metadata()
        {
            total += meta.len();
            count += 1;
        }
    }
    (total, count)
}

/// Scan the capture directory. Returns (index_bytes, body_bytes, body_file_count).
fn scan_capture_dir(dir: &str) -> (u64, u64, usize) {
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        return (0, 0, 0);
    }

    // Scan index files (active + rotated)
    let mut index_bytes: u64 = 0;
    if let Ok(entries) = fs::read_dir(dir_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if (name_str == "index.jsonl" || name_str.starts_with("index.jsonl."))
                && let Ok(meta) = entry.metadata()
            {
                index_bytes += meta.len();
            }
        }
    }

    // Scan bodies directory
    let bodies_dir = dir_path.join("bodies");
    let (body_bytes, body_count) = if bodies_dir.is_dir() {
        scan_directory_total(&bodies_dir)
    } else {
        (0, 0)
    };

    (index_bytes, body_bytes, body_count)
}

/// Scan all files in a directory, returning (total_bytes, file_count).
fn scan_directory_total(dir: &Path) -> (u64, usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut total: u64 = 0;
    let mut count = 0;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            total += meta.len();
            count += 1;
        }
    }
    (total, count)
}

/// Clear logs matching the request criteria.
pub fn clear_logs(request: &ClearLogsRequest) -> Result<ClearLogsResponse, String> {
    let trace_base = trace_log_base_path();
    let debug_base = debug_trace_log_path();
    let capture = raw_capture_dir();

    let cutoff = request
        .older_than_hours
        .map(|h| SystemTime::now() - Duration::from_secs(h as u64 * 3600));

    let mut deleted_files = Vec::new();
    let mut freed_bytes: u64 = 0;

    match request.target {
        ClearTarget::TraceRotated => {
            delete_rotated_files(&trace_base, cutoff, &mut deleted_files, &mut freed_bytes);
        }
        ClearTarget::DebugRotated => {
            delete_rotated_files(&debug_base, cutoff, &mut deleted_files, &mut freed_bytes);
        }
        ClearTarget::Capture => {
            delete_capture_files(&capture, cutoff, &mut deleted_files, &mut freed_bytes);
        }
        ClearTarget::All => {
            delete_rotated_files(&trace_base, cutoff, &mut deleted_files, &mut freed_bytes);
            delete_rotated_files(&debug_base, cutoff, &mut deleted_files, &mut freed_bytes);
            delete_capture_files(&capture, cutoff, &mut deleted_files, &mut freed_bytes);
        }
    }

    info!(
        target = ?request.target,
        deleted = deleted_files.len(),
        freed_bytes,
        "Log clear completed"
    );

    Ok(ClearLogsResponse {
        deleted_files,
        freed_bytes,
    })
}

/// Delete rotated files for a given base path.
fn delete_rotated_files(
    base_path: &str,
    cutoff: Option<SystemTime>,
    deleted: &mut Vec<String>,
    freed: &mut u64,
) {
    let path = Path::new(base_path);
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };

    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Only delete rotated files, never the active file
        if !(name_str.starts_with(file_name) && name_str != file_name) {
            continue;
        }

        let Ok(meta) = entry.metadata() else {
            continue;
        };

        // Check age cutoff
        if let Some(cutoff_time) = cutoff
            && let Ok(modified) = meta.modified()
            && modified > cutoff_time
        {
            continue;
        }

        let size = meta.len();
        let path_str = entry.path().to_string_lossy().to_string();
        if fs::remove_file(entry.path()).is_ok() {
            deleted.push(path_str);
            *freed += size;
        }
    }
}

/// Delete capture files (index rotated + bodies).
fn delete_capture_files(
    dir: &str,
    cutoff: Option<SystemTime>,
    deleted: &mut Vec<String>,
    freed: &mut u64,
) {
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        return;
    }

    // Delete rotated index files
    if let Ok(entries) = fs::read_dir(dir_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Rotated index: index.jsonl.{timestamp}
            if !name_str.starts_with("index.jsonl.") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if let Some(cutoff_time) = cutoff
                && let Ok(modified) = meta.modified()
                && modified > cutoff_time
            {
                continue;
            }
            let size = meta.len();
            let path_str = entry.path().to_string_lossy().to_string();
            if fs::remove_file(entry.path()).is_ok() {
                deleted.push(path_str);
                *freed += size;
            }
        }
    }

    // Delete body files
    let bodies_dir = dir_path.join("bodies");
    if !bodies_dir.is_dir() {
        return;
    }
    if let Ok(entries) = fs::read_dir(&bodies_dir) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if let Some(cutoff_time) = cutoff
                && let Ok(modified) = meta.modified()
                && modified > cutoff_time
            {
                continue;
            }
            let size = meta.len();
            let path_str = entry.path().to_string_lossy().to_string();
            if fs::remove_file(entry.path()).is_ok() {
                deleted.push(path_str);
                *freed += size;
            }
        }
    }
}

/// Enforce retention policy. Called by the background cleanup task.
pub fn enforce_retention(policy: &RetentionPolicy) -> Result<ClearLogsResponse, String> {
    let trace_base = trace_log_base_path();
    let debug_base = debug_trace_log_path();
    let capture = raw_capture_dir();

    let mut deleted_files = Vec::new();
    let mut freed_bytes: u64 = 0;

    // 1. Age-based cleanup
    if policy.max_age_hours > 0 {
        let cutoff = SystemTime::now() - Duration::from_secs(policy.max_age_hours as u64 * 3600);
        let cutoff = Some(cutoff);

        delete_rotated_files(&trace_base, cutoff, &mut deleted_files, &mut freed_bytes);
        delete_rotated_files(&debug_base, cutoff, &mut deleted_files, &mut freed_bytes);
        delete_capture_files(&capture, cutoff, &mut deleted_files, &mut freed_bytes);
    }

    // 2. File-count-based cleanup for trace files
    if policy.max_trace_files > 0 {
        prune_rotated_by_count(
            &trace_base,
            policy.max_trace_files,
            &mut deleted_files,
            &mut freed_bytes,
        );
        prune_rotated_by_count(
            &debug_base,
            policy.max_trace_files,
            &mut deleted_files,
            &mut freed_bytes,
        );
    }

    // 3. Disk-usage-based cleanup
    if policy.max_disk_mb > 0 {
        let max_bytes = policy.max_disk_mb as u64 * 1024 * 1024;
        prune_by_disk_usage(
            &trace_base,
            &debug_base,
            &capture,
            max_bytes,
            &mut deleted_files,
            &mut freed_bytes,
        );
    }

    // 4. Capture body file count limit
    if policy.max_capture_body_files > 0 {
        prune_capture_bodies(
            &capture,
            policy.max_capture_body_files,
            &mut deleted_files,
            &mut freed_bytes,
        );
    }

    if !deleted_files.is_empty() {
        info!(
            deleted = deleted_files.len(),
            freed_bytes, "Retention enforcement cleaned up log files"
        );
    }

    Ok(ClearLogsResponse {
        deleted_files,
        freed_bytes,
    })
}

/// Keep at most `max_count` rotated files, deleting the oldest.
fn prune_rotated_by_count(
    base_path: &str,
    max_count: usize,
    deleted: &mut Vec<String>,
    freed: &mut u64,
) {
    let path = Path::new(base_path);
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };

    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };

    let mut rotated: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(file_name) && name_str != file_name {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                Some((e.path(), size))
            } else {
                None
            }
        })
        .collect();

    rotated.sort();

    while rotated.len() > max_count {
        let (oldest, size) = rotated.remove(0);
        let path_str = oldest.to_string_lossy().to_string();
        if fs::remove_file(&oldest).is_ok() {
            deleted.push(path_str);
            *freed += size;
        }
    }
}

/// Prune oldest files across trace + capture until total is under `max_bytes`.
fn prune_by_disk_usage(
    trace_base: &str,
    debug_base: &str,
    capture_dir: &str,
    max_bytes: u64,
    deleted: &mut Vec<String>,
    freed: &mut u64,
) {
    // Collect all prunable files with their modification times
    let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();

    collect_rotated_with_mtime(trace_base, &mut files);
    collect_rotated_with_mtime(debug_base, &mut files);
    collect_capture_files_with_mtime(capture_dir, &mut files);

    // Compute current total
    let current_total: u64 = files.iter().map(|(_, size, _)| *size).sum();
    if current_total <= max_bytes {
        return;
    }

    // Sort oldest first
    files.sort_by_key(|(_, _, mtime)| *mtime);

    let mut remaining = current_total;
    for (path, size, _) in files {
        if remaining <= max_bytes {
            break;
        }
        let path_str = path.to_string_lossy().to_string();
        if fs::remove_file(&path).is_ok() {
            remaining = remaining.saturating_sub(size);
            deleted.push(path_str);
            *freed += size;
        }
    }
}

/// Collect rotated files with their modification times.
fn collect_rotated_with_mtime(base_path: &str, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let path = Path::new(base_path);
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };

    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(file_name)
            && name_str != file_name
            && let Ok(meta) = entry.metadata()
        {
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push((entry.path(), meta.len(), mtime));
        }
    }
}

/// Collect capture files (rotated index + bodies) with modification times.
fn collect_capture_files_with_mtime(dir: &str, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        return;
    }

    // Rotated index files
    if let Ok(entries) = fs::read_dir(dir_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("index.jsonl.")
                && let Ok(meta) = entry.metadata()
            {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                out.push((entry.path(), meta.len(), mtime));
            }
        }
    }

    // Body files
    let bodies_dir = dir_path.join("bodies");
    if let Ok(entries) = fs::read_dir(&bodies_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                out.push((entry.path(), meta.len(), mtime));
            }
        }
    }
}

/// Keep at most `max_files` body files, deleting the oldest.
fn prune_capture_bodies(dir: &str, max_files: usize, deleted: &mut Vec<String>, freed: &mut u64) {
    let bodies_dir = Path::new(dir).join("bodies");
    if !bodies_dir.is_dir() {
        return;
    }

    let Ok(entries) = fs::read_dir(&bodies_dir) else {
        return;
    };

    let mut files: Vec<(PathBuf, u64)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if meta.is_file() {
                Some((e.path(), meta.len()))
            } else {
                None
            }
        })
        .collect();

    files.sort();

    while files.len() > max_files {
        let (oldest, size) = files.remove(0);
        let path_str = oldest.to_string_lossy().to_string();
        if fs::remove_file(&oldest).is_ok() {
            deleted.push(path_str);
            *freed += size;
        }
    }
}

/// Background loop that enforces the retention policy every 10 minutes.
/// Also prunes PG trace_logs based on the configured retention period.
pub async fn log_retention_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(600));
    loop {
        interval.tick().await;
        let policy = state.log_retention.read().clone();

        // PG trace_logs retention (default 7 days).
        {
            let pg_ref = state.pg_store.read().clone();
            if let Some(ref pg) = pg_ref {
                let retention_days: u64 = std::env::var("CRADMIN_TRACE_RETENTION_DAYS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(7);
                if retention_days > 0 {
                    let cutoff_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64
                        - retention_days * 86_400_000;
                    match pg.prune_trace_logs(cutoff_ms).await {
                        Ok(count) if count > 0 => {
                            info!(
                                deleted = count,
                                cutoff_days = retention_days,
                                "PG trace_logs pruned"
                            );
                        }
                        Err(e) => warn!(error = %e, "PG trace_logs prune failed"),
                        _ => {}
                    }
                    match pg.prune_request_logs(cutoff_ms).await {
                        Ok(count) if count > 0 => {
                            info!(
                                deleted = count,
                                cutoff_days = retention_days,
                                "PG request_logs pruned"
                            );
                        }
                        Err(e) => warn!(error = %e, "PG request_logs prune failed"),
                        _ => {}
                    }
                }
            }
        }

        // JSONL file retention.
        if policy.max_age_hours == 0
            && policy.max_disk_mb == 0
            && policy.max_trace_files == 0
            && policy.max_capture_body_files == 0
        {
            continue;
        }
        match tokio::task::spawn_blocking(move || enforce_retention(&policy)).await {
            Ok(Ok(result)) if !result.deleted_files.is_empty() => {
                info!(
                    deleted = result.deleted_files.len(),
                    freed_bytes = result.freed_bytes,
                    "Log retention enforcement cleaned up files"
                );
            }
            Ok(Err(e)) => warn!(error = %e, "Log retention enforcement failed"),
            Err(e) => warn!(error = %e, "Log retention task panicked"),
            _ => {}
        }
    }
}
