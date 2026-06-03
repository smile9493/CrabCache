use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Unified client type — extensible via config without modifying pipeline selection logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Cursor,
    Codex,
    Windsurf,
    Aider,
    Continue,
    Generic,
}

impl ClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Windsurf => "windsurf",
            Self::Aider => "aider",
            Self::Continue => "continue",
            Self::Generic => "generic",
        }
    }
}

/// Unified client detector — replaces scattered Cursor/Codex detection logic.
///
/// Detection priority (highest first):
/// 1. P0: `/v1/responses` wire → `Codex` (unless User-Agent is explicitly Cursor)
/// 2. P0b: `/v1/responses` + GPT/Codex model → `Codex` (Cursor UA + GPT on Responses)
/// 3. P1: `X-Client-Kind` header → explicit declaration
/// 4. P2: User-Agent contains `cursor` → `Cursor`
/// 5. P3: User-Agent contains `codex_cli` → `Codex`
/// 6. P4–P6: windsurf / aider / continue User-Agent
/// 7. P7: Chat Completions payload signals (`tools` / `conversation_id` / …) → `Cursor`
/// 8. P8: fallback → `Generic`
pub struct ClientDetector;

impl ClientDetector {
    /// Detect client kind from request metadata.
    ///
    /// # Arguments
    /// * `path` — request URI path (e.g. `/v1/chat/completions`)
    /// * `user_agent` — `User-Agent` header value
    /// * `client_kind_header` — `X-Client-Kind` header value (explicit override)
    /// * `originator` — `Originator` header (Codex CLI sends `codex_cli_rs`)
    /// * `payload` — parsed JSON request body
    pub fn detect(
        path: &str,
        user_agent: Option<&str>,
        client_kind_header: Option<&str>,
        originator: Option<&str>,
        payload: Option<&Value>,
    ) -> ClientKind {
        // P0: /v1/responses wire — Codex CLI/Desktop unless explicitly Cursor UA.
        // Responses + `tools` must not fall through to Cursor payload heuristics (P7).
        if path.ends_with("/v1/responses") {
            if originator
                .is_some_and(|o| o.trim().eq_ignore_ascii_case("codex_cli_rs"))
            {
                return ClientKind::Codex;
            }
            if let Some(ua) = user_agent {
                let ua_lower = ua.to_ascii_lowercase();
                if ua_lower.contains("codex_cli") || ua_lower.contains("codex/") {
                    return ClientKind::Codex;
                }
                if !ua_lower.contains("cursor") {
                    return ClientKind::Codex;
                }
            } else {
                return ClientKind::Codex;
            }
        }

        // P0b: /v1/responses + GPT/Codex model (legacy path when Cursor UA present)
        if path.ends_with("/v1/responses") {
            if let Some(model) = payload_model(payload) {
                if is_codex_family_model(model) {
                    return ClientKind::Codex;
                }
            }
        }

        // P1: explicit header
        if let Some(header) = client_kind_header {
            let lower = header.trim().to_ascii_lowercase();
            match lower.as_str() {
                "cursor" => return ClientKind::Cursor,
                "codex" => return ClientKind::Codex,
                "windsurf" => return ClientKind::Windsurf,
                "aider" => return ClientKind::Aider,
                "continue" => return ClientKind::Continue,
                "generic" => return ClientKind::Generic,
                _ => {} // unknown value, fall through
            }
        }

        // P2-P6: User-Agent heuristics
        if let Some(ua) = user_agent {
            let ua_lower = ua.to_ascii_lowercase();
            if ua_lower.contains("cursor") {
                return ClientKind::Cursor;
            }
            if ua_lower.contains("codex_cli") {
                return ClientKind::Codex;
            }
            if ua_lower.contains("windsurf") {
                return ClientKind::Windsurf;
            }
            if ua_lower.contains("aider") {
                return ClientKind::Aider;
            }
            if ua_lower.contains("continue") {
                return ClientKind::Continue;
            }
        }

        // P7: payload signals (Cursor agent patterns) — Chat Completions wire only.
        if !path.ends_with("/v1/responses") && has_cursor_payload_signals(payload) {
            return ClientKind::Cursor;
        }

        // P8: fallback
        ClientKind::Generic
    }
}

fn payload_model(payload: Option<&Value>) -> Option<&str> {
    payload
        .and_then(|p| p.get("model"))
        .and_then(|m| m.as_str())
}

fn is_codex_family_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.starts_with("gpt-")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("chatgpt-")
        || lower.starts_with("codex")
}

