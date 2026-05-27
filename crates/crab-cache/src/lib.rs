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

pub use coalescing::{CoalesceError, CoalesceGuard, RequestCoalescer};
pub use hit_rate_sim::{
    CacheStats, SemanticCache, SimEmbedder, SimulatedCache, SweepResult, parameter_sweep,
    simulate_cache_hit_rate, simulate_cache_with_l2,
};
pub use key::{
    FingerprintConfig, generate_cache_key, generate_cache_key_with_fingerprint,
    generate_composite_cache_key, generate_composite_cache_key_from_value,
    generate_namespaced_cache_key, generate_namespaced_cache_key_with_fingerprint,
};
pub use sanitized_trace::{
    FittedParameters, SanitizedLogEntry, load_sanitized_log, save_sanitized_log,
};
pub use tiered::{CacheError, InvalidateScanOptions, TieredCache};
pub use trace_analyzer::{
    ComparisonResult, TraceRecord, TraceStats, load_trace_from_file, save_trace_to_file,
};
pub use trace_loader::{LoadPattern, TraceEvent, TraceGenerator};
pub use types::{CacheEntry, L0Config, TtlConfig, UsageInfo};
