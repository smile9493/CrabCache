//! Render charts to SVG strings via Plotters (`SVGBackend::with_string`).

use plotters::element::Pie;
use plotters::prelude::*;
use plotters::series::{AreaSeries, LineSeries};

use super::core::{
    ChartSeries, DonutSegment, ScatterPoint, ThresholdLine, WaterfallStage, scatter_range, y_range,
};
use super::theme::{ChartPalette, resolve_series_color};
use crate::theme::Theme;

pub const CHART_WIDTH: u32 = 640;
pub const CHART_HEIGHT: u32 = 260;

fn mesh_label(palette: &ChartPalette) -> TextStyle<'_> {
    TextStyle::from(("sans-serif", 12)).color(&palette.muted)
}

fn line_stroke(color: RGBColor, dashed: bool) -> ShapeStyle {
    if dashed {
        color.mix(0.55).stroke_width(2)
    } else {
        ShapeStyle::from(&color).stroke_width(2)
    }
}

pub fn render_line_chart(
    labels: &[String],
    series: &[ChartSeries],
    theme: Theme,
    thresholds: &[ThresholdLine],
) -> Option<String> {
    if labels.is_empty() || series.is_empty() {
        return None;
    }
    let has_point = series
        .iter()
        .any(|s| s.values.iter().any(|v| matches!(v, Some(x) if *x > 0.0)));
    if !has_point {
        return None;
    }

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let (ymin, ymax) = y_range(series);
    let n = labels.len();
    let x_end = n.saturating_sub(1).max(1) as f64;

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .set_all_label_area_size(42)
        .build_cartesian_2d(0.0..x_end, ymin..ymax)
        .ok()?;

    chart
        .configure_mesh()
        .max_light_lines(4)
        .bold_line_style(palette.grid.mix(0.25))
        .light_line_style(palette.grid.mix(0.12))
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .x_label_formatter(&|x| {
            labels
                .get(*x as usize)
                .cloned()
                .unwrap_or_default()
        })
        .x_labels(6.min(n))
        .y_labels(5)
        .draw()
        .ok()?;

    for t in thresholds {
        let color = resolve_series_color(t.color, &palette);
        let _ = chart.draw_series(std::iter::once(PathElement::new(
            vec![(0.0, t.value), (x_end, t.value)],
            color.mix(0.7).stroke_width(1),
        )));
    }

    for s in series {
        let color = resolve_series_color(s.color, &palette);
        let points: Vec<(f64, f64)> = s
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.map(|y| (i as f64, y)))
            .collect();
        if points.is_empty() {
            continue;
        }
        if s.fill {
            let _ = chart.draw_series(AreaSeries::new(
                points.iter().copied(),
                ymin,
                color.mix(0.18).filled(),
            ));
        }
        let _ = chart.draw_series(LineSeries::new(points, line_stroke(color, s.dashed)));
    }

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

pub fn render_stacked_bar_chart(
    labels: &[String],
    series: &[ChartSeries],
    theme: Theme,
) -> Option<String> {
    if labels.is_empty() || series.is_empty() {
        return None;
    }
    let has_point = series.iter().any(|s| {
        s.values
            .iter()
            .any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite()))
    });
    if !has_point {
        return None;
    }

    // Compute stacked max for Y range.
    let n = labels.len();
    let stacked_max = (0..n)
        .map(|i| {
            series
                .iter()
                .filter_map(|s| s.values.get(i).and_then(|v| *v))
                .filter(|v| v.is_finite() && *v > 0.0)
                .sum::<f64>()
        })
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let ymax = stacked_max * 1.1;

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let x_end = n.saturating_sub(1).max(1) as f64;

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .set_all_label_area_size(42)
        .build_cartesian_2d(0.0..x_end, 0.0..ymax)
        .ok()?;

    chart
        .configure_mesh()
        .max_light_lines(4)
        .bold_line_style(palette.grid.mix(0.25))
        .light_line_style(palette.grid.mix(0.12))
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .x_label_formatter(&|x| {
            labels
                .get(*x as usize)
                .cloned()
                .unwrap_or_default()
        })
        .x_labels(6.min(n))
        .y_labels(5)
        .draw()
        .ok()?;

    let bar_w = 0.72;
    for (i, _) in labels.iter().enumerate() {
        let mut cum = 0.0;
        for s in series {
            let Some(v) = s.values.get(i).and_then(|opt| *opt) else {
                continue;
            };
            if !(v > 0.0 && v.is_finite()) {
                continue;
            }
            let color = resolve_series_color(s.color, &palette).mix(0.88).filled();
            let x0 = i as f64 - bar_w / 2.0;
            let x1 = i as f64 + bar_w / 2.0;
            let y0 = cum;
            let y1 = cum + v;
            let _ = chart.draw_series(std::iter::once(Rectangle::new(
                [(x0, y0), (x1, y1)],
                color,
            )));
            cum = y1;
        }
    }

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

