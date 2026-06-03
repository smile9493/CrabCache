//! Codex upstream rate-limit / capacity error detection (CLIProxyAPI-aligned).
//!
//! Promotes "high demand", "at capacity", and `usage_limit_reached` bodies to 429 semantics
//! and parses `resets_in_seconds` / `resets_at` for precise key cooldowns.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// When upstream returns generic "high demand" without `resets_in_*`, avoid reusing the
/// account for at least this long (usage limits often last hours; 60s pool default is too short).
pub const CODEX_HIGH_DEMAND_FALLBACK_SECS: u64 = 900;

/// Result of classifying an upstream response for key-pool rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitClassification {
    pub is_rate_limit: bool,
    /// Status code used for metrics / logging (429 when capacity is promoted).
    pub effective_status: u16,
    /// Cooldown until the key may be reused; `None` → use pool default.
    pub cooldown: Option<Duration>,
}

impl RateLimitClassification {
    pub fn none(status: u16) -> Self {
        Self {
            is_rate_limit: false,
            effective_status: status,
            cooldown: None,
        }
    }
}

/// Model family scope for per-key cooldown (Codex vs spark vs default).
pub fn codex_model_scope(model: &str) -> &'static str {
    let m = model.trim().to_ascii_lowercase();
    if m.contains("spark") {
        "spark"
    } else if m.contains("codex") {
        "codex"
    } else {
        "default"
    }
}

fn body_text(body: Option<&str>) -> String {
    body.unwrap_or("").trim().to_string()
}

/// True when the upstream body indicates Codex capacity / usage-limit pressure.
pub fn body_indicates_codex_rate_limit(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    if lower.contains("usage_limit_reached") {
        return true;
    }
    if lower.contains("high demand") || lower.contains("experiencing high") {
        return true;
    }
    if lower.contains("selected model is at capacity")
        || lower.contains("model is at capacity")
        || lower.contains("at capacity. please try")
    {
        return true;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if v.pointer("/error/type")
            .and_then(|t| t.as_str())
            .is_some_and(|t| t.eq_ignore_ascii_case("usage_limit_reached"))
        {
            return true;
        }
        for path in ["/error/message", "/message", "/detail"] {
            if v.pointer(path)
                .and_then(|m| m.as_str())
                .is_some_and(|msg| message_indicates_codex_rate_limit(msg))
            {
                return true;
            }
        }
    }
    message_indicates_codex_rate_limit(body)
}

fn message_indicates_codex_rate_limit(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("high demand")
        || lower.contains("experiencing high")
        || lower.contains("usage limit")
        || lower.contains("usage_limit")
        || lower.contains("at capacity")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
}

/// Parse Codex `usage_limit_reached` retry window from JSON body.
pub fn parse_codex_retry_after(body: &str, now: SystemTime) -> Option<Duration> {
    let v = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let err = v.get("error")?;
    if err
        .get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| !t.eq_ignore_ascii_case("usage_limit_reached"))
    {
        // Still allow resets_* fields on other error shapes when present.
        if !body_indicates_codex_rate_limit(body) {
            return None;
        }
    }
    if let Some(resets_at) = err.get("resets_at").and_then(|v| v.as_i64()) {
        if resets_at > 0 {
            let reset = UNIX_EPOCH + Duration::from_secs(resets_at.max(0) as u64);
            if reset > now {
                return reset.duration_since(now).ok();
            }
        }
    }
    if let Some(secs) = err.get("resets_in_seconds").and_then(|v| v.as_i64()) {
        if secs > 0 {
            return Some(Duration::from_secs(secs as u64));
        }
    }
    None
}

fn cooldown_from_headers_and_body(
    body: Option<&str>,
    retry_after_header: Option<&str>,
    default_secs: u64,
) -> Duration {
    let now = SystemTime::now();
    if let Some(text) = body.filter(|b| !b.trim().is_empty()) {
        if let Some(d) = parse_codex_retry_after(text, now) {
            return d;
        }
        if body_indicates_codex_rate_limit(text) {
            return Duration::from_secs(CODEX_HIGH_DEMAND_FALLBACK_SECS.max(default_secs));
        }
    }
    if let Some(hdr) = retry_after_header {
        if let Some(d) = crate::circuit_breaker::parse_retry_after(hdr) {
            return d;
        }
    }
    Duration::from_secs(default_secs.max(1))
}

/// Classify upstream status + optional body for key-pool rate-limit handling.
pub fn classify_upstream_rate_limit(
    status: u16,
    body: Option<&str>,
    retry_after_header: Option<&str>,
    codex_context: bool,
    mimo_context: bool,
    default_cooldown_secs: u64,
) -> RateLimitClassification {
    let text = body_text(body);

    if status == 429 || body_indicates_codex_rate_limit(&text) {
        return RateLimitClassification {
            is_rate_limit: true,
            effective_status: 429,
            cooldown: Some(cooldown_from_headers_and_body(
                body,
                retry_after_header,
                default_cooldown_secs,
            )),
        };
    }

    // Header-only path for Codex/MiMo overload (no body yet at response_filter).
    if (codex_context || mimo_context) && status == 503 {
        return RateLimitClassification {
            is_rate_limit: true,
            effective_status: 429,
            cooldown: Some(Duration::from_secs(default_cooldown_secs.max(1))),
        };
    }

    RateLimitClassification::none(status)
}

