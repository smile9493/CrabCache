//! Upstream error classification and fallback decision logic.
//!
//! Ported from OmniRoute's checkFallbackError(). Classifies upstream errors
//! into structured decisions: cooldown duration, whether to retry, and reason.

use crate::circuit_breaker::{FailureKind, classify_failure, parse_retry_after};
use std::time::Duration;
use tracing::debug;

/// Decision result from check_fallback_error().
pub struct FallbackDecision {
    pub should_fallback: bool,
    pub cooldown: Duration,
    pub reason: String,
    pub failure_kind: FailureKind,
}

/// Check an upstream error and decide fallback behavior.
///
/// Priority chain (from OmniRoute):
/// 1. 401 → disable key (permanent)
/// 2. Quota exhausted body → 1h cooldown
/// 3. 429 + Retry-After → use parsed value
/// 4. 5xx / 408 → transient cooldown (5s)
/// 5. Other → 5s transient
pub fn check_fallback_error(
    status: u16,
    body: Option<&str>,
    retry_after_header: Option<&str>,
) -> FallbackDecision {
    let kind = classify_failure(status, body);

    // 401: permanent key disable
    if status == 401 {
        return FallbackDecision {
            should_fallback: false,
            cooldown: Duration::from_secs(31536000), // 1 year
            reason: "upstream key unauthorized (401)".to_string(),
            failure_kind: FailureKind::Transient,
        };
    }

    // 429: classify body to distinguish rate-limit vs quota-exhausted
    if status == 429 {
        // Quota exhausted (from body analysis) — long cooldown regardless of Retry-After
        if kind == FailureKind::QuotaExhausted {
            return FallbackDecision {
                should_fallback: true,
                cooldown: Duration::from_secs(3600),
                reason: "429 quota exhausted".to_string(),
                failure_kind: FailureKind::QuotaExhausted,
            };
        }
        // Rate limit — respect Retry-After header
        if let Some(retry_after) = retry_after_header {
            if let Some(duration) = parse_retry_after(retry_after) {
                debug!(
                    status,
                    retry_after_secs = duration.as_secs(),
                    "429: using upstream Retry-After hint"
                );
                return FallbackDecision {
                    should_fallback: true,
                    cooldown: duration,
                    reason: format!("429 rate limited (retry-after: {}s)", duration.as_secs()),
                    failure_kind: FailureKind::RateLimit,
                };
            }
        }
        return FallbackDecision {
            should_fallback: true,
            cooldown: Duration::from_secs(60),
            reason: "429 rate limited (no retry-after)".to_string(),
            failure_kind: FailureKind::RateLimit,
        };
    }

    // 5xx / 408: transient
    if matches!(status, 408 | 500 | 502 | 503 | 504) {
        return FallbackDecision {
            should_fallback: true,
            cooldown: Duration::from_secs(5),
            reason: format!("transient error ({status})"),
            failure_kind: FailureKind::Transient,
        };
    }

    // 400: context overflow or malformed → immediate fallback
    if status == 400 {
        return FallbackDecision {
            should_fallback: true,
            cooldown: Duration::ZERO,
            reason: "bad request (400)".to_string(),
            failure_kind: FailureKind::Transient,
        };
    }

    // Default: 5s transient
    FallbackDecision {
        should_fallback: true,
        cooldown: Duration::from_secs(5),
        reason: format!("unexpected error ({status})"),
        failure_kind: FailureKind::Transient,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_is_permanent() {
        let d = check_fallback_error(401, None, None);
        assert!(!d.should_fallback);
        assert!(d.cooldown.as_secs() > 3600);
    }

    #[test]
    fn quota_exhausted_long_cooldown() {
        let d = check_fallback_error(429, Some("daily quota exceeded"), None);
        assert_eq!(d.failure_kind, FailureKind::QuotaExhausted);
        assert_eq!(d.cooldown.as_secs(), 3600);
    }

    #[test]
    fn rate_limit_with_retry_after() {
        let d = check_fallback_error(429, None, Some("30"));
        assert_eq!(d.cooldown.as_secs(), 30);
    }

    #[test]
    fn rate_limit_without_retry_after() {
        let d = check_fallback_error(429, None, None);
        assert_eq!(d.cooldown.as_secs(), 60);
    }

    #[test]
    fn transient_error() {
        let d = check_fallback_error(503, None, None);
        assert_eq!(d.cooldown.as_secs(), 5);
    }

    #[test]
    fn bad_request_immediate() {
        let d = check_fallback_error(400, None, None);
        assert_eq!(d.cooldown, Duration::ZERO);
    }
}
