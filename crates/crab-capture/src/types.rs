use serde::{Deserialize, Serialize};

/// Per-message structural info extracted from an OpenAI-compatible payload.
/// Stores no raw content -- only role, lengths, and heuristic flags.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageStructure {
    pub role: String,
    /// "text" | "array" | "null" | "missing"
    #[serde(default)]
    pub content_kind: String,
    /// Character length of string content, or sum of text parts for array content.
    #[serde(default)]
    pub content_chars: u64,
    /// Character length of `reasoning_content` field (DeepSeek V4).
    #[serde(default)]
    pub reasoning_content_chars: u64,
    #[serde(default)]
    pub has_tool_calls: bool,
    #[serde(default)]
    pub tool_calls_count: u32,
    /// `name` field for tool result messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Content contains `<thinking>` or `<details>` thinking markup.
    #[serde(default)]
    pub has_thinking_markup: bool,
}

/// Structural summary of a single OpenAI-compatible request packet.
/// No raw content is stored -- only counts, lengths, and heuristic flags.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PacketStructureSummary {
    pub message_count: u32,
    pub system_message_count: u32,
    pub system_chars: u64,
    pub tool_count: u32,
    pub has_tools: bool,
    pub roles: RoleCounts,
    pub tool_turn_count: u32,
    pub assistant_with_tool_calls_count: u32,
    pub total_content_chars: u64,
    pub total_reasoning_content_chars: u64,
    /// Any message contains `reasoning_content` field (DeepSeek V4).
    pub has_reasoning_content: bool,
    /// Any message content contains `<thinking>` / `<details>` markup.
    pub has_thinking_markup: bool,
    pub messages: Vec<MessageStructure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleCounts {
    pub system: u32,
    pub user: u32,
    pub assistant: u32,
    pub tool: u32,
}

/// Diff between client and upstream packet structure, focusing on
/// reasoning-related changes that cause context explosion.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StructureDiff {
    pub client: PacketStructureSummary,
    pub upstream: PacketStructureSummary,
    /// upstream.message_count - client.message_count
    pub delta_message_count: i32,
    /// upstream.total_content_chars - client.total_content_chars
    pub delta_content_chars: i64,
    /// upstream.total_reasoning_content_chars - client.total_reasoning_content_chars
    pub delta_reasoning_chars: i64,
    /// upstream.system_chars - client.system_chars
    pub delta_system_chars: i64,
    /// upstream.tool_count - client.tool_count
    pub delta_tool_count: i32,
    /// Whether upstream gained new messages compared to client.
    pub upstream_has_more_messages: bool,
    /// Whether upstream has reasoning content that client didn't.
    pub reasoning_was_injected: bool,
}

/// Per-request routing / session / timing metadata (from gateway at logging).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureRequestMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    /// First `user` message fingerprint — groups Cursor chat threads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_fingerprint: Option<String>,
    /// `user` field from JSON body (upstream / Cursor account id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_user: Option<String>,
    /// Ketama affinity prefix: `conv` | `pck` | `user` | `ip`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity_key: Option<String>,
    /// Selected upstream backend (load balancing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_host: Option<String>,
    /// SHA-256 prefix of client Bearer token (same key → same value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_key_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_tier: Option<String>,
    #[serde(default)]
    pub cache_hit: bool,
    #[serde(default)]
    pub coalesced_follower: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coalesce_leader: Option<bool>,
    #[serde(default)]
    pub duration_ms: u64,
    /// Request start → upstream response headers (MiMo prefill).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefill_ms: Option<u64>,
    /// Response headers → first upstream body chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
}

/// Complete raw capture entry written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCaptureEntry {
    pub timestamp_ms: u64,
    /// Beijing display time; filled by Admin API for the dashboard (not in index.jsonl).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_beijing: Option<String>,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_hash: Option<String>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<String>,
    pub stream: bool,
    pub client_body_bytes: u64,
    pub upstream_body_bytes: u64,
    /// upstream_body_bytes - client_body_bytes (positive = upstream is larger).
    pub delta_bytes: i64,
    pub structure: StructureDiff,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retired_prefix_messages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_strategy: Option<String>,
    /// Relative path to client body file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_path: Option<String>,
    /// Relative path to upstream body file (None if identical to client).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_path: Option<String>,
    /// Set when JSON parse or write fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_error: Option<String>,

    // ── Session / LB / perf (optional for older index lines) ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_key_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_tier: Option<String>,
    #[serde(default)]
    pub cache_hit: bool,
    #[serde(default)]
    pub coalesced_follower: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coalesce_leader: Option<bool>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefill_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_latency_ms: Option<f64>,
}
