pub mod analyze;
pub mod session;
pub mod time;
pub mod types;

pub use analyze::{analyze_packet, diff_structure};
pub use session::{affinity_kind_from_key, fingerprint_bytes, session_fingerprint_from_payload};
pub use time::{
    format_beijing_datetime_ms, format_beijing_datetime_secs_ms, format_beijing_hour_label,
    normalize_epoch_ms,
};
pub use types::{
    CaptureRequestMeta, MessageStructure, PacketStructureSummary, RawCaptureEntry, RoleCounts,
    StructureDiff,
};
