//! Shared API key masking utilities.

use regex::Regex;
use std::sync::OnceLock;

static SK_RE: OnceLock<Regex> = OnceLock::new();

fn sk_regex() -> &'static Regex {
    SK_RE.get_or_init(|| Regex::new(r"sk-[a-zA-Z0-9_-]{5,}").expect("valid regex"))
}

/// Mask `sk-*` / `sk-cc-*` bearer tokens with partial reveal.
///
/// For tokens longer than 8 chars, reveals first 4 and last 4 (e.g. `sk-c...k1l2`).
/// For shorter tokens, reveals first char only (e.g. `s***`).
///
/// Uses regex matching, which correctly handles multi-byte UTF-8 characters
/// (unlike byte-level implementations that may truncate UTF-8 sequences).
pub fn mask_api_keys(text: &str) -> String {
    let re = sk_regex();
    re.replace_all(text, |caps: &regex::Captures| {
        let token = &caps[0];
        if token.len() > 8 {
            format!("{}...{}", &token[..4], &token[token.len() - 4..])
        } else {
            format!("{}***", &token[..1])
        }
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_long_token() {
        let text = r#"{"api_key": "sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2"}"#;
        let masked = mask_api_keys(text);
        assert!(!masked.contains("sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2"));
        assert!(masked.contains("sk-c"));
        assert!(masked.contains("k1l2"));
    }

    #[test]
    fn test_mask_short_token() {
        let text = "key=sk-short";
        let masked = mask_api_keys(text);
        assert!(!masked.contains("sk-short"));
        assert!(masked.contains("s***"));
    }

    #[test]
    fn test_no_match() {
        let text = "no keys here";
        assert_eq!(mask_api_keys(text), text);
    }

    #[test]
    fn test_multiple_tokens() {
        let text = "sk-aaaabbbbccccdddd and sk-1111222233334444";
        let masked = mask_api_keys(text);
        assert!(!masked.contains("aaaabbbbccccdddd"));
        assert!(!masked.contains("1111222233334444"));
    }

    #[test]
    fn test_unicode_safety() {
        let text = "中文sk-cc-a1b2c3d4e5f6g7h8i9j0k1l2中文";
        let masked = mask_api_keys(text);
        assert!(masked.starts_with("中文"));
        assert!(masked.ends_with("中文"));
        assert!(!masked.contains("a1b2c3d4e5f6g7h8i9j0k1l2"));
    }
}
