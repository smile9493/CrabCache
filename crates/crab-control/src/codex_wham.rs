//! Codex WHAM (Usage API) response parser.
//!
//! Parses `chatgpt.com/backend-api/wham/usage` JSON into structured quota data.
//! Shared between gateway runtime (quota preflight) and admin dashboard (Key test).
//!
//! Window classification uses `limit_window_seconds` (CPA-Manager style) to
//! correctly identify 5h/7d windows even when the API returns them in swapped order.

use chrono::{DateTime, Utc};

/// Primary window duration (5 hours).
const FIVE_HOUR_SECONDS: f64 = 18_000.0;
/// Secondary window duration (7 days / 168 hours).
const WEEK_SECONDS: f64 = 604_800.0;

// ── Window item for display ──────────────────────────────────────────────────

/// A single Codex quota window for dashboard display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodexQuotaWindowItem {
    /// Stable identifier: "five-hour", "weekly", "code-review-five-hour", etc.
    pub id: String,
    /// Human-readable label (English default; Dashboard maps via locale).
    pub label: String,
    /// Used percent (0–100). `None` = no data; `Some(100)` = limit_reached with reset info.
    pub used_percent: Option<f64>,
    /// Absolute reset time as Unix seconds. `None` = unknown.
    pub reset_at_secs: Option<i64>,
}

// ── Snapshot (runtime preflight) ─────────────────────────────────────────────

/// Dual-window quota snapshot from WHAM API (used by runtime preflight).
#[derive(Debug, Clone)]
pub struct CodexQuotaSnapshot {
    /// Primary (5h session) window used percent (0–100).
    pub session_used_percent: Option<f64>,
    /// Secondary (7d weekly) window used percent (0–100).
    pub weekly_used_percent: Option<f64>,
    /// When the session (5h) window resets.
    pub session_reset_at: Option<DateTime<Utc>>,
    /// When the weekly (7d) window resets.
    pub weekly_reset_at: Option<DateTime<Utc>>,
    /// Whether the rate limiter has declared `limit_reached`.
    pub limit_reached: bool,
}

impl CodexQuotaSnapshot {
    /// Parse a WHAM API JSON response body using `limit_window_seconds` classification.
    ///
    /// Returns `None` if no windows are present (not a Codex account or malformed response).
    pub fn parse_wham_json(body: &serde_json::Value) -> Option<Self> {
        let rate_limit = get_field(body, "rate_limit").or_else(|| get_field(body, "rateLimit"))?;
        let primary_window = get_field(rate_limit, "primary_window")
            .or_else(|| get_field(rate_limit, "primaryWindow"));
        let secondary_window = get_field(rate_limit, "secondary_window")
            .or_else(|| get_field(rate_limit, "secondaryWindow"));

        if primary_window.is_none() && secondary_window.is_none() {
            return None;
        }

        // CPA-style classify: use limit_window_seconds to find the real 5h/7d windows,
        // falling back to positional (primary=5h, secondary=7d).
        let (five_hour, weekly) = classify_windows(primary_window, secondary_window);

        let session_used_percent = window_used_percent(five_hour, rate_limit);
        let weekly_used_percent = window_used_percent(weekly, rate_limit);
        let session_reset_at = five_hour.and_then(parse_window_reset_datetime);
        let weekly_reset_at = weekly.and_then(parse_window_reset_datetime);

        let limit_reached = get_bool(rate_limit, "limit_reached")
            .or_else(|| get_bool(rate_limit, "limitReached"))
            .unwrap_or(false);

        Some(Self {
            session_used_percent,
            weekly_used_percent,
            session_reset_at,
            weekly_reset_at,
            limit_reached,
        })
    }

    /// Minimum remaining percent across both windows (100 – max(used%)).
    pub fn min_remaining_percent(&self) -> Option<f64> {
        let worst = [self.session_used_percent, self.weekly_used_percent]
            .iter()
            .filter_map(|v| *v)
            .fold(None, |acc, x| Some(acc.map_or(x, |a: f64| a.max(x))));
        worst.map(|used| (100.0 - used).max(0.0))
    }

