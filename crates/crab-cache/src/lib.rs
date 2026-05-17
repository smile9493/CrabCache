mod coalescing;
mod hit_rate_sim;
mod key;
mod sanitized_trace;
mod tiered;
mod trace_analyzer;
mod trace_loader;
mod types;

#[cfg(test)]
pub mod mock;

pub use coalescing::{RequestCoalescer, CoalesceGuard};
pub use hit_rate_sim::{
    parameter_sweep, simulate_cache_hit_rate, simulate_cache_with_l2, CacheStats, SemanticCache,
    SimEmbedder, SimulatedCache, SweepResult,
};
pub use key::{
    generate_cache_key, generate_cache_key_with_fingerprint,
    generate_namespaced_cache_key, generate_namespaced_cache_key_with_fingerprint,
    FingerprintConfig,
};
pub use sanitized_trace::{
    load_sanitized_log, save_sanitized_log, FittedParameters, SanitizedLogEntry,
};
pub use tiered::{InvalidateScanOptions, TieredCache};
pub use trace_analyzer::{
    load_trace_from_file, save_trace_to_file, ComparisonResult, TraceRecord, TraceStats,
};
pub use trace_loader::{LoadPattern, TraceEvent, TraceGenerator};
pub use types::{CacheEntry, L0Config, TtlConfig, UsageInfo};


