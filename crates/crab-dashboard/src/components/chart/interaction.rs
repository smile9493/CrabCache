//! Hover overlay helpers for native SVG charts (tooltips, crosshair bands).

use super::core::ChartSeries;
use super::format_tooltip_value;

/// Format a numeric value with full precision for financial tooltips.
pub fn format_precise_value(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.2}", v)
    } else if v >= 1.0 {
        format!("{:.2}", v)
    } else if v >= 0.01 {
        format!("{:.4}", v)
    } else {
        format!("{:.6}", v)
    }
}

/// Tooltip rows for a bucket index: (series_name, formatted_value, color, raw).
pub fn bucket_tooltip_rows(
    series: &[ChartSeries],
    idx: usize,
) -> Vec<(String, String, String, f64)> {
    bucket_tooltip_rows_with_pricing(series, idx, &[])
}

/// Like [`bucket_tooltip_rows`], but appends an estimated USD cost when `price_per_million[i]` is set.
pub fn bucket_tooltip_rows_with_pricing(
    series: &[ChartSeries],
    idx: usize,
    price_per_million: &[Option<f64>],
) -> Vec<(String, String, String, f64)> {
    series
        .iter()
        .enumerate()
        .filter_map(|(si, s)| {
            let v = s.values.get(idx).and_then(|opt| *opt)?;
            let formatted = match price_per_million.get(si).and_then(|p| *p) {
                Some(price) if price > 0.0 => {
                    let cost = (v / 1_000_000.0) * price;
                    format!("{} · ${:.4}", format_precise_value(v), cost)
                }
                _ => format_precise_value(v),
            };
            Some((s.label.clone(), formatted, s.color.clone(), v))
        })
        .collect()
}

/// Horizontal position percent for bucket `idx` of `n` categories (center of bucket).
pub fn bucket_center_pct(idx: usize, n: usize) -> f64 {
    if n == 0 {
        return 50.0;
    }
    (idx as f64 + 0.5) / n as f64 * 100.0
}

/// Bucket width as percent of plot width.
pub fn bucket_width_pct(n: usize) -> f64 {
    if n == 0 {
        return 100.0;
    }
    100.0 / n as f64
}

/// X center percent for line chart index (0..n-1 mapped to 0..100%).
pub fn line_center_pct(idx: usize, n: usize) -> f64 {
    if n <= 1 {
        50.0
    } else {
        idx as f64 / (n - 1) as f64 * 100.0
    }
}

/// Highlight band `(left%, width%)` for line chart hover column.
pub fn line_band_style(idx: usize, n: usize) -> (f64, f64) {
    if n <= 1 {
        return (25.0, 50.0);
    }
    let center = line_center_pct(idx, n);
    let w = 100.0 / (n - 1) as f64 * 0.8;
    ((center - w / 2.0).max(0.0), w.min(100.0))
}

/// Tooltip position for line chart (uses line spacing, not bucket centers).
pub fn line_tooltip_position_style(idx: usize, n: usize) -> String {
    let pct = line_center_pct(idx, n);
    if pct > 70.0 {
        format!("right: {:.1}%", 100.0 - pct)
    } else {
        format!("left: {:.1}%", pct)
    }
}

/// Tooltip side and CSS position for a bucket index.
pub fn tooltip_position_style(idx: usize, n: usize) -> String {
    let pct = bucket_center_pct(idx, n);
    let side = if pct > 70.0 { "right" } else { "left" };
    if side == "right" {
        format!("right: {:.1}%", 100.0 - pct)
    } else {
        format!("left: {:.1}%", pct)
    }
}

/// Max finite positive value in column `idx` across series.
pub fn column_max_value(series: &[ChartSeries], idx: usize) -> Option<f64> {
    series
        .iter()
        .filter_map(|s| s.values.get(idx).and_then(|opt| *opt))
        .filter(|v| v.is_finite() && *v > 0.0)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

/// Y-axis label top percent from value (0 = top of plot, 100 = bottom).
pub fn value_top_pct(val: f64, ymin: f64, ymax: f64) -> f64 {
    let span = (ymax - ymin).max(1.0);
    let pct = ((val - ymin) / span).clamp(0.0, 1.0);
    (1.0 - pct) * 100.0
}

/// Build tooltip at index using shared formatter.
pub fn series_tooltip_at(
    labels: &[String],
    series: &[ChartSeries],
    idx: Option<usize>,
) -> Option<(String, Vec<(String, String, String)>)> {
    let idx = idx?;
    if idx >= labels.len() {
        return None;
    }
    let label = labels[idx].clone();
    let values: Vec<(String, String, String)> = series
        .iter()
        .filter_map(|s| {
            let v = s.values.get(idx).and_then(|opt| *opt)?;
            Some((s.label.clone(), format_tooltip_value(v), s.color.clone()))
        })
        .collect();
    if values.is_empty() {
        None
    } else {
        Some((label, values))
    }
}
