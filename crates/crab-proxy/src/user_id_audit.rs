//! DeepSeek `user_id` trace audit helpers (shadow log fields).

use crab_pipeline::RequestPipeline;
use serde_json::Value;

/// Audit status for upstream `user_id` injection (serialized in trace JSONL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserIdAuditStatus {
    NotApplicable,
    Injected,
    Absent,
    StrippedClient,
    Mismatch,
}

impl UserIdAuditStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::Injected => "injected",
            Self::Absent => "absent",
            Self::StrippedClient => "stripped_client",
            Self::Mismatch => "mismatch",
        }
    }
}

pub fn parse_user_id_from_json(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("user_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn is_deepseek_pipeline(pipeline: Option<RequestPipeline>) -> bool {
    matches!(
        pipeline,
        Some(RequestPipeline::CursorDeepSeekV4) | Some(RequestPipeline::DeepSeekLight)
    )
}

/// Compute audit status for trace logging.
pub fn compute_user_id_audit(
    pipeline: Option<RequestPipeline>,
    project_id: Option<&str>,
    client_body_user_id: Option<&str>,
    upstream_user_id: Option<&str>,
) -> UserIdAuditStatus {
    if !is_deepseek_pipeline(pipeline) {
        return UserIdAuditStatus::NotApplicable;
    }

    let project = project_id.filter(|s| !s.is_empty());
    let upstream = upstream_user_id.filter(|s| !s.is_empty());
    let client = client_body_user_id.filter(|s| !s.is_empty());

    match (project, upstream, client) {
        (Some(pid), Some(uid), Some(cid)) if cid != uid => {
            if uid == pid {
                UserIdAuditStatus::StrippedClient
            } else {
                UserIdAuditStatus::Mismatch
            }
        }
        (Some(pid), Some(uid), _) if uid == pid => UserIdAuditStatus::Injected,
        (Some(_), Some(_), _) => UserIdAuditStatus::Mismatch,
        (Some(_), None, _) => UserIdAuditStatus::Absent,
        (None, None, Some(_)) => UserIdAuditStatus::StrippedClient,
        (None, None, None) => UserIdAuditStatus::Absent,
        (None, Some(_), _) => UserIdAuditStatus::Mismatch,
    }
}

/// Populate trace audit fields on a log entry from gateway context.
pub fn apply_user_id_audit_to_entry(
    entry: &mut crate::trace_logger::SanitizedLogEntry,
    pipeline: Option<RequestPipeline>,
    upstream_profile_id: Option<&str>,
    upstream_model: Option<&str>,
    project_id: Option<&str>,
    original_body: Option<&[u8]>,
    upstream_body: Option<&[u8]>,
) {
    entry.upstream_profile_id = upstream_profile_id
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    entry.pipeline = pipeline.map(|p| p.as_str().to_string());
    entry.upstream_model = upstream_model.filter(|s| !s.is_empty()).map(str::to_string);

    let client_body_user_id = original_body.and_then(parse_user_id_from_json);
    let upstream_user_id = upstream_body.and_then(parse_user_id_from_json);

    entry.client_body_user_id = client_body_user_id.clone();
    entry.upstream_user_id = upstream_user_id.clone();

    let status = compute_user_id_audit(
        pipeline,
        project_id,
        client_body_user_id.as_deref(),
        upstream_user_id.as_deref(),
    );
    entry.user_id_audit = Some(status.as_str().to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_when_project_matches_upstream() {
        let _body = br#"{"model":"deepseek-v4-pro","user_id":"tenant_a"}"#;
        let status = compute_user_id_audit(
            Some(RequestPipeline::CursorDeepSeekV4),
            Some("tenant_a"),
            Some("wrong"),
            Some("tenant_a"),
        );
        assert_eq!(status, UserIdAuditStatus::StrippedClient);

        let status = compute_user_id_audit(
            Some(RequestPipeline::DeepSeekLight),
            Some("tenant_a"),
            None,
            Some("tenant_a"),
        );
        assert_eq!(status, UserIdAuditStatus::Injected);
    }

    #[test]
    fn absent_without_project_and_upstream_user_id() {
        let status =
            compute_user_id_audit(Some(RequestPipeline::CursorDeepSeekV4), None, None, None);
        assert_eq!(status, UserIdAuditStatus::Absent);
    }

    #[test]
    fn mimo_pipeline_not_applicable() {
        let status = compute_user_id_audit(
            Some(RequestPipeline::MimoTokenPlanRelay),
            Some("tenant_a"),
            None,
            Some("tenant_a"),
        );
        assert_eq!(status, UserIdAuditStatus::NotApplicable);
    }

    #[test]
    fn parse_user_id_from_json_body() {
        let body = br#"{"user_id":"abc_1"}"#;
        assert_eq!(parse_user_id_from_json(body).as_deref(), Some("abc_1"));
    }
}