    /// Whether any window indicates the account is exhausted.
    pub fn is_exhausted(&self, threshold_percent: f64) -> bool {
        if self.limit_reached {
            return true;
        }
        if let Some(remaining) = self.min_remaining_percent() {
            return remaining <= threshold_percent;
        }
        false
    }
}

// ── Full window list builder (dashboard) ─────────────────────────────────────

/// Build the complete list of Codex quota windows from a WHAM response.
/// Includes: main rate_limit (5h/7d), code_review_rate_limit, additional_rate_limits.
pub fn build_codex_quota_windows(body: &serde_json::Value) -> Vec<CodexQuotaWindowItem> {
    let mut windows = Vec::new();

    // Main rate_limit
    let rate_limit = get_field(body, "rate_limit").or_else(|| get_field(body, "rateLimit"));
    let limit_reached = rate_limit
        .and_then(|rl| get_bool(rl, "limit_reached").or_else(|| get_bool(rl, "limitReached")))
        .unwrap_or(false);
    let allowed = rate_limit.and_then(|rl| get_bool(rl, "allowed"));

    if let Some(rl) = rate_limit {
        add_rate_limit_windows(
            &mut windows,
            rl,
            "five-hour",
            "5h window",
            "weekly",
            "7d window",
            limit_reached,
            allowed,
        );
    }

    // code_review_rate_limit
    let code_review = get_field(body, "code_review_rate_limit")
        .or_else(|| get_field(body, "codeReviewRateLimit"));
    if let Some(rl) = code_review {
        let cr_limit_reached = limit_reached
            || get_bool(rl, "limit_reached")
                .or_else(|| get_bool(rl, "limitReached"))
                .unwrap_or(false);
        let cr_allowed = allowed.or_else(|| get_bool(rl, "allowed"));
        add_rate_limit_windows(
            &mut windows,
            rl,
            "code-review-five-hour",
            "Code Review 5h",
            "code-review-weekly",
            "Code Review 7d",
            cr_limit_reached,
            cr_allowed,
        );
    }

    // additional_rate_limits[]
    let additional = get_field(body, "additional_rate_limits")
        .or_else(|| get_field(body, "additionalRateLimits"));
    if let Some(arr) = additional.and_then(|v| v.as_array()) {
        for (idx, item) in arr.iter().enumerate() {
            let limit_name = get_str(item, "limit_name")
                .or_else(|| get_str(item, "limitName"))
                .unwrap_or("Additional");
            let rl = get_field(item, "rate_limit").or_else(|| get_field(item, "rateLimit"));
            let Some(rl) = rl else { continue };

            let a_limit_reached = limit_reached
                || get_bool(rl, "limit_reached")
                    .or_else(|| get_bool(rl, "limitReached"))
                    .unwrap_or(false);
            let a_allowed = allowed.or_else(|| get_bool(rl, "allowed"));

            let normalized = normalize_window_id(limit_name);
            let fh_id = format!("{normalized}-five-hour-{idx}");
            let wk_id = format!("{normalized}-weekly-{idx}");
            let fh_label = format!("{limit_name} 5h");
            let wk_label = format!("{limit_name} 7d");
            add_rate_limit_windows(
                &mut windows,
                rl,
                &fh_id,
                &fh_label,
                &wk_id,
                &wk_label,
                a_limit_reached,
                a_allowed,
            );
        }
    }

    windows
}

