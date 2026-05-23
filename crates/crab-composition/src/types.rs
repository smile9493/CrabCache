use serde::{Deserialize, Serialize};

/// Per-component fingerprint for cursor-specific prompt constructs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentFingerprint {
    /// Whether the component was detected in the request.
    pub present: bool,
    /// SHA256 prefix fingerprint of the detected content segment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

/// Cursor-specific agent constructs detected in system messages.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CursorComponents {
    /// Workspace rules / `always_applied_workspace_rules` / `.cursor/rules`.
    pub rules: ComponentFingerprint,
    /// `available_skills` / `SKILL.md` / `agent_skill`.
    pub skills: ComponentFingerprint,
    /// `mcpServers` / `CallMcpTool` / `mcp_file_system`.
    pub mcp: ComponentFingerprint,
    /// `subagent_type` / `Task tool` / `Launch.*agent`.
    pub subagent: ComponentFingerprint,
}

/// Per-role message counts in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleCounts {
    pub system: u32,
    pub user: u32,
    pub assistant: u32,
    pub tool: u32,
}

/// Structured fingerprint of a request's composition, extracted from the
/// OpenAI-compatible JSON payload. Stored in trace logs for offline analysis.
/// **Never stores raw message content** -- only hashes and counts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestComposition {
    // --- Identity dimensions (from GatewayContext) ---
    pub consumer: String,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default)]
    pub pipeline: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    // --- Model ---
    pub client_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,

    // --- System prefix block ---
    /// SHA256 of leading system messages + tools JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prefix_hash: Option<String>,
    /// Number of consecutive leading system messages.
    pub system_message_count: u32,
    /// Total character count of system messages content.
    pub system_chars: u32,

    // --- Tools ---
    pub tool_count: u32,
    /// SHA256 of sorted tool names (empty string if no tools).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_names_hash: Option<String>,
    pub has_tools: bool,

    // --- Conversation history ---
    pub message_count: u32,
    pub roles: RoleCounts,
    /// Messages where role == "tool" (tool result).
    pub tool_turn_count: u32,
    /// Assistant messages that contain tool_calls.
    pub assistant_with_tool_calls_count: u32,

    // --- Cursor agent constructs ---
    pub components: CursorComponents,
}
