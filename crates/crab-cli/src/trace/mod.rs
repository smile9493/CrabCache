//! Trace JSONL analysis subcommands.

pub mod cache;
pub mod compare;
pub mod key_distribution;
pub mod latency;

pub use cache::print_analysis as print_cache;
pub use compare::{print_compare, print_compare_at};
pub use key_distribution::print_analysis as print_keys;
pub use latency::print_analysis as print_latency;
