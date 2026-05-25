pub mod aggregate;
pub mod cursor_heuristics;
pub mod extract;
pub mod hash;
pub mod types;

pub use aggregate::{CompositionSummary, aggregate_composition};
pub use extract::{CompositionHints, extract_composition, extract_system_text, extract_tools_json};
pub use hash::{fingerprint_bytes, immutable_prefix_block_hash, tool_names_hash};
pub use types::{
    ComponentFingerprint, CompositionDebugEntry, CursorComponents, RequestComposition, RoleCounts,
};
