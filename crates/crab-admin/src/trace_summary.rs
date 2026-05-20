//! Trace log analysis helpers (shared by trace API and overview bundle).

use crate::trace_log::TraceLogEntry;
use crate::types::TraceSummary;

pub fn compute_trace_summary(entries: &[TraceLogEntry], hours: u32) -> TraceSummary {
    if entries.is_empty() {
        return TraceSummary {
            hours,
            total_requests: 0,
            cache_hit_ratio: 0.0,
        };
    }
    let total = entries.len();
    let cache_hits = entries.iter().filter(|e| e.cache_hit).count();
    TraceSummary {
        hours,
        total_requests: total,
        cache_hit_ratio: if total > 0 {
            cache_hits as f64 / total as f64
        } else {
            0.0
        },
    }
}
