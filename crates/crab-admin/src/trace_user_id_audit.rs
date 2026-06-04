//! DeepSeek `user_id` isolation audit from shadow trace logs.

use crate::trace_log::TraceLogEntry;
use crate::types::{
    DeepSeekUserIdAudit, UserIdAuditBreakdown, UserIdModelCount, UserIdProjectCount,
};
use std::collections::HashMap;

const TOP_PROJECT_LIMIT: usize = 10;

fn is_deepseek_entry(e: &TraceLogEntry) -> bool {
    if matches!(
        e.pipeline.as_deref(),
        Some("cursor_deepseek_v4") | Some("deepseek_light")
    ) {
        return true;
    }
    if e.model.to_ascii_lowercase().starts_with("deepseek-") {
        return true;
    }
    if e.upstream_model
        .as_deref()
        .is_some_and(|m| m.to_ascii_lowercase().starts_with("deepseek-"))
    {
        return true;
    }
    false
}

fn model_label(e: &TraceLogEntry) -> String {
    e.upstream_model
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| e.model.clone())
}

/// Aggregate DeepSeek `user_id` isolation metrics from trace entries.
pub fn compute_deepseek_user_id_audit(entries: &[TraceLogEntry]) -> DeepSeekUserIdAudit {
    let deepseek: Vec<&TraceLogEntry> = entries.iter().filter(|e| is_deepseek_entry(e)).collect();
    let total = deepseek.len();

    if total == 0 {
        return DeepSeekUserIdAudit {
            deepseek_requests: 0,
            with_upstream_user_id: 0,
            without_upstream_user_id: 0,
            upstream_user_id_ratio: 0.0,
            missing_project_id: 0,
            client_user_id_leaks: 0,
            audit_breakdown: UserIdAuditBreakdown::default(),
            by_upstream_model: vec![],
            top_project_ids: vec![],
            isolation_ok: true,
            conclusion: "No DeepSeek requests in the selected window.".to_string(),
        };
    }

    let mut with_upstream = 0usize;
    let mut missing_project = 0usize;
    let mut client_leaks = 0usize;
    let mut breakdown = UserIdAuditBreakdown::default();
    let mut model_counts: HashMap<String, usize> = HashMap::new();
    let mut project_counts: HashMap<String, usize> = HashMap::new();

    for e in &deepseek {
        let has_upstream = e.upstream_user_id.as_deref().is_some_and(|s| !s.is_empty());
        if has_upstream {
            with_upstream += 1;
            if let Some(pid) = e.upstream_user_id.as_deref().filter(|s| !s.is_empty()) {
                *project_counts.entry(pid.to_string()).or_insert(0) += 1;
            }
        }

        if e.project_id.as_deref().unwrap_or("").is_empty() {
            missing_project += 1;
        }

        if e.client_body_user_id
            .as_deref()
            .is_some_and(|c| !c.is_empty())
        {
            let upstream = e.upstream_user_id.as_deref().unwrap_or("");
            if upstream.is_empty() || Some(upstream) != e.client_body_user_id.as_deref() {
                client_leaks += 1;
            }
        }

        match e.user_id_audit.as_deref() {
            Some("injected") => breakdown.injected += 1,
            Some("absent") => breakdown.absent += 1,
            Some("stripped_client") => breakdown.stripped_client += 1,
            Some("mismatch") => breakdown.mismatch += 1,
            _ => breakdown.not_applicable += 1,
        }

        *model_counts.entry(model_label(e)).or_insert(0) += 1;
    }

    let without_upstream = total.saturating_sub(with_upstream);
    let upstream_ratio = with_upstream as f64 / total as f64;

    let mut by_upstream_model: Vec<UserIdModelCount> = model_counts
        .into_iter()
        .map(|(model, count)| UserIdModelCount {
            model,
            count,
            percentage: count as f64 / total as f64 * 100.0,
        })
        .collect();
    by_upstream_model.sort_by(|a, b| b.count.cmp(&a.count));

    let mut top_project_ids: Vec<UserIdProjectCount> = project_counts
        .into_iter()
        .map(|(project_id, count)| UserIdProjectCount {
            project_id,
            count,
            percentage: count as f64 / total as f64 * 100.0,
        })
        .collect();
    top_project_ids.sort_by(|a, b| b.count.cmp(&a.count));
    top_project_ids.truncate(TOP_PROJECT_LIMIT);

    let injected_ratio = breakdown.injected as f64 / total as f64;
    let isolation_ok = breakdown.mismatch == 0
        && client_leaks == 0
        && (breakdown.injected > 0 || total == 0)
        && injected_ratio >= 0.95
        && missing_project == 0;

    let conclusion = if isolation_ok {
        format!(
            "DeepSeek user_id isolation looks healthy: {:.0}% injected, 0 mismatch, 0 client leaks.",
            injected_ratio * 100.0
        )
    } else if breakdown.mismatch > 0 {
        format!(
            "{} request(s) have user_id mismatch between project_id and upstream body.",
            breakdown.mismatch
        )
    } else if missing_project > 0 {
        format!(
            "{missing_project} DeepSeek request(s) lack project_id (shared empty user_id risk)."
        )
    } else if client_leaks > 0 {
        format!(
            "{client_leaks} request(s) still carry client-supplied user_id not aligned with gateway policy."
        )
    } else if injected_ratio < 0.95 {
        format!(
            "Only {:.0}% of DeepSeek requests have injected user_id; check sk-cc-* project_id bindings.",
            injected_ratio * 100.0
        )
    } else {
        "Review audit breakdown for absent or not_applicable entries.".to_string()
    };

    DeepSeekUserIdAudit {
        deepseek_requests: total,
        with_upstream_user_id: with_upstream,
        without_upstream_user_id: without_upstream,
        upstream_user_id_ratio: upstream_ratio,
        missing_project_id: missing_project,
        client_user_id_leaks: client_leaks,
        audit_breakdown: breakdown,
        by_upstream_model,
        top_project_ids,
        isolation_ok,
        conclusion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace_log::TraceLogEntry;

    fn entry(
        model: &str,
        pipeline: &str,
        project_id: Option<&str>,
        upstream_user_id: Option<&str>,
        audit: &str,
    ) -> TraceLogEntry {
        TraceLogEntry {
            timestamp_ms: 0,
            request_hash: "abc".into(),
            content_length: 0,
            semantic_cluster: 0,
            conversation_id: None,
            consumer: None,
            model: model.into(),
            prompt_tokens: 0,
            latency_ms: 0.0,
            upstream_latency_ms: None,
            prefill_ms: None,
            pre_header_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cache_hit: false,
            cache_tier: None,
            domain: None,
            project_id: project_id.map(str::to_string),
            composition: None,
            request_messages_snapshot: None,
            response_preview: None,
            retired_prefix_messages: None,
            reasoning_strategy: None,
            prompt_cache_hit_ratio: None,
            upstream_profile_id: Some("deepseek".into()),
            pipeline: Some(pipeline.into()),
            upstream_model: Some(model.into()),
            client_body_user_id: None,
            upstream_user_id: upstream_user_id.map(str::to_string),
            user_id_audit: Some(audit.into()),
            upstream_key_id: None,
            affinity_key: None,
            affinity_kind: None,
            backend_name: None,
            session_fingerprint: None,
            is_coalesced: false,
            client_key_id: None,
            session_store: None,
            stable_session_kind: None,
            upstream_outbound_bytes: None,
            request_passthrough: false,
            request_passthrough_prefix_len: None,
            status_code: None,
            error_code: None,
            limit_source: None,
            cache_decision: None,
            upstream_result: None,
            phase_durations_ms: None,
            client_ip: None,
            client_kind: None,
        }
    }

    #[test]
    fn aggregates_injected_requests() {
        let entries = vec![
            entry(
                "deepseek-v4-pro",
                "cursor_deepseek_v4",
                Some("tenant_a"),
                Some("tenant_a"),
                "injected",
            ),
            entry(
                "deepseek-v4-flash",
                "deepseek_light",
                Some("tenant_b"),
                Some("tenant_b"),
                "injected",
            ),
        ];
        let audit = compute_deepseek_user_id_audit(&entries);
        assert_eq!(audit.deepseek_requests, 2);
        assert_eq!(audit.with_upstream_user_id, 2);
        assert!(audit.isolation_ok);
        assert_eq!(audit.audit_breakdown.injected, 2);
    }

    #[test]
    fn flags_missing_project() {
        let entries = vec![entry(
            "deepseek-v4-pro",
            "cursor_deepseek_v4",
            None,
            None,
            "absent",
        )];
        let audit = compute_deepseek_user_id_audit(&entries);
        assert_eq!(audit.missing_project_id, 1);
        assert!(!audit.isolation_ok);
    }
}