/// Build a `KeyQuotaInfo` from WHAM JSON (unified entry point for management API).
pub fn wham_to_key_quota_info(body: &serde_json::Value) -> Option<KeyQuotaInfoInner> {
    use self::KeyQuotaInfoInner as KeyQuotaInfo;

    let rate_limit = get_field(body, "rate_limit").or_else(|| get_field(body, "rateLimit"));
    let plan_type = get_str(body, "plan_type")
        .or_else(|| get_str(body, "planType"))
        .map(str::to_string);
    let balance = get_f64(body, "credit_balance").or_else(|| get_f64(body, "balance"));
    let total_used = get_f64(body, "used").or_else(|| get_f64(body, "total_used"))
        .or_else(|| get_f64(body, "totalUsed"));
    let rate_allowed = rate_limit.and_then(|rl| get_bool(rl, "allowed"));
    let limit_reached = rate_limit
        .and_then(|rl| get_bool(rl, "limit_reached").or_else(|| get_bool(rl, "limitReached")))
        .unwrap_or(false);
    let is_available = rate_allowed.or(if limit_reached { Some(false) } else { None });

    // Classify primary windows for backward-compat fields
    let primary_window = rate_limit.and_then(|rl| {
        get_field(rl, "primary_window").or_else(|| get_field(rl, "primaryWindow"))
    });
    let secondary_window = rate_limit.and_then(|rl| {
        get_field(rl, "secondary_window").or_else(|| get_field(rl, "secondaryWindow"))
    });

    let (five_hour, weekly) = classify_windows(primary_window, secondary_window);

    let primary_used_percent = five_hour.and_then(window_used_percent_from_window);
    let secondary_used_percent = weekly.and_then(window_used_percent_from_window);

    // Use CPA logic: if limit_reached and no used_percent, treat as 100%
    let primary_used_percent = if primary_used_percent.is_none() && limit_reached && five_hour.is_some() {
        five_hour.and_then(parse_window_reset_secs).map(|_| 100.0)
    } else {
        primary_used_percent
    };
    let secondary_used_percent = if secondary_used_percent.is_none() && limit_reached && weekly.is_some() {
        weekly.and_then(parse_window_reset_secs).map(|_| 100.0)
    } else {
        secondary_used_percent
    };

    let primary_reset_after_secs = five_hour
        .and_then(|w| get_f64(w, "reset_after_seconds").or_else(|| get_f64(w, "resetAfterSeconds")))
        .map(|v| v as u64);
    let secondary_reset_after_secs = weekly
        .and_then(|w| get_f64(w, "reset_after_seconds").or_else(|| get_f64(w, "resetAfterSeconds")))
        .map(|v| v as u64);

    let primary_reset_at_secs = five_hour.and_then(parse_window_reset_secs);
    let secondary_reset_at_secs = weekly.and_then(parse_window_reset_secs);

    let codex_windows = build_codex_quota_windows(body);
    let codex_windows = if codex_windows.is_empty() {
        None
    } else {
        Some(codex_windows)
    };

    if plan_type.is_none()
        && primary_used_percent.is_none()
        && secondary_used_percent.is_none()
        && balance.is_none()
        && total_used.is_none()
        && is_available.is_none()
    {
        return None;
    }

    Some(KeyQuotaInfo {
        is_available,
        balance,
        total_granted: None,
        total_used,
        plan_type,
        primary_used_percent,
        secondary_used_percent,
        primary_reset_after_secs,
        secondary_reset_after_secs,
        primary_reset_at_secs,
        secondary_reset_at_secs,
        codex_windows,
    })
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Classify primary/secondary windows into (5h, weekly) using `limit_window_seconds`.
/// Falls back to positional assumption when duration is absent.
fn classify_windows<'a>(
    primary: Option<&'a serde_json::Value>,
    secondary: Option<&'a serde_json::Value>,
) -> (Option<&'a serde_json::Value>, Option<&'a serde_json::Value>) {
    let primary_seconds = primary.and_then(|w| get_f64(w, "limit_window_seconds")
        .or_else(|| get_f64(w, "limitWindowSeconds")));
    let secondary_seconds = secondary.and_then(|w| get_f64(w, "limit_window_seconds")
        .or_else(|| get_f64(w, "limitWindowSeconds")));

    let mut five_hour: Option<&serde_json::Value> = None;
    let mut weekly: Option<&serde_json::Value> = None;

    // First pass: classify by duration
    for (window, seconds) in [(primary, primary_seconds), (secondary, secondary_seconds)] {
        let Some(w) = window else { continue };
        match seconds {
            Some(s) if s == FIVE_HOUR_SECONDS && five_hour.is_none() => five_hour = Some(w),
            Some(s) if s == WEEK_SECONDS && weekly.is_none() => weekly = Some(w),
            _ => {}
        }
    }

    // Order fallback (allowOrderFallback)
    if five_hour.is_none() {
        if let Some(p) = primary {
            if weekly.is_none_or(|w| !std::ptr::eq(p, w)) {
                five_hour = Some(p);
            }
        }
    }
    if weekly.is_none() {
        if let Some(s) = secondary {
            if five_hour.is_none_or(|f| !std::ptr::eq(s, f)) {
                weekly = Some(s);
            }
        }
    }

    (five_hour, weekly)
}