pub fn render_bar_chart(
    labels: &[String],
    series: &[ChartSeries],
    theme: Theme,
) -> Option<String> {
    if labels.is_empty() || series.is_empty() {
        return None;
    }
    let has_point = series.iter().any(|s| {
        s.values
            .iter()
            .any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite()))
    });
    if !has_point {
        return None;
    }

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let (ymin, ymax) = y_range(series);
    let n = labels.len();
    let x_end = n.saturating_sub(1).max(1) as f64;
    let series_count = series.len().max(1) as f64;
    let group_w = 0.8 / series_count;
    let bar_w = group_w * 0.82;

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .set_all_label_area_size(42)
        .build_cartesian_2d(0.0..x_end, ymin..ymax)
        .ok()?;

    chart
        .configure_mesh()
        .max_light_lines(4)
        .bold_line_style(palette.grid.mix(0.25))
        .light_line_style(palette.grid.mix(0.12))
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .x_label_formatter(&|x| {
            labels
                .get(*x as usize)
                .cloned()
                .unwrap_or_default()
        })
        .x_labels(6.min(n))
        .y_labels(5)
        .draw()
        .ok()?;

    for (j, s) in series.iter().enumerate() {
        let color = resolve_series_color(s.color, &palette).mix(0.88).filled();
        let offset = -0.4 + j as f64 * group_w + (group_w - bar_w) / 2.0;
        let rects: Vec<Rectangle<(f64, f64)>> = s
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let y = v.as_ref().copied()?;
                if !(y > 0.0 && y.is_finite()) {
                    return None;
                }
                let x0 = i as f64 + offset;
                let x1 = x0 + bar_w;
                Some(Rectangle::new([(x0, ymin), (x1, y)], color))
            })
            .collect();
        let _ = chart.draw_series(rects);
    }

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

struct HistBin {
    range_start: f64,
    count: usize,
}

fn auto_bins(values: &[f64], bin_count: usize) -> Vec<HistBin> {
    if values.is_empty() || bin_count == 0 {
        return Vec::new();
    }
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    for &v in values {
        if v.is_finite() {
            min = min.min(v);
            max = max.max(v);
        }
    }
    if max <= min {
        max = min + 1.0;
    }
    let span = max - min;
    let bin_width = span / bin_count as f64;
    let mut counts = vec![0usize; bin_count];
    for &v in values {
        if !v.is_finite() {
            continue;
        }
        let idx = ((v - min) / bin_width).floor() as usize;
        counts[idx.min(bin_count - 1)] += 1;
    }
    (0..bin_count)
        .map(|i| HistBin {
            range_start: min + i as f64 * bin_width,
            count: counts[i],
        })
        .collect()
}

