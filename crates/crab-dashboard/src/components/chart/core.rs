//! Shared chart data types and coordinate math (used by Plotters renderers and hover overlays).

#[derive(Clone, PartialEq)]
pub struct ChartSeries {
    pub label: String,
    pub color: &'static str,
    pub values: Vec<Option<f64>>,
    pub dashed: bool,
    /// When true, Plotters renders a filled area below the line.
    pub fill: bool,
}

#[derive(Clone)]
pub struct ScatterPoint {
    pub x: f64,
    pub y: f64,
    pub color: &'static str,
    pub label: String,
}

#[derive(Clone)]
pub struct WaterfallStage {
    pub label: String,
    pub duration_ms: f64,
}

pub fn scatter_range(points: &[ScatterPoint]) -> (f64, f64, f64, f64) {
    let mut xmin = f64::MAX;
    let mut xmax = f64::MIN;
    let mut ymin = f64::MAX;
    let mut ymax = f64::MIN;
    for p in points {
        if p.x.is_finite() && p.y.is_finite() {
            xmin = xmin.min(p.x);
            xmax = xmax.max(p.x);
            ymin = ymin.min(p.y);
            ymax = ymax.max(p.y);
        }
    }
    if xmax <= xmin {
        xmax = xmin + 1.0;
    }
    if ymax <= ymin {
        ymax = ymin + 1.0;
    }
    let pad_x = (xmax - xmin) * 0.1;
    let pad_y = (ymax - ymin) * 0.1;
    (
        (xmin - pad_x).max(0.0),
        xmax + pad_x,
        (ymin - pad_y).max(0.0),
        ymax + pad_y,
    )
}

/// Largest-Triangle-Three-Buckets (LTTB) downsampling for time series data.
/// Reduces `points` to at most `target_count` points while preserving visual shape.
/// Points are `(x, y)` tuples where x is typically a timestamp.
pub fn downsample_lttb(points: &[(f64, f64)], target_count: usize) -> Vec<(f64, f64)> {
    if points.len() <= target_count || target_count < 3 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(target_count);
    result.push(points[0]); // Always include first point.

    let bucket_size = (points.len() - 2) as f64 / (target_count - 2) as f64;

    let mut prev_selected = 0usize;

    for i in 0..target_count - 2 {
        // Bucket boundaries (for the "next" bucket used in area calculation).
        let bucket_start = ((i + 1) as f64 * bucket_size).floor() as usize + 1;
        let bucket_end = (((i + 2) as f64 * bucket_size).floor() as usize + 1)
            .min(points.len() - 1);

        // Average of next bucket (for area calculation).
        let (avg_x, avg_y) = {
            let mut sx = 0.0f64;
            let mut sy = 0.0f64;
            let count = (bucket_end - bucket_start + 1) as f64;
            for p in &points[bucket_start..=bucket_end] {
                sx += p.0;
                sy += p.1;
            }
            (sx / count, sy / count)
        };

        // Current bucket boundaries.
        let cur_start = (i as f64 * bucket_size).floor() as usize + 1;
        let cur_end = (((i + 1) as f64 * bucket_size).floor() as usize + 1)
            .min(points.len() - 1);

        // Find point in current bucket with largest triangle area.
        let (px, py) = points[prev_selected];
        let mut max_area = -1.0f64;
        let mut max_idx = cur_start;

        for (j, &(cx, cy)) in points[cur_start..=cur_end].iter().enumerate() {
            let area = ((px - avg_x) * (cy - py) - (px - cx) * (avg_y - py)).abs();
            if area > max_area {
                max_area = area;
                max_idx = cur_start + j;
            }
        }

        result.push(points[max_idx]);
        prev_selected = max_idx;
    }

    result.push(points[points.len() - 1]); // Always include last point.
    result
}

/// Downsample a `ChartSeries`'s values to at most `target_count` points using LTTB.
/// Returns a new Vec<Option<f64>> with `None` values preserved as gaps.
pub fn downsample_series(values: &[Option<f64>], target_count: usize) -> Vec<Option<f64>> {
    if values.len() <= target_count {
        return values.to_vec();
    }

    // Build contiguous (index, value) pairs for non-None values.
    let pairs: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|y| (i as f64, y)))
        .collect();

    if pairs.len() <= target_count {
        // Fewer real points than target — just clone.
        return values.to_vec();
    }

    let downsampled = downsample_lttb(&pairs, target_count);

    // Map back to indexed Option<f64>.
    let mut result = vec![None; values.len()];
    for (x, y) in downsampled {
        let idx = x.round() as usize;
        if idx < result.len() {
            result[idx] = Some(y);
        }
    }
    result
}

