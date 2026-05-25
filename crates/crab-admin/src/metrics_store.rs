//! SQLite-backed cold storage for Admin metrics history snapshots.
//!
//! Each Prometheus counter snapshot (scraped every 60 seconds) is persisted
//! here so that restarting `crab-admin` does not clear the Dashboard curves.

use crate::metrics_history::MetricsCounterSnapshot;
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;
use tracing::warn;

const DB_PATH_ENV: &str = "CRABCACHE_ADMIN_METRICS_DB_PATH";
const RETENTION_ENV: &str = "CRABCACHE_METRICS_DB_RETENTION_SECS";

/// Default retention: 30 days.
const DEFAULT_RETENTION_SECS: u64 = 30 * 86400;

pub struct MetricsStore {
    conn: Mutex<Connection>,
}

impl MetricsStore {
    /// Open (or create) the SQLite database at the configured path.
    /// Creates the `metrics_snapshots` table if it does not exist.
    pub fn open() -> anyhow::Result<Self> {
        let path = Self::db_path();
        if let Some(parent) = Path::new(&path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS metrics_snapshots (
                sampled_at INTEGER PRIMARY KEY,
                gateway_uptime_secs INTEGER NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_metrics_snapshots_at
                ON metrics_snapshots(sampled_at);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (for tests).
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS metrics_snapshots (
                sampled_at INTEGER PRIMARY KEY,
                gateway_uptime_secs INTEGER NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_metrics_snapshots_at
                ON metrics_snapshots(sampled_at);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn db_path() -> String {
        std::env::var(DB_PATH_ENV).unwrap_or_else(|_| "data/metrics.sqlite".to_string())
    }

    pub fn retention_secs() -> u64 {
        std::env::var(RETENTION_ENV)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_RETENTION_SECS)
    }

    /// Insert a single snapshot.  If `sampled_at` already exists the row is
    /// replaced (idempotent).
    pub fn insert_snapshot(&self, snapshot: &MetricsCounterSnapshot, gateway_uptime_secs: u64) {
        let payload = match serde_json::to_string(snapshot) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to serialize snapshot");
                return;
            }
        };
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Metrics store mutex poisoned");
                return;
            }
        };
        if let Err(e) = conn.execute(
            "INSERT OR REPLACE INTO metrics_snapshots(sampled_at, gateway_uptime_secs, payload)
             VALUES (?1, ?2, ?3)",
            params![
                snapshot.sampled_at as i64,
                gateway_uptime_secs as i64,
                payload
            ],
        ) {
            warn!(error = %e, "Failed to persist metrics snapshot");
        }
    }

    /// Load all snapshots with `sampled_at >= cutoff_ts`, ordered ascending.
    pub fn load_snapshots_since(&self, cutoff_ts: u64) -> Vec<MetricsCounterSnapshot> {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Metrics store mutex poisoned");
                return Vec::new();
            }
        };
        let mut stmt = match conn.prepare(
            "SELECT payload FROM metrics_snapshots WHERE sampled_at >= ?1 ORDER BY sampled_at ASC",
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to prepare load query");
                return Vec::new();
            }
        };
        let rows = match stmt.query_map(params![cutoff_ts as i64], |row| {
            let payload: String = row.get(0)?;
            Ok(payload)
        }) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "Failed to query snapshots");
                return Vec::new();
            }
        };
        rows.filter_map(|r| match r {
            Ok(payload) => serde_json::from_str(&payload).ok(),
            Err(e) => {
                warn!(error = %e, "Error reading snapshot row");
                None
            }
        })
        .collect()
    }

    /// Return the `gateway_uptime_secs` column of the most recent row, or
    /// `None` if the database is empty.
    pub fn last_gateway_uptime(&self) -> Option<u64> {
        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare("SELECT gateway_uptime_secs FROM metrics_snapshots ORDER BY sampled_at DESC LIMIT 1")
            .ok()?;
        stmt.query_row([], |row| row.get::<_, i64>(0))
            .ok()
            .map(|v| v as u64)
    }

    /// Delete rows older than `cutoff_ts`.
    pub fn prune_older_than(&self, cutoff_ts: u64) {
        let conn = match self.conn.lock() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Metrics store mutex poisoned");
                return;
            }
        };
        if let Err(e) = conn.execute(
            "DELETE FROM metrics_snapshots WHERE sampled_at < ?1",
            params![cutoff_ts as i64],
        ) {
            warn!(error = %e, "Failed to prune old snapshots");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics_history::MetricsCounterSnapshot;

    fn make_snapshot(ts: u64) -> MetricsCounterSnapshot {
        MetricsCounterSnapshot {
            sampled_at: ts,
            ..Default::default()
        }
    }

    #[test]
    fn insert_and_load_roundtrip() {
        let store = MetricsStore::open_in_memory().unwrap();
        let s1 = make_snapshot(1000);
        let s2 = make_snapshot(2000);
        store.insert_snapshot(&s1, 3600);
        store.insert_snapshot(&s2, 7200);

        let loaded = store.load_snapshots_since(0);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].sampled_at, 1000);
        assert_eq!(loaded[1].sampled_at, 2000);
    }

    #[test]
    fn load_since_cutoff() {
        let store = MetricsStore::open_in_memory().unwrap();
        store.insert_snapshot(&make_snapshot(100), 100);
        store.insert_snapshot(&make_snapshot(200), 100);
        store.insert_snapshot(&make_snapshot(300), 100);

        let loaded = store.load_snapshots_since(150);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].sampled_at, 200);
        assert_eq!(loaded[1].sampled_at, 300);
    }

    #[test]
    fn last_gateway_uptime_empty() {
        let store = MetricsStore::open_in_memory().unwrap();
        assert!(store.last_gateway_uptime().is_none());
    }

    #[test]
    fn last_gateway_uptime_returns_latest() {
        let store = MetricsStore::open_in_memory().unwrap();
        store.insert_snapshot(&make_snapshot(100), 500);
        store.insert_snapshot(&make_snapshot(200), 1000);
        assert_eq!(store.last_gateway_uptime(), Some(1000));
    }

    #[test]
    fn prune_removes_old() {
        let store = MetricsStore::open_in_memory().unwrap();
        store.insert_snapshot(&make_snapshot(100), 0);
        store.insert_snapshot(&make_snapshot(200), 0);
        store.insert_snapshot(&make_snapshot(300), 0);

        store.prune_older_than(150);
        let loaded = store.load_snapshots_since(0);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].sampled_at, 200);
    }

    #[test]
    fn idempotent_insert() {
        let store = MetricsStore::open_in_memory().unwrap();
        store.insert_snapshot(&make_snapshot(100), 50);
        store.insert_snapshot(&make_snapshot(100), 100);

        let loaded = store.load_snapshots_since(0);
        assert_eq!(loaded.len(), 1);
    }
}