pub fn render_histogram(values: &[f64], theme: Theme, bin_count: usize) -> Option<String> {
    let values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    let bins = auto_bins(&values, bin_count);
    if bins.is_empty() || bins.iter().all(|b| b.count == 0) {
        return None;
    }

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let max_count = bins.iter().map(|b| b.count).max().unwrap_or(1) as f64;
    let n = bins.len();
    let x_end = n.saturating_sub(1).max(1) as f64;

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .set_all_label_area_size(42)
        .build_cartesian_2d(0.0..x_end, 0.0..max_count)
        .ok()?;

    chart
        .configure_mesh()
        .max_light_lines(4)
        .bold_line_style(palette.grid.mix(0.25))
        .light_line_style(palette.grid.mix(0.12))
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .x_label_formatter(&|x| {
            bins.get(*x as usize)
                .map(|b| format!("{:.0}", b.range_start))
                .unwrap_or_default()
        })
        .x_labels(6.min(n))
        .y_labels(5)
        .draw()
        .ok()?;

    let bar_w = 0.72;
    let rects: Vec<Rectangle<(f64, f64)>> = bins
        .iter()
        .enumerate()
        .filter_map(|(i, bin)| {
            if bin.count == 0 {
                return None;
            }
            let x0 = i as f64 - bar_w / 2.0;
            let x1 = i as f64 + bar_w / 2.0;
            Some(Rectangle::new(
                [(x0, 0.0), (x1, bin.count as f64)],
                palette.accent.mix(0.8).filled(),
            ))
        })
        .collect();
    let _ = chart.draw_series(rects);

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

pub fn render_donut(segments: &[DonutSegment], theme: Theme, size: u32) -> Option<String> {
    if segments.is_empty() {
        return None;
    }
    let total: f64 = segments.iter().map(|s| s.value).sum();
    if total <= 0.0 {
        return None;
    }

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (size, size)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let sizes: Vec<f64> = segments.iter().map(|s| s.value).collect();
    let colors: Vec<RGBColor> = segments
        .iter()
        .map(|s| resolve_series_color(s.color, &palette))
        .collect();
    let labels: Vec<&str> = segments.iter().map(|s| s.label.as_str()).collect();

    let center = (size as i32 / 2, size as i32 / 2);
    let radius = size as f64 * 0.36;
    let mut pie = Pie::new(&center, &radius, &sizes, &colors, &labels);
    pie.start_angle(-90.0);
    pie.donut_hole(radius * 0.58);

    root.draw(&pie).ok()?;
    root.present().ok()?;
    drop(root);
    Some(buf)
}

pub fn render_sparkline(values: &[f64], theme: Theme, width: u32, height: u32) -> Option<String> {
    let valid: Vec<f64> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    if valid.len() < 2 {
        return None;
    }

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (width, height)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let ymin = valid.iter().cloned().fold(f64::MAX, f64::min);
    let ymax = valid.iter().cloned().fold(f64::MIN, f64::max);
    let span = (ymax - ymin).max(1.0);
    let x_end = valid.len().saturating_sub(1).max(1) as f64;

    let mut chart = ChartBuilder::on(&root)
        .margin(2)
        .build_cartesian_2d(0.0..x_end, ymin..(ymin + span))
        .ok()?;

    chart.configure_mesh().disable_mesh().draw().ok()?;

    let points: Vec<(f64, f64)> = valid
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v))
        .collect();
    let _ = chart.draw_series(LineSeries::new(
        points,
        ShapeStyle::from(&palette.accent).stroke_width(2),
    ));

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

fn waterfall_bar_color(ms: f64, palette: &ChartPalette) -> RGBColor {
    if ms < 10.0 {
        palette.success
    } else if ms < 100.0 {
        palette.warning
    } else {
        palette.error
    }
}