#[derive(Clone)]
pub struct DonutSegment {
    pub label: String,
    pub value: f64,
    pub color: &'static str,
}

/// Horizontal threshold reference line.
#[derive(Clone)]
pub struct ThresholdLine {
    pub value: f64,
    pub label: String,
    pub color: &'static str,
}

pub fn value_segments_indexed(values: &[Option<f64>]) -> Vec<(usize, Vec<f64>)> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut current = Vec::new();
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(x) if x.is_finite() && *x > 0.0 => {
                if current.is_empty() {
                    start = i;
                }
                current.push(*x);
            }
            _ => {
                if !current.is_empty() {
                    segments.push((start, std::mem::take(&mut current)));
                }
            }
        }
    }
    if !current.is_empty() {
        segments.push((start, current));
    }
    segments
}

pub fn y_range(series: &[ChartSeries]) -> (f64, f64) {
    let mut ymin = f64::MAX;
    let mut ymax = f64::MIN;
    for s in series {
        for v in &s.values {
            if let Some(x) = v
                && *x > 0.0
                && x.is_finite()
            {
                ymin = ymin.min(*x);
                ymax = ymax.max(*x);
            }
        }
    }
    if ymax <= ymin {
        ymin = 0.0;
        ymax = 1.0;
    }
    let pad = (ymax - ymin) * 0.1;
    ((ymin - pad).max(0.0), ymax + pad)
}

/// Convert a mouse event's client position to SVG viewBox X (0..100).
pub fn mouse_to_svg_x(ev: &web_sys::MouseEvent, svg: &web_sys::SvgsvgElement) -> Option<f64> {
    // Use client rect mapping for robust cross-browser behavior in nested flex/grid
    // layouts and modal contexts; CTM-based mapping can become unstable in these cases.
    let rect = svg.get_bounding_client_rect();
    let width = rect.width();
    if width <= f64::EPSILON {
        return None;
    }
    let x = ev.client_x() as f64 - rect.left();
    if x < 0.0 || x > width {
        return None;
    }
    Some((x / width) * 100.0)
}

pub fn format_tooltip_value(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
}

use crate::types::{RequestDetail, RequestLog};
use crate::types::TimeSeriesPoint;

/// Derive waterfall stages from a request log + detail.
///
/// Only `Upstream` and `TTFT` are taken from the API; `Gateway` is estimated
/// as `max(0, latency_ms - upstream - ttft)`. This is NOT distributed tracing.
pub fn waterfall_stages_from_log(
    summary: &RequestLog,
    detail: &RequestDetail,
) -> Vec<WaterfallStage> {
    let total = summary.latency_ms as f64;
    let upstream = detail.upstream_latency_ms.unwrap_or(0.0);
    let ttft = detail.ttft_ms.unwrap_or(0.0);
    let mut stages = Vec::new();

    // Gateway overhead = total - upstream - ttft (may be < 0 if data is noisy)
    let gateway = (total - upstream - ttft).max(0.0);
    if gateway > 0.0 {
        stages.push(WaterfallStage {
            label: "Gateway".to_string(),
            duration_ms: gateway,
        });
    }
    if upstream > 0.0 {
        stages.push(WaterfallStage {
            label: "Upstream".to_string(),
            duration_ms: upstream,
        });
    }
    if ttft > 0.0 && ttft != upstream {
        // TTFT is a sub-phase of upstream; only add if it provides extra info.
        stages.push(WaterfallStage {
            label: "TTFT".to_string(),
            duration_ms: ttft,
        });
    }
    stages
}

/// Points used for mini trend lines (prefers daily, then hourly buckets).
pub fn trend_points(metrics: &crate::types::MetricsSnapshot) -> &[TimeSeriesPoint] {
    if !metrics.daily_stats.is_empty() {
        metrics.daily_stats.as_slice()
    } else {
        metrics.hourly_stats.as_slice()
    }
}

pub fn sparkline_requests(points: &[TimeSeriesPoint]) -> Vec<f64> {
    points.iter().map(|p| p.requests as f64).collect()
}

pub fn sparkline_hit_rate_pct(points: &[TimeSeriesPoint]) -> Vec<f64> {
    points
        .iter()
        .map(|p| {
            if p.hit_rate > 0.0 {
                p.hit_rate * 100.0
            } else if p.requests > 0 {
                p.cache_hits as f64 / p.requests as f64 * 100.0
            } else {
                0.0
            }
        })
        .collect()
}