/// Get used_percent for a window, with CPA logic: if limit_reached/allowed=false
/// and the window has reset info but no used_percent, treat as 100%.
fn window_used_percent(
    window: Option<&serde_json::Value>,
    rate_limit: &serde_json::Value,
) -> Option<f64> {
    let w = window?;
    let direct = get_f64(w, "used_percent").or_else(|| get_f64(w, "usedPercent"));
    if direct.is_some() {
        return direct;
    }
    // CPA: limit_reached or allowed=false + has reset info → 100% used
    let limit_reached = get_bool(rate_limit, "limit_reached")
        .or_else(|| get_bool(rate_limit, "limitReached"))
        .unwrap_or(false);
    let allowed = get_bool(rate_limit, "allowed");
    let is_blocked = limit_reached || allowed == Some(false);
    if is_blocked && parse_window_reset_secs(w).is_some() {
        return Some(100.0);
    }
    None
}

/// Same as `window_used_percent` but from a window value directly (no rate_limit context).
fn window_used_percent_from_window(window: &serde_json::Value) -> Option<f64> {
    get_f64(window, "used_percent").or_else(|| get_f64(window, "usedPercent"))
}

/// Add classified 5h/7d windows from a rate_limit object.
fn add_rate_limit_windows(
    out: &mut Vec<CodexQuotaWindowItem>,
    rate_limit: &serde_json::Value,
    fh_id: &str,
    fh_label: &str,
    wk_id: &str,
    wk_label: &str,
    limit_reached: bool,
    allowed: Option<bool>,
) {
    let primary_window = get_field(rate_limit, "primary_window")
        .or_else(|| get_field(rate_limit, "primaryWindow"));
    let secondary_window = get_field(rate_limit, "secondary_window")
        .or_else(|| get_field(rate_limit, "secondaryWindow"));

    let (five_hour, weekly) = classify_windows(primary_window, secondary_window);

    let is_blocked = limit_reached || allowed == Some(false);

    // 5h window
    let fh_used = five_hour
        .and_then(|w| get_f64(w, "used_percent").or_else(|| get_f64(w, "usedPercent")))
        .or({
            if is_blocked && five_hour.and_then(parse_window_reset_secs).is_some() {
                Some(100.0)
            } else {
                None
            }
        });
    let fh_reset = five_hour.and_then(parse_window_reset_secs);
    if five_hour.is_some() {
        out.push(CodexQuotaWindowItem {
            id: fh_id.to_string(),
            label: fh_label.to_string(),
            used_percent: fh_used,
            reset_at_secs: fh_reset,
        });
    }

    // Weekly window
    let wk_used = weekly
        .and_then(|w| get_f64(w, "used_percent").or_else(|| get_f64(w, "usedPercent")))
        .or({
            if is_blocked && weekly.and_then(parse_window_reset_secs).is_some() {
                Some(100.0)
            } else {
                None
            }
        });
    let wk_reset = weekly.and_then(parse_window_reset_secs);
    if weekly.is_some() {
        out.push(CodexQuotaWindowItem {
            id: wk_id.to_string(),
            label: wk_label.to_string(),
            used_percent: wk_used,
            reset_at_secs: wk_reset,
        });
    }
}

/// Parse window reset into absolute Unix seconds (reset_at preferred, else now + reset_after).
fn parse_window_reset_secs(window: &serde_json::Value) -> Option<i64> {
    if let Some(reset_at) = get_f64(window, "reset_at").or_else(|| get_f64(window, "resetAt")) {
        if reset_at > 0.0 {
            return Some(reset_at as i64);
        }
    }
    if let Some(reset_after) = get_f64(window, "reset_after_seconds")
        .or_else(|| get_f64(window, "resetAfterSeconds"))
    {
        if reset_after > 0.0 {
            let now = chrono::Utc::now().timestamp();
            return Some(now + reset_after as i64);
        }
    }
    None
}

/// Parse window reset as `DateTime<Utc>` (for runtime preflight).
fn parse_window_reset_datetime(window: &serde_json::Value) -> Option<DateTime<Utc>> {
    let secs = parse_window_reset_secs(window)?;
    DateTime::from_timestamp(secs, 0)
}

