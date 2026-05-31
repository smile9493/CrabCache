//! Map downstream display names (Cursor / ChatGPT UI) to Codex upstream slugs.

/// Normalize client model for alias lookup: trim, ASCII lower, collapse whitespace.
pub fn normalize_client_model_key(model: &str) -> String {
    model
        .trim()
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `(normalized display name → upstream slug)` — case/spacing insensitive on lookup key.
const CODEX_DISPLAY_ALIASES: &[(&str, &str)] = &[
    // GPT-branded (ChatGPT UI)
    ("gpt-5.5", "gpt-5.5"),
    ("gpt 5.5", "gpt-5.5"),
    ("gpt-5.4", "gpt-5.4"),
    ("gpt 5.4", "gpt-5.4"),
    ("gpt-5.2", "gpt-5.2"),
    ("gpt 5.2", "gpt-5.2"),
    ("gpt-5.3", "gpt-5.3-codex"),
    ("gpt 5.3", "gpt-5.3-codex"),
    ("gpt-5.1", "gpt-5.1-codex"),
    ("gpt 5.1", "gpt-5.1-codex"),
    ("gpt-5", "gpt-5"),
    ("gpt 5", "gpt-5"),
    // Codex-branded (Cursor model picker)
    ("codex 5.3", "gpt-5.3-codex"),
    ("codex 5.2", "gpt-5.2-codex"),
    ("codex 5.1 max", "gpt-5.1-codex-max"),
    ("codex 5.1", "gpt-5.1-codex"),
    ("codex 5.4", "gpt-5.4"),
    ("codex 5.5", "gpt-5.5"),
    // Legacy OpenAI API names (passthrough canonical slug)
    ("gpt-5-codex", "gpt-5-codex"),
    ("gpt-5.1-codex", "gpt-5.1-codex"),
    ("gpt-5.1-codex-max", "gpt-5.1-codex-max"),
    ("gpt-5.2-codex", "gpt-5.2-codex"),
    ("gpt-5.3-codex", "gpt-5.3-codex"),
    ("gpt-5.3-codex-spark", "gpt-5.3-codex-spark"),
];

/// Resolve a display-style model name to a Codex upstream slug, if known.
pub fn resolve_codex_display_alias(model: &str) -> Option<&'static str> {
    let key = normalize_client_model_key(model);
    CODEX_DISPLAY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == key)
        .map(|(_, slug)| *slug)
}

/// Rewrite client `model` to canonical slug when it matches a known display alias.
pub fn canonicalize_client_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    resolve_codex_display_alias(trimmed)
        .unwrap_or(trimmed)
        .to_string()
}

/// True when the model looks like a GPT/Codex display name (for profile routing).
pub fn is_openai_or_codex_display_model(model: &str) -> bool {
    let lower = normalize_client_model_key(model);
    lower.starts_with("gpt")
        || lower.starts_with("gpt-")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("chatgpt-")
        || lower.starts_with("codex ")
        || lower.starts_with("codex-")
        || lower == "codex"
        || resolve_codex_display_alias(model).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_cursor_display_names_case_insensitive() {
        assert_eq!(canonicalize_client_model("GPT-5.5"), "gpt-5.5");
        assert_eq!(canonicalize_client_model("Codex 5.2"), "gpt-5.2-codex");
        assert_eq!(
            canonicalize_client_model("Codex 5.1 Max"),
            "gpt-5.1-codex-max"
        );
        assert_eq!(canonicalize_client_model("codex 5.3"), "gpt-5.3-codex");
    }

    #[test]
    fn unknown_model_passthrough() {
        assert_eq!(
            canonicalize_client_model("deepseek-v4-pro"),
            "deepseek-v4-pro"
        );
    }

    #[test]
    fn codex_display_routes_as_openai_family() {
        assert!(is_openai_or_codex_display_model("Codex 5.2"));
        assert!(is_openai_or_codex_display_model("GPT-5.5"));
    }
}
