pub mod aggregate;
pub mod cursor_heuristics;
pub mod extract;
pub mod hash;
pub mod types;

pub use aggregate::{aggregate_composition, CompositionSummary};
pub use extract::{extract_composition, extract_system_text, extract_tools_json, CompositionHints};
pub use hash::{fingerprint_bytes, immutable_prefix_block_hash, tool_names_hash};
pub use types::{ComponentFingerprint, CompositionDebugEntry, CursorComponents, RequestComposition, RoleCounts};
