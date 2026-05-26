//! Beijing (UTC+8) display formatting for capture / admin UI.

use chrono::{DateTime, FixedOffset};

fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("valid UTC+8 offset")
}

/// Unix seconds stored as `timestamp_ms` (legacy) → true milliseconds.
pub fn normalize_epoch_ms(ms: u64) -> u64 {
    if ms > 0 && ms < 1_000_000_000_000 {
        ms.saturating_mul(1000)
    } else {
        ms
    }
}

/// `2026-05-26 04:40` in China Standard Time.
pub fn format_beijing_datetime_ms(ms: u64) -> String {
    let ms = normalize_epoch_ms(ms);
    DateTime::from_timestamp_millis(ms as i64)
        .map(|utc| {
            utc.with_timezone(&beijing_offset())
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}

/// Hour bucket label: `04:00` (Beijing).
pub fn format_beijing_hour_label(ms: u64) -> String {
    let ms = normalize_epoch_ms(ms);
    DateTime::from_timestamp_millis(ms as i64)
        .map(|utc| {
            utc.with_timezone(&beijing_offset())
                .format("%H:00")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}

/// `2026-05-26 04:40:45` in China Standard Time.
pub fn format_beijing_datetime_secs_ms(ms: u64) -> String {
    let ms = normalize_epoch_ms(ms);
    DateTime::from_timestamp_millis(ms as i64)
        .map(|utc| {
            utc.with_timezone(&beijing_offset())
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beijing_from_unix_millis() {
        // 2026-05-25 20:40:45 UTC → 2026-05-26 04:40 Beijing
        assert_eq!(
            format_beijing_datetime_ms(1_779_741_645_547),
            "2026-05-26 04:40"
        );
    }

    #[test]
    fn normalize_seconds_to_millis() {
        assert_eq!(normalize_epoch_ms(1_779_741_645), 1_779_741_645_000);
    }
}