/// Normalize a window name to a stable lowercase-kebab id (CPA normalizeWindowId).
fn normalize_window_id(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut last_dash = false;
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            result.push(lower);
            last_dash = false;
        } else if !last_dash {
            result.push('-');
            last_dash = true;
        }
    }
    result.trim_matches('-').to_string()
}

// ── JSON helpers ─────────────────────────────────────────────────────────────

fn get_field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value.get(key)
}

fn get_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    let v = get_field(value, key)?;
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        if let Ok(n) = s.parse::<f64>() {
            return Some(n);
        }
    }
    None
}

fn get_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    let v = get_field(value, key)?;
    v.as_bool()
}

fn get_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    let v = get_field(value, key)?;
    v.as_str()
}

// Alias for internal use (crate re-exports CodexQuotaWindowItem from validate.rs,
// but codex_wham also needs it for build_codex_quota_windows).
// We use crab-control's own CodexQuotaWindowItem here since the types match.

use crate::validate::KeyQuotaInfo as KeyQuotaInfoInner;

#[cfg(test)]
mod tests {
    use super::*;

    // ── CPA classify tests ────────────────────────────────────────────────

    #[test]
    fn classify_swapped_windows_by_duration() {
        // CPA test: primary=604800(weekly), secondary=18000(5h) → should be swapped
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 10.0,
                    "limit_window_seconds": 604_800,
                    "reset_after_seconds": 60
                },
                "secondary_window": {
                    "used_percent": 30.0,
                    "limit_window_seconds": 18_000,
                    "reset_after_seconds": 120
                }
            }
        });

        let windows = build_codex_quota_windows(&json);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, "five-hour");
        assert_eq!(windows[0].used_percent, Some(30.0)); // was secondary
        assert_eq!(windows[1].id, "weekly");
        assert_eq!(windows[1].used_percent, Some(10.0)); // was primary

        // Snapshot should also be correct
        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert_eq!(snapshot.session_used_percent, Some(30.0));
        assert_eq!(snapshot.weekly_used_percent, Some(10.0));
    }

    #[test]
    fn classify_limit_reached_no_used_percent() {
        // CPA test: limit_reached=true, no used_percent → treated as 100%
        let json = serde_json::json!({
            "rate_limit": {
                "limit_reached": true,
                "primary_window": {
                    "limit_window_seconds": 18_000,
                    "reset_after_seconds": 300
                }
            }
        });

        let windows = build_codex_quota_windows(&json);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, "five-hour");
        assert_eq!(windows[0].used_percent, Some(100.0));
    }

    #[test]
    fn classify_additional_rate_limits() {
        // CPA test: additional_rate_limits with normalized id
        let json = serde_json::json!({
            "additional_rate_limits": [{
                "limit_name": "Code Review Premium",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 45.0,
                        "limit_window_seconds": 18_000,
                        "reset_after_seconds": 600
                    },
                    "secondary_window": {
                        "used_percent": 55.0,
                        "limit_window_seconds": 604_800,
                        "reset_after_seconds": 1200
                    }
                }
            }]
        });

        let windows = build_codex_quota_windows(&json);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, "code-review-premium-five-hour-0");
        assert_eq!(windows[0].label, "Code Review Premium 5h");
        assert_eq!(windows[0].used_percent, Some(45.0));
        assert_eq!(windows[1].id, "code-review-premium-weekly-0");
        assert_eq!(windows[1].label, "Code Review Premium 7d");
        assert_eq!(windows[1].used_percent, Some(55.0));
    }

    #[test]
    fn classify_code_review_rate_limit() {
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": { "used_percent": 10.0, "limit_window_seconds": 18_000 },
                "secondary_window": { "used_percent": 20.0, "limit_window_seconds": 604_800 }
            },
            "code_review_rate_limit": {
                "primary_window": { "used_percent": 30.0, "limit_window_seconds": 18_000 },
                "secondary_window": { "used_percent": 40.0, "limit_window_seconds": 604_800 }
            }
        });

        let windows = build_codex_quota_windows(&json);
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0].id, "five-hour");
        assert_eq!(windows[1].id, "weekly");
        assert_eq!(windows[2].id, "code-review-five-hour");
        assert_eq!(windows[3].id, "code-review-weekly");
    }

    // ── Original tests (adapted) ──────────────────────────────────────────

    #[test]
    fn parse_dual_window_response() {
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 45.0,
                    "limit_window_seconds": 18_000,
                    "reset_after_seconds": 18000
                },
                "secondary_window": {
                    "used_percent": 12.5,
                    "limit_window_seconds": 604_800,
                    "reset_after_seconds": 504000
                },
                "limit_reached": false
            }
        });

        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert_eq!(snapshot.session_used_percent, Some(45.0));
        assert_eq!(snapshot.weekly_used_percent, Some(12.5));
        assert!(!snapshot.limit_reached);
        assert!(snapshot.session_reset_at.is_some());
        assert!(snapshot.weekly_reset_at.is_some());

        assert_eq!(snapshot.min_remaining_percent(), Some(55.0));
        assert!(!snapshot.is_exhausted(2.0));
    }

    #[test]
    fn parse_single_window_response() {
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 99.0,
                    "limit_window_seconds": 18_000
                }
            }
        });

        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert_eq!(snapshot.session_used_percent, Some(99.0));
        assert_eq!(snapshot.weekly_used_percent, None);
        assert_eq!(snapshot.min_remaining_percent(), Some(1.0));
        assert!(snapshot.is_exhausted(2.0));
    }

    #[test]
    fn parse_limit_reached() {
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 80.0,
                    "limit_window_seconds": 18_000
                },
                "limit_reached": true
            }
        });

        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert!(snapshot.limit_reached);
        assert!(snapshot.is_exhausted(2.0));
    }

    #[test]
    fn parse_empty_response() {
        let json = serde_json::json!({
            "some_other_field": "value"
        });

        assert!(CodexQuotaSnapshot::parse_wham_json(&json).is_none());
    }

    #[test]
    fn parse_camel_case_fields() {
        let json = serde_json::json!({
            "rateLimit": {
                "primaryWindow": {
                    "usedPercent": 55.0,
                    "resetAt": 1700000000,
                    "limitWindowSeconds": 18_000
                },
                "limitReached": false
            }
        });

        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert_eq!(snapshot.session_used_percent, Some(55.0));
        assert_eq!(snapshot.min_remaining_percent(), Some(45.0));
    }

    #[test]
    fn worst_case_across_windows() {
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 10.0,
                    "limit_window_seconds": 18_000
                },
                "secondary_window": {
                    "used_percent": 97.0,
                    "limit_window_seconds": 604_800
                }
            }
        });

        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert_eq!(snapshot.min_remaining_percent(), Some(3.0));
        assert!(!snapshot.is_exhausted(2.0));
        assert!(snapshot.is_exhausted(4.0));
    }

    #[test]
    fn no_secondary_returns_none_weekly() {
        let json = serde_json::json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 50.0,
                    "limit_window_seconds": 18_000
                }
            }
        });

        let snapshot = CodexQuotaSnapshot::parse_wham_json(&json).unwrap();
        assert_eq!(snapshot.session_used_percent, Some(50.0));
        assert_eq!(snapshot.weekly_used_percent, None);

        let windows = build_codex_quota_windows(&json);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, "five-hour");
    }

    #[test]
    fn wham_to_key_quota_info_full() {
        let json = serde_json::json!({
            "plan_type": "plus",
            "credit_balance": 42.5,
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 30.0,
                    "limit_window_seconds": 18_000,
                    "reset_after_seconds": 3600
                },
                "secondary_window": {
                    "used_percent": 5.0,
                    "limit_window_seconds": 604_800,
                    "reset_after_seconds": 500000
                }
            }
        });

        let info = wham_to_key_quota_info(&json).unwrap();
        assert_eq!(info.plan_type.as_deref(), Some("plus"));
        assert_eq!(info.balance, Some(42.5));
        assert_eq!(info.primary_used_percent, Some(30.0));
        assert_eq!(info.secondary_used_percent, Some(5.0));
        assert!(info.codex_windows.is_some());
        assert_eq!(info.codex_windows.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn normalize_window_id_basic() {
        assert_eq!(normalize_window_id("Code Review Premium"), "code-review-premium");
        assert_eq!(normalize_window_id("  extra  spaces  "), "extra-spaces");
    }
}
