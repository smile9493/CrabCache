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
}
