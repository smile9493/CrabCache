//! Multi-tenant project resolution and DeepSeek `user_id` sanitization.

const USER_ID_MAX_LEN: usize = 512;

fn is_valid_user_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectResolveError {
    /// Key-bound `project_id` does not match `X-Project-Id`.
    Mismatch,
    /// Header value failed sanitization.
    InvalidHeader(String),
}

impl std::fmt::Display for ProjectResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mismatch => write!(f, "project id mismatch"),
            Self::InvalidHeader(e) => write!(f, "invalid project id: {e}"),
        }
    }
}

impl From<String> for ProjectResolveError {
    fn from(e: String) -> Self {
        Self::InvalidHeader(e)
    }
}

/// Validate and return a DeepSeek-compatible `user_id` / `project_id` string.
pub fn sanitize_user_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("project id must not be empty".to_string());
    }
    if trimmed.len() > USER_ID_MAX_LEN {
        return Err(format!(
            "project id exceeds maximum length of {USER_ID_MAX_LEN}"
        ));
    }
    if !trimmed.chars().all(is_valid_user_id_char) {
        return Err(
            "project id must match [a-zA-Z0-9\\-_]+ and must not contain spaces or other characters"
                .to_string(),
        );
    }
    Ok(trimmed.to_string())
}

/// Resolve the effective project id for this request (server-authoritative).
pub fn resolve_project_id(
    key_project_id: Option<&str>,
    header: Option<&str>,
) -> Result<Option<String>, ProjectResolveError> {
    let key = key_project_id
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_user_id)
        .transpose()
        .map_err(ProjectResolveError::InvalidHeader)?;

    let header_val = header
        .filter(|s| !s.trim().is_empty())
        .map(sanitize_user_id)
        .transpose()
        .map_err(ProjectResolveError::InvalidHeader)?;

    match (key.as_deref(), header_val.as_deref()) {
        (Some(k), None) => Ok(Some(k.to_string())),
        (Some(k), Some(h)) if k == h => Ok(Some(k.to_string())),
        (Some(_), Some(_)) => Err(ProjectResolveError::Mismatch),
        (None, Some(h)) => Ok(Some(h.to_string())),
        (None, None) => Ok(None),
    }
}

/// Combine global config namespace with per-request `project_id`.
pub fn effective_cache_namespace(global: Option<&str>, project: Option<&str>) -> Option<String> {
    let g = global.filter(|s| !s.trim().is_empty());
    let p = project.filter(|s| !s.trim().is_empty());
    match (g, p) {
        (Some(g), Some(p)) => Some(format!("{g}:{p}")),
        (_, Some(p)) => Some(p.to_string()),
        (Some(g), None) => Some(g.to_string()),
        (None, None) => None,
    }
}

/// Derive a stable `project_id` from a client API key (sk-cc-*).
/// Format: `client_{sha256_hex_16}` — conforms to `sanitize_user_id` regex.
pub fn derive_project_id_from_client_key(client_key: &str) -> Result<String, ProjectResolveError> {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(client_key.as_bytes());
    let hex16 = hex::encode(hash)[..16].to_string();
    let derived = format!("client_{hex16}");
    sanitize_user_id(&derived).map_err(ProjectResolveError::InvalidHeader)?;
    Ok(derived)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_valid() {
        assert_eq!(sanitize_user_id("proj-A_1").unwrap(), "proj-A_1");
    }

    #[test]
    fn sanitize_rejects_invalid() {
        assert!(sanitize_user_id("proj A").is_err());
        assert!(sanitize_user_id("").is_err());
    }

    #[test]
    fn resolve_key_only() {
        assert_eq!(
            resolve_project_id(Some("alpha"), None).unwrap(),
            Some("alpha".to_string())
        );
    }

    #[test]
    fn resolve_mismatch() {
        assert_eq!(
            resolve_project_id(Some("alpha"), Some("beta")),
            Err(ProjectResolveError::Mismatch)
        );
    }

    #[test]
    fn resolve_header_only() {
        assert_eq!(
            resolve_project_id(None, Some("beta")).unwrap(),
            Some("beta".to_string())
        );
    }

    #[test]
    fn effective_namespace_composes() {
        assert_eq!(
            effective_cache_namespace(Some("global"), Some("proj")),
            Some("global:proj".to_string())
        );
        assert_eq!(
            effective_cache_namespace(None, Some("proj")),
            Some("proj".to_string())
        );
    }
}
