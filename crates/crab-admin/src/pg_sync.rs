use crate::state::AppState;
use crate::trace_log::{self, TraceLogEntry};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Background task that periodically syncs new JSONL trace log entries to PG.
/// Reads from the JSONL file and tracks the last-synced byte offset to avoid
/// re-inserting entries.
pub fn spawn_pg_sync(state: Arc<AppState>) {
    tokio::spawn(async move {
        let interval = Duration::from_secs(10);
        let mut last_offset: u64 = 0;

        loop {
            tokio::time::sleep(interval).await;

            let pg_ref = state.pg_store.read().clone();
            let Some(ref pg) = pg_ref else { continue };

            let trace_path = trace_log::trace_log_path();
            match read_new_entries(&trace_path, last_offset) {
                Ok((entries, new_offset)) if !entries.is_empty() => {
                    match pg.insert_trace_logs(&entries).await {
                        Ok(()) => {
                            info!(
                                count = entries.len(),
                                "PG sync: wrote new trace log entries"
                            );
                            last_offset = new_offset;
                        }
                        Err(e) => {
                            warn!(error = %e, "PG sync: failed to insert trace logs");
                        }
                    }
                }
                Ok((_, new_offset)) => {
                    last_offset = new_offset;
                }
                Err(e) => {
                    warn!(error = %e, "PG sync: failed to read trace log");
                }
            }
        }
    });
}

/// Read trace log entries starting from `byte_offset` in the JSONL file.
/// Returns the parsed entries and the new byte offset after reading.
fn read_new_entries(
    path: &str,
    byte_offset: u64,
) -> Result<(Vec<TraceLogEntry>, u64), std::io::Error> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek, SeekFrom};

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // Seek to the last known position.
    // If the file has been rotated (offset > file size), reset to 0.
    if byte_offset > 0 {
        let file_len = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        if byte_offset <= file_len {
            reader.seek(SeekFrom::Start(byte_offset))?;
        }
        // else: file was rotated/smaller, start from beginning
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

        match serde_json::from_str::<TraceLogEntry>(trimmed) {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // Skip malformed lines.
            }
        }
    }

    let new_offset = reader.stream_position()?;
    Ok((entries, new_offset))
}