pub fn render_scatter(
    points: &[ScatterPoint],
    theme: Theme,
    x_desc: &str,
    y_desc: &str,
    fit_line: Option<(f64, f64)>,
) -> Option<String> {
    if points.is_empty() {
        return None;
    }
    let (xmin, xmax, ymin, ymax) = scatter_range(points);
    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let mut chart = ChartBuilder::on(&root)
        .margin(10)
        .set_all_label_area_size(48)
        .build_cartesian_2d(xmin..xmax, ymin..ymax)
        .ok()?;

    chart
        .configure_mesh()
        .max_light_lines(4)
        .bold_line_style(palette.grid.mix(0.25))
        .light_line_style(palette.grid.mix(0.12))
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .x_desc(x_desc)
        .y_desc(y_desc)
        .draw()
        .ok()?;

    // Optional regression fit line
    if let Some((slope, intercept)) = fit_line {
        let color = palette.warning.mix(0.7);
        let line_style = color.stroke_width(2);
        let _ = chart.draw_series(std::iter::once(PathElement::new(
            vec![(xmin, slope * xmin + intercept), (xmax, slope * xmax + intercept)],
            line_style,
        )));
    }

    for p in points {
        if !(p.x.is_finite() && p.y.is_finite()) {
            continue;
        }
        let color = resolve_series_color(p.color, &palette).mix(0.85).filled();
        let _ = chart.draw_series(std::iter::once(Circle::new((p.x, p.y), 4, color)));
    }

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

pub fn render_waterfall(stages: &[WaterfallStage], theme: Theme) -> Option<String> {
    if stages.is_empty() {
        return None;
    }
    let max_ms = stages
        .iter()
        .map(|s| s.duration_ms)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let n = stages.len();
    let height = (n as u32 * 32 + 56).clamp(120, 320);
    let width = 520u32;

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (width, height)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let y_end = n as f64;
    let mut chart = ChartBuilder::on(&root)
        .margin(8)
        .set_left_and_bottom_label_area_size(52)
        .build_cartesian_2d(0.0..max_ms, 0.0..y_end)
        .ok()?;

    chart
        .configure_mesh()
        .disable_x_mesh()
        .max_light_lines(3)
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .y_label_formatter(&|y| {
            stages
                .get(*y as usize)
                .map(|s| s.label.clone())
                .unwrap_or_default()
        })
        .y_labels(n)
        .x_desc("ms")
        .draw()
        .ok()?;

    let bar_h = 0.72;
    let rects: Vec<Rectangle<(f64, f64)>> = stages
        .iter()
        .enumerate()
        .map(|(i, stage)| {
            let y0 = i as f64 + (1.0 - bar_h) / 2.0;
            let y1 = y0 + bar_h;
            let color = waterfall_bar_color(stage.duration_ms, &palette).mix(0.88).filled();
            Rectangle::new([(0.0, y0), (stage.duration_ms.max(0.5), y1)], color)
        })
        .collect();
    let _ = chart.draw_series(rects);

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}

pub fn render_horizontal_bar_chart(
    labels: &[String],
    values: &[f64],
    theme: Theme,
    width: u32,
    height: u32,
) -> Option<String> {
    if labels.is_empty() || values.is_empty() {
        return None;
    }
    let has_val = values.iter().any(|v| *v > 0.0 && v.is_finite());
    if !has_val {
        return None;
    }

    let n = labels.len();
    let max_val = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let x_max = max_val * 1.1;

    let palette = ChartPalette::for_theme(theme);
    let mut buf = String::new();
    let root = SVGBackend::with_string(&mut buf, (width, height)).into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let y_end = n as f64;
    let mut chart = ChartBuilder::on(&root)
        .margin(8)
        .set_left_and_bottom_label_area_size(52)
        .build_cartesian_2d(0.0..x_max, 0.0..y_end)
        .ok()?;

    chart
        .configure_mesh()
        .disable_x_mesh()
        .max_light_lines(3)
        .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
        .label_style(mesh_label(&palette))
        .y_label_formatter(&|y| {
            labels
                .get(*y as usize)
                .cloned()
                .unwrap_or_default()
        })
        .y_labels(n)
        .draw()
        .ok()?;

    let bar_h = 0.72;
    let rects: Vec<Rectangle<(f64, f64)>> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0 && v.is_finite())
        .map(|(i, &v)| {
            let y0 = i as f64 + (1.0 - bar_h) / 2.0;
            let y1 = y0 + bar_h;
            let color = palette.accent.mix(0.85).filled();
            Rectangle::new([(0.0, y0), (v, y1)], color)
        })
        .collect();
    let _ = chart.draw_series(rects);

    drop(chart);
    root.present().ok()?;
    drop(root);
    Some(buf)
}
