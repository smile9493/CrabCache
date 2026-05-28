//! Fast extraction of common JSON fields without full `serde_json` parse.

use memchr::memmem;

/// Fields needed for pipeline selection and affinity before full JSON parse.
#[derive(Debug, Clone, Default)]
pub struct QuickRequestFields {
    pub model: Option<String>,
    pub stream: Option<bool>,
    pub conversation_id: Option<String>,
    pub prompt_cache_key: Option<String>,
}

/// Extract `"model"`, `"stream"`, `"conversation_id"`, `"prompt_cache_key"` via substring search.
///
/// Falls back to partial results on malformed JSON; caller must full-parse when required.
pub fn quick_parse_request_fields(body: &[u8]) -> QuickRequestFields {
    let s = match std::str::from_utf8(body) {
        Ok(v) => v,
        Err(_) => return QuickRequestFields::default(),
    };

    QuickRequestFields {
        model: extract_json_string_value(s, "\"model\""),
        stream: extract_json_bool_value(s, "\"stream\""),
        conversation_id: extract_json_string_value(s, "\"conversation_id\""),
        prompt_cache_key: extract_json_string_value(s, "\"prompt_cache_key\""),
    }
}

fn extract_json_string_value(haystack: &str, key_literal: &str) -> Option<String> {
    let start = memmem::find(haystack.as_bytes(), key_literal.as_bytes())?;
    let after_key = &haystack[start + key_literal.len()..];
    let colon = memchr::memchr(b':', after_key.as_bytes())?;
    let mut rest = after_key[colon + 1..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    rest = &rest[1..];
    let end = rest.find('"')?;
    let raw = &rest[..end];
    if raw.is_empty() {
        return None;
    }
    Some(raw.to_string())
}

fn extract_json_bool_value(haystack: &str, key_literal: &str) -> Option<bool> {
    let start = memmem::find(haystack.as_bytes(), key_literal.as_bytes())?;
    let after_key = &haystack[start + key_literal.len()..];
    let colon = memchr::memchr(b':', after_key.as_bytes())?;
    let rest = after_key[colon + 1..].trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_parse_extracts_model_and_stream() {
        let body = br#"{"model":"mimo-v2","stream":true,"messages":[]}"#;
        let q = quick_parse_request_fields(body);
        assert_eq!(q.model.as_deref(), Some("mimo-v2"));
        assert_eq!(q.stream, Some(true));
    }

    #[test]
    fn quick_parse_extracts_optional_ids() {
        let body =
            br#"{"model":"x","conversation_id":"c1","prompt_cache_key":"pck","stream":false}"#;
        let q = quick_parse_request_fields(body);
        assert_eq!(q.conversation_id.as_deref(), Some("c1"));
        assert_eq!(q.prompt_cache_key.as_deref(), Some("pck"));
        assert_eq!(q.stream, Some(false));
    }

    #[test]
    fn quick_parse_reversed_field_order() {
        let body =
            br#"{"stream":false,"prompt_cache_key":"pck-rev","conversation_id":"c-rev","model":"deepseek-v4-pro","messages":[]}"#;
        let q = quick_parse_request_fields(body);
        assert_eq!(q.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(q.stream, Some(false));
        assert_eq!(q.conversation_id.as_deref(), Some("c-rev"));
        assert_eq!(q.prompt_cache_key.as_deref(), Some("pck-rev"));
    }

    #[test]
    fn quick_parse_model_with_escaped_quotes_in_nearby_value() {
        // Model value itself doesn't contain quotes, but a later string value does.
        // This tests that the parser doesn't bleed across fields.
        let body = br#"{"model":"mimo-v2","messages":[{"role":"user","content":"He said \"hello\" to me"}]}"#;
        let q = quick_parse_request_fields(body);
        assert_eq!(q.model.as_deref(), Some("mimo-v2"));
    }

    #[test]
    fn quick_parse_missing_model_returns_none() {
        let body = br#"{"stream":true,"messages":[]}"#;
        let q = quick_parse_request_fields(body);
        assert!(q.model.is_none());
        assert_eq!(q.stream, Some(true));
    }

    #[test]
    fn quick_parse_empty_string_fields_ignored() {
        let body = br#"{"model":"","conversation_id":"","prompt_cache_key":"pk","stream":true}"#;
        let q = quick_parse_request_fields(body);
        assert!(q.model.is_none(), "empty model string should be None");
        assert!(
            q.conversation_id.is_none(),
            "empty conversation_id should be None"
        );
        assert_eq!(q.prompt_cache_key.as_deref(), Some("pk"));
    }

    #[test]
    fn quick_parse_whitespace_around_colon() {
        let body = br#"{ "model" : "mimo-v2" , "stream" : true }"#;
        let q = quick_parse_request_fields(body);
        assert_eq!(q.model.as_deref(), Some("mimo-v2"));
        assert_eq!(q.stream, Some(true));
    }

    #[test]
    fn quick_parse_invalid_utf8_returns_defaults() {
        let body: &[u8] = &[0xFF, 0xFE, 0xFD];
        let q = quick_parse_request_fields(body);
        assert!(q.model.is_none());
        assert!(q.stream.is_none());
    }
}