/// Resolve cooldown seconds from an upstream error body (SSE or JSON).
pub fn resolve_codex_cooldown_secs(body: &str, default_secs: u64) -> u64 {
    if let Some(d) = parse_codex_retry_after(body, SystemTime::now()) {
        return d.as_secs().max(1);
    }
    if body_indicates_codex_rate_limit(body) {
        CODEX_HIGH_DEMAND_FALLBACK_SECS.max(default_secs)
    } else {
        default_secs.max(1)
    }
}

/// Dynamic same-request retry budget from pool size (P1).
pub fn codex_retry_budget(pool_len: usize, max_budget: u8) -> u8 {
    if pool_len == 0 {
        0
    } else if pool_len == 1 {
        1
    } else {
        pool_len.saturating_sub(1).min(max_budget as usize) as u8
    }
}

/// Same as [`codex_retry_budget`] — shared by Codex bridge and MiMo relay pipelines.
pub fn pool_scaled_retry_budget(pool_len: usize, max_budget: u8) -> u8 {
    codex_retry_budget(pool_len, max_budget)
}

/// True for Codex OAuth / DeepSeek bridge pipelines only.
///
/// [`RequestPipeline::CodexMimo`] is excluded: it uses the MiMo API key pool and
/// [`GatewayProxy::is_mimo_pipeline`], not Codex quota preflight or OAuth binding.
pub fn is_codex_upstream_pipeline(pipeline: Option<crab_pipeline::RequestPipeline>) -> bool {
    matches!(
        pipeline,
        Some(
            crab_pipeline::RequestPipeline::CodexRelay
                | crab_pipeline::RequestPipeline::CodexDeepSeek
        )
    )
}

/// Rate-limit cooldown scope for Codex bridge and MiMo relay pipelines.
///
/// Returns [`Some`](codex_model_scope) when `codex || mimo`, else global (no scope).
pub fn upstream_rate_limit_scope(
    codex: bool,
    mimo: bool,
    upstream_model: &str,
) -> Option<&'static str> {
    if codex || mimo {
        Some(codex_model_scope(upstream_model))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_message_promoted() {
        let body = r#"{"error":{"message":"Selected model is at capacity. Please try a different model."}}"#;
        assert!(body_indicates_codex_rate_limit(body));
        let c = classify_upstream_rate_limit(400, Some(body), None, true, false, 60);
        assert!(c.is_rate_limit);
        assert_eq!(c.effective_status, 429);
    }

    #[test]
    fn high_demand_promoted() {
        let body = r#"{"error":{"message":"We're currently experiencing high demand, which may cause temporary errors."}}"#;
        assert!(body_indicates_codex_rate_limit(body));
        let c = classify_upstream_rate_limit(502, Some(body), None, true, false, 60);
        assert!(c.is_rate_limit);
    }

    #[test]
    fn usage_limit_resets_in_seconds() {
        let body = r#"{"error":{"type":"usage_limit_reached","resets_in_seconds":123}}"#;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let d = parse_codex_retry_after(body, now).unwrap();
        assert_eq!(d, Duration::from_secs(123));
    }

    #[test]
    fn usage_limit_resets_at() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let resets_at = now + Duration::from_secs(300);
        let body = format!(
            r#"{{"error":{{"type":"usage_limit_reached","resets_at":{}}}}}"#,
            resets_at.duration_since(UNIX_EPOCH).unwrap().as_secs()
        );
        let d = parse_codex_retry_after(&body, now).unwrap();
        assert_eq!(d, Duration::from_secs(300));
    }

    #[test]
    fn codex_503_header_only() {
        let c = classify_upstream_rate_limit(503, None, None, true, false, 90);
        assert!(c.is_rate_limit);
        assert_eq!(c.cooldown, Some(Duration::from_secs(90)));
    }

    #[test]
    fn non_codex_503_not_rate_limit() {
        let c = classify_upstream_rate_limit(503, None, None, false, false, 60);
        assert!(!c.is_rate_limit);
    }

    #[test]
    fn high_demand_fallback_cooldown() {
        let body = r#"{"error":{"message":"We're currently experiencing high demand"}}"#;
        assert_eq!(
            resolve_codex_cooldown_secs(body, 60),
            CODEX_HIGH_DEMAND_FALLBACK_SECS
        );
    }

    #[test]
    fn codex_retry_budget_scales_with_pool() {
        assert_eq!(codex_retry_budget(0, 3), 0);
        assert_eq!(codex_retry_budget(1, 3), 1);
        assert_eq!(codex_retry_budget(2, 3), 1);
        assert_eq!(codex_retry_budget(4, 3), 3);
        assert_eq!(codex_retry_budget(10, 3), 3);
    }

    #[test]
    fn spark_scope() {
        assert_eq!(codex_model_scope("gpt-5.3-codex-spark"), "spark");
        assert_eq!(codex_model_scope("gpt-5-codex"), "codex");
    }

    #[test]
    fn pool_scaled_retry_budget_matches_codex_retry_budget() {
        assert_eq!(pool_scaled_retry_budget(4, 3), codex_retry_budget(4, 3));
    }

    #[test]
    fn codex_upstream_pipeline_excludes_codex_mimo() {
        use crab_pipeline::RequestPipeline;
        assert!(is_codex_upstream_pipeline(Some(RequestPipeline::CodexRelay)));
        assert!(is_codex_upstream_pipeline(Some(RequestPipeline::CodexDeepSeek)));
        assert!(!is_codex_upstream_pipeline(Some(RequestPipeline::CodexMimo)));
        assert!(!is_codex_upstream_pipeline(Some(RequestPipeline::MimoTokenPlanRelay)));
    }
}
