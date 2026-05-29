//! Render charts onto HTML5 Canvas via Plotters (`CanvasBackend`).
//!
//! Drop-in replacements for `svg_render` functions targeting scatter, waterfall,
//! and horizontal-bar charts. Donut and sparkline remain on SVG (small DOM).

use plotters::prelude::*;
use plotters_canvas::CanvasBackend;
use web_sys::HtmlCanvasElement;

use super::core::{ScatterPoint, WaterfallStage, scatter_range};
use super::theme::{ChartPalette, resolve_series_color};
use crate::theme::Theme;

const MARGIN: u32 = 10;

fn mesh_label(palette: &ChartPalette) -> TextStyle<'_> {
    TextStyle::from(("sans-serif", 12)).color(&palette.muted)
}

/// Resize a canvas to its CSS layout size scaled by `devicePixelRatio`.
/// Returns `(logical_w, logical_h)` in CSS pixels.
pub(crate) fn size_canvas_to_css(
    canvas: &HtmlCanvasElement,
    default_w: u32,
    default_h: u32,
) -> (u32, u32) {
    let rect = canvas.get_bounding_client_rect();
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0)
        .max(1.0);
    let css_w = if rect.width() > 0.0 {
        rect.width() as u32
    } else {
        default_w
    };
    let css_h = if rect.height() > 0.0 {
        rect.height() as u32
    } else {
        default_h
    };
    canvas.set_width((css_w as f64 * dpr) as u32);
    canvas.set_height((css_h as f64 * dpr) as u32);
    (css_w, css_h)
}

// ---------------------------------------------------------------------------
// Scatter
// ---------------------------------------------------------------------------

pub fn render_scatter(
    canvas: &HtmlCanvasElement,
    points: &[ScatterPoint],
    theme: Theme,
    x_desc: &str,
    y_desc: &str,
    fit_line: Option<(f64, f64)>,
) -> Option<()> {
    if points.is_empty() {
        return None;
    }
    let (xmin, xmax, ymin, ymax) = scatter_range(points);
    let palette = ChartPalette::for_theme(theme);
    size_canvas_to_css(
        canvas,
        super::svg_render::CHART_WIDTH,
        super::svg_render::CHART_HEIGHT,
    );
    let backend = CanvasBackend::with_canvas_object(canvas.clone())?;
    let root = backend.into_drawing_area();
    root.fill(&palette.bg).ok()?;

    let mut chart = ChartBuilder::on(&root)
        .margin(MARGIN)
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

    if let Some((slope, intercept)) = fit_line {
        let color = palette.warning.mix(0.7);
        let _ = chart.draw_series(std::iter::once(PathElement::new(
            vec![
                (xmin, slope * xmin + intercept),
                (xmax, slope * xmax + intercept),
            ],
            color.stroke_width(2),
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
    Some(())
}

// ---------------------------------------------------------------------------
// Waterfall
// ---------------------------------------------------------------------------

pub fn render_waterfall(
    canvas: &HtmlCanvasElement,
    stages: &[WaterfallStage],
    theme: Theme,
) -> Option<()> {
    if stages.is_empty() {
        return None;
    }
    let max_ms = stages
        .iter()
        .map(|s| s.duration_ms)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let n = stages.len();
    let palette = ChartPalette::for_theme(theme);
    size_canvas_to_css(canvas, 520, (n as u32 * 32 + 56).clamp(120, 320));
    let backend = CanvasBackend::with_canvas_object(canvas.clone())?;
    let root = backend.into_drawing_area();
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
    for (i, stage) in stages.iter().enumerate() {
        let y0 = i as f64 + (1.0 - bar_h) / 2.0;
        let y1 = y0 + bar_h;
        let color = waterfall_bar_color(stage.duration_ms, &palette)
            .mix(0.88)
            .filled();
        let _ = chart.draw_series(std::iter::once(Rectangle::new(
            [(0.0, y0), (stage.duration_ms.max(0.5), y1)],
            color,
        )));
    }

    drop(chart);
    root.present().ok()?;
    Some(())
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

// ---------------------------------------------------------------------------
// Horizontal bar
// ---------------------------------------------------------------------------

pub fn render_horizontal_bar_chart(
    canvas: &HtmlCanvasElement,
    labels: &[String],
    values: &[f64],
    theme: Theme,
    default_w: u32,
    default_h: u32,
) -> Option<()> {
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
    size_canvas_to_css(canvas, default_w, default_h);
    let backend = CanvasBackend::with_canvas_object(canvas.clone())?;
    let root = backend.into_drawing_area();
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
        .y_label_formatter(&|y| labels.get(*y as usize).cloned().unwrap_or_default())
        .y_labels(n)
        .draw()
        .ok()?;

        let bar_h = 0.72;
        for (i, &v) in values.iter().enumerate() {
            if !(v > 0.0 && v.is_finite()) {
                continue;
            }
            let y0 = i as f64 + (1.0 - bar_h) / 2.0;
            let y1 = y0 + bar_h;
            let color = palette.nth_series_color(i).mix(0.85).filled();
            let _ = chart.draw_series(std::iter::once(Rectangle::new([(0.0, y0), (v, y1)], color)));
        }

    drop(chart);
    root.present().ok()?;
    Some(())
}