/// Payload-based Cursor agent signal detection (consolidated from `signals.rs`).
pub(crate) fn has_cursor_payload_signals(payload: Option<&Value>) -> bool {
    let Some(payload) = payload else {
        return false;
    };

    if payload.get("tools").is_some() {
        return true;
    }
    if payload
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
    {
        return true;
    }
    if payload
        .get("prompt_cache_key")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
    {
        return true;
    }

    let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };

    messages.iter().any(|msg| {
        msg.get("tool_calls").is_some() || msg.get("reasoning_content").is_some()
    })
}

/// Simple glob pattern match for model names.
///
/// Supports `*` wildcard at the end (prefix match) or beginning (suffix match).
/// Examples: `"deepseek-v4-*"`, `"*-codex"`, `"mimo-*"`.
pub fn matches_model_pattern(model: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return model.ends_with(suffix);
    }
    model == pattern
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cursor_user_agent() {
        assert_eq!(
            ClientDetector::detect(
                "/v1/chat/completions",
                Some("Cursor/0.45.0"),
                None,
                None,
                None,
            ),
            ClientKind::Cursor,
        );
    }

    #[test]
    fn codex_user_agent() {
        assert_eq!(
            ClientDetector::detect(
                "/v1/chat/completions",
                Some("codex_cli/1.0"),
                None,
                None,
                None,
            ),
            ClientKind::Codex,
        );
    }

    #[test]
    fn explicit_header_overrides_ua() {
        assert_eq!(
            ClientDetector::detect(
                "/v1/chat/completions",
                Some("Mozilla/5.0"),
                Some("cursor"),
                None,
                None,
            ),
            ClientKind::Cursor,
        );
    }

    #[test]
    fn responses_endpoint_codex_model() {
        let payload = json!({"model": "gpt-5", "messages": []});
        assert_eq!(
            ClientDetector::detect("/v1/responses", None, None, None, Some(&payload)),
            ClientKind::Codex,
        );
    }

    #[test]
    fn responses_endpoint_non_codex_model() {
        let payload = json!({"model": "deepseek-v4-pro", "input": []});
        assert_eq!(
            ClientDetector::detect("/v1/responses", None, None, None, Some(&payload)),
            ClientKind::Codex,
        );
    }

    #[test]
    fn responses_mimo_with_tools_is_codex_not_cursor() {
        let payload = json!({
            "model": "mimo-v2.5-pro",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
            "tools": []
        });
        assert_eq!(
            ClientDetector::detect(
                "/v1/responses",
                Some("codex_cli_rs/0.133.0"),
                None,
                Some("codex_cli_rs"),
                Some(&payload),
            ),
            ClientKind::Codex,
        );
    }

    #[test]
    fn responses_cursor_ua_mimo_stays_cursor() {
        let payload = json!({"model": "mimo-v2.5-pro", "input": []});
        assert_eq!(
            ClientDetector::detect(
                "/v1/responses",
                Some("Cursor/1.0.0"),
                None,
                None,
                Some(&payload),
            ),
            ClientKind::Cursor,
        );
    }

    #[test]
    fn tools_in_payload_signals_cursor() {
        let payload = json!({"model": "deepseek-v4-pro", "tools": [], "messages": []});
        assert_eq!(
            ClientDetector::detect("/v1/chat/completions", None, None, None, Some(&payload)),
            ClientKind::Cursor,
        );
    }

    #[test]
    fn conversation_id_signals_cursor() {
        let payload = json!({"model": "mimo-v2.5-pro", "conversation_id": "abc"});
        assert_eq!(
            ClientDetector::detect("/v1/chat/completions", None, None, None, Some(&payload)),
            ClientKind::Cursor,
        );
    }

    #[test]
    fn generic_fallback() {
        let payload = json!({"model": "deepseek-chat", "messages": [{"role": "user", "content": "hi"}]});
        assert_eq!(
            ClientDetector::detect(
                "/v1/chat/completions",
                Some("curl/7.0"),
                None,
                None,
                Some(&payload),
            ),
            ClientKind::Generic,
        );
    }

    #[test]
    fn model_pattern_prefix() {
        assert!(matches_model_pattern("deepseek-v4-pro", "deepseek-v4-*"));
        assert!(matches_model_pattern("deepseek-v4-lite", "deepseek-v4-*"));
        assert!(!matches_model_pattern("deepseek-chat", "deepseek-v4-*"));
    }

    #[test]
    fn model_pattern_exact() {
        assert!(matches_model_pattern("gpt-5", "gpt-5"));
        assert!(!matches_model_pattern("gpt-5.1", "gpt-5"));
    }

    #[test]
    fn model_pattern_wildcard_all() {
        assert!(matches_model_pattern("anything", "*"));
    }

    #[test]
    fn windsurf_user_agent() {
        assert_eq!(
            ClientDetector::detect("/v1/chat/completions", Some("Windsurf/1.0"), None, None, None),
            ClientKind::Windsurf,
        );
    }
}
