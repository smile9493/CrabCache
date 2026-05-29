use crate::state::AppState;
use crate::trace_log::{self, TraceLogEntry};
use std::sync::Arc;
use std::time::Duration;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use tracing::{info, warn};

/// When gateway writes PG directly (`CRABCACHE_TRACE_PG_URL`), set to `false` on admin.
pub fn pg_sync_enabled() -> bool {
    let enabled = std::env::var("CRABCACHE_ADMIN_TRACE_PG_SYNC")
        .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    if enabled
        && std::env::var("CRABCACHE_TRACE_PG_URL")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    {
        warn!(
            "CRABCACHE_TRACE_PG_URL is set (gateway direct PG write) while admin JSONL sync is enabled; \
             set CRABCACHE_ADMIN_TRACE_PG_SYNC=false to avoid duplicate insert attempts"
        );
    }
    enabled
}

/// Background task that periodically syncs new JSONL trace log entries to PG.
/// Returns `true` if the sync loop was spawned.
pub fn spawn_pg_sync(state: Arc<AppState>) -> bool {
    if !pg_sync_enabled() {
        info!("PG trace JSONL sync disabled (CRABCACHE_ADMIN_TRACE_PG_SYNC=false)");
        return false;
    }
    tokio::spawn(async move {
        let interval = Duration::from_secs(10);
        let mut last_offset: u64 = 0;
        #[cfg(unix)]
        let mut last_inode: Option<u64> = None;

        loop {
            tokio::time::sleep(interval).await;

            let pg_ref = state.pg_store.read().clone();
            let Some(ref pg) = pg_ref else { continue };

            let trace_path = trace_log::trace_log_path();
            #[cfg(unix)]
            let sync_result = read_new_entries(&trace_path, last_offset, last_inode);
            #[cfg(not(unix))]
            let sync_result = read_new_entries(&trace_path, last_offset);
            match sync_result {
                Ok((entries, new_offset, new_inode)) if !entries.is_empty() => {
                    match pg.insert_trace_logs(&entries).await {
                        Ok(()) => {
                            info!(
                                count = entries.len(),
                                "PG sync: wrote new trace log entries"
                            );
                            last_offset = new_offset;
                            #[cfg(unix)]
                            {
                                last_inode = new_inode;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "PG sync: failed to insert trace logs");
                        }
                    }
                }
                Ok((_, new_offset, new_inode)) => {
                    last_offset = new_offset;
                    #[cfg(unix)]
                    {
                        last_inode = new_inode;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "PG sync: failed to read trace log");
                }
            }
        }
    });
    true
}

/// Read trace log entries starting from `byte_offset` in the JSONL file.
/// Returns parsed entries, the new byte offset, and current inode (Unix).
fn read_new_entries(
    path: &str,
    byte_offset: u64,
    #[cfg(unix)] last_inode: Option<u64>,
) -> Result<(Vec<TraceLogEntry>, u64, Option<u64>), std::io::Error> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let file = File::open(path)?;
    let meta = file.metadata()?;
    let file_len = meta.len();
    #[cfg(unix)]
    let current_inode = Some(meta.ino());
    #[cfg(not(unix))]
    let current_inode: Option<u64> = None;

    let mut effective_offset = byte_offset;
    #[cfg(unix)]
    if let (Some(prev), Some(cur)) = (last_inode, current_inode) {
        if prev != cur {
            // File rotated in place: read only new active file from start.
            effective_offset = 0;
        }
    }
    #[cfg(not(unix))]
    if effective_offset > file_len {
        effective_offset = 0;
    }

    let mut reader = BufReader::new(file);
    if effective_offset > 0 && effective_offset <= file_len {
        reader.seek(SeekFrom::Start(effective_offset))?;
    }

    let mut entries = Vec::new();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<TraceLogEntry>(trimmed) {
            entries.push(entry);
        }
    }

    let new_offset = reader.stream_position()?;
    Ok((entries, new_offset, current_inode))
}