pub fn sparkline_tokens(points: &[TimeSeriesPoint]) -> Vec<f64> {
    points.iter().map(|p| p.tokens as f64).collect()
}

/// Pick ~max_ticks X label indices (plotters-style mesh thinning).
pub fn x_tick_indices(count: usize, max_ticks: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    if count <= max_ticks {
        return (0..count).collect();
    }
    let step = (count / max_ticks).max(1);
    let mut out: Vec<usize> = (0..count).step_by(step).collect();
    if out.last() != Some(&(count - 1)) {
        out.push(count - 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkline_hit_rate_uses_field_or_ratio() {
        let pts = vec![
            TimeSeriesPoint {
                timestamp: "a".into(),
                requests: 10,
                tokens: 0,
                cache_hits: 8,
                avg_latency_ms: 0.0,
                hit_rate: 0.0,
            },
            TimeSeriesPoint {
                timestamp: "b".into(),
                requests: 0,
                tokens: 0,
                cache_hits: 0,
                avg_latency_ms: 0.0,
                hit_rate: 0.42,
            },
        ];
        let v = sparkline_hit_rate_pct(&pts);
        assert!((v[0] - 80.0).abs() < 0.01);
        assert!((v[1] - 42.0).abs() < 0.01);
    }

    #[test]
    fn y_range_defaults_when_empty() {
        let (lo, hi) = y_range(&[]);
        assert_eq!(lo, 0.0);
        assert!((hi - 1.1).abs() < f64::EPSILON);
    }

    #[test]
    fn x_tick_indices_includes_last() {
        let idx = x_tick_indices(24, 8);
        assert_eq!(*idx.last().unwrap(), 23);
    }

    #[test]
    fn waterfall_stages_from_log_basic() {
        let summary = RequestLog {
            id: "1".into(),
            timestamp: "t".into(),
            model: "m".into(),
            consumer: "c".into(),
            latency_ms: 100,
            total_tokens: 0,
            cache_status: "L0".into(),
            request_payload: "".into(),
            response_preview: "".into(),
            input_tokens: None,
            output_tokens: None,
            ttft_ms: None,
            content_length: None,
            request_hash: None,
            project_id: None,
            upstream_user_id: None,
            user_id_audit: None,
            upstream_key_id: None,
        };
        let detail = RequestDetail {
            cache_path: "".into(),
            request_payload: "".into(),
            response_body: "".into(),
            route_backend: "".into(),
            upstream_latency_ms: Some(60.0),
            ttft_ms: Some(30.0),
            input_tokens: None,
            output_tokens: None,
            request_hash: None,
            semantic_cluster: None,
            upstream_key_id: None,
        };
        let stages = waterfall_stages_from_log(&summary, &detail);
        // Gateway = 100 - 60 - 30 = 10
        assert_eq!(stages.len(), 3);
        assert_eq!(stages[0].label, "Gateway");
        assert!((stages[0].duration_ms - 10.0).abs() < 0.01);
        assert_eq!(stages[1].label, "Upstream");
        assert!((stages[1].duration_ms - 60.0).abs() < 0.01);
        assert_eq!(stages[2].label, "TTFT");
        assert!((stages[2].duration_ms - 30.0).abs() < 0.01);
    }

    #[test]
    fn waterfall_stages_no_upstream_returns_gateway_only() {
        let summary = RequestLog {
            id: "2".into(),
            timestamp: "t".into(),
            model: "m".into(),
            consumer: "c".into(),
            latency_ms: 50,
            total_tokens: 0,
            cache_status: "L0".into(),
            request_payload: "".into(),
            response_preview: "".into(),
            input_tokens: None,
            output_tokens: None,
            ttft_ms: None,
            content_length: None,
            request_hash: None,
            project_id: None,
            upstream_user_id: None,
            user_id_audit: None,
            upstream_key_id: None,
        };
        let detail = RequestDetail {
            cache_path: "".into(),
            request_payload: "".into(),
            response_body: "".into(),
            route_backend: "".into(),
            upstream_latency_ms: None,
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            request_hash: None,
            semantic_cluster: None,
            upstream_key_id: None,
        };
        let stages = waterfall_stages_from_log(&summary, &detail);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].label, "Gateway");
        assert!((stages[0].duration_ms - 50.0).abs() < 0.01);
    }
}
