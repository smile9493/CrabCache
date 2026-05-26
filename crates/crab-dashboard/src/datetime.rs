//! Timestamps for the dashboard in China Standard Time (UTC+8, 北京时间).

pub use crate::types::{
    format_beijing_datetime_ms, format_beijing_datetime_secs_ms, format_beijing_hour_label,
    normalize_epoch_ms,
};

/// `2026-05-26 04:40` in Beijing.
pub fn format_ms_china_datetime(ms: u64) -> String {
    format_beijing_datetime_ms(ms)
}

/// `2026-05-26 04:40:23` in Beijing.
pub fn format_ms_china_datetime_secs(ms: u64) -> String {
    format_beijing_datetime_secs_ms(ms)
}

/// Hour bucket label for trend charts: `21:00` (Beijing).
pub fn format_ms_china_hour_label(ms: u64) -> String {
    format_beijing_hour_label(ms)
}

/// Short clock label for charts: `21:39` or `21:39:05`.
pub fn format_ms_china_time(ms: u64, with_seconds: bool) -> String {
    if with_seconds {
        format_beijing_datetime_secs_ms(ms)
            .split_whitespace()
            .nth(1)
            .map(str::to_string)
            .unwrap_or_else(|| format_beijing_datetime_ms(ms))
    } else {
        format_beijing_datetime_ms(ms)
            .split_whitespace()
            .nth(1)
            .map(str::to_string)
            .unwrap_or_else(|| format_beijing_datetime_ms(ms))
    }
}
