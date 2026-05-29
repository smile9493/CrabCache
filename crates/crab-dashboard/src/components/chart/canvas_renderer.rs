//! Canvas-side implementation of the shared chart renderer trait.

use plotters::prelude::*;
use plotters_canvas::CanvasBackend;
use web_sys::HtmlCanvasElement;

use crate::components::chart::canvas_render::size_canvas_to_css;
use crate::components::chart::core::value_segments_indexed;
use crate::components::chart::renderer::{BarDrawRequest, ChartRenderer, LineDrawRequest};
use crate::components::chart::theme::{ChartPalette, resolve_series_color};
use crate::theme::Theme;

const MARGIN: u32 = 8;

fn mesh_label(palette: &ChartPalette) -> TextStyle<'_> {
    TextStyle::from(("sans-serif", 11)).color(&palette.muted)
}

pub struct CanvasLineRenderer {
    pub theme: Theme,
}

impl CanvasLineRenderer {
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }
}

impl ChartRenderer for CanvasLineRenderer {
    fn render_line(
        &mut self,
        canvas: &HtmlCanvasElement,
        req: &LineDrawRequest,
    ) -> Result<(), String> {
        if req.labels.is_empty() || req.series.is_empty() {
            return Err("empty line draw request".to_string());
        }

        let palette = ChartPalette::for_theme(self.theme);
        size_canvas_to_css(canvas, req.width_px, req.height_px);
        let backend =
            CanvasBackend::with_canvas_object(canvas.clone()).ok_or("failed to create backend")?;
        let root = backend.into_drawing_area();
        root.fill(&palette.bg).ok();

        // Compute Y range — use the same logic as `y_range()` in core.rs
        // so that canvas coordinates match tooltip hover calculations.
        let (auto_ymin, auto_ymax) = {
            let mut ymin = f64::MAX;
            let mut ymax = f64::MIN;
            for s in &req.series {
                for v in &s.values {
                    if let Some(x) = v {
                        if *x > 0.0 && x.is_finite() {
                            ymin = ymin.min(*x);
                            ymax = ymax.max(*x);
                        }
                    }
                }
            }
            if ymax <= ymin {
                ymin = 0.0;
                ymax = 1.0;
            }
            let pad = (ymax - ymin) * 0.1;
            ((ymin - pad).max(0.0), ymax + pad)
        };
        let ymin = req.y_min.unwrap_or(auto_ymin);
        let ymax = req.y_max.unwrap_or(auto_ymax);

        let n = req.labels.len();
        let x_max = (n.saturating_sub(1)).max(1) as f64;

        let mut chart = ChartBuilder::on(&root)
            .margin(MARGIN)
            .set_all_label_area_size(36)
            .build_cartesian_2d(0.0f64..x_max, ymin..ymax)
            .map_err(|e| e.to_string())?;

        chart
            .configure_mesh()
            .max_light_lines(4)
            .bold_line_style(palette.grid.mix(0.2))
            .light_line_style(palette.grid.mix(0.1))
            .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
            .label_style(mesh_label(&palette))
            .x_label_formatter(&|x| {
                let idx = *x as usize;
                req.labels.get(idx).cloned().unwrap_or_default()
            })
            .y_label_formatter(&|y| format!("{:.0}{}", y, req.y_unit))
            .draw()
            .map_err(|e| e.to_string())?;

        // Threshold reference lines.
        for t in &req.thresholds {
            let color = resolve_series_color(t.color, &palette);
            let style = color.mix(0.6).stroke_width(1);
            let _ = chart.draw_series(std::iter::once(PathElement::new(
                vec![(0.0f64, t.value), (x_max, t.value)],
                style,
            )));
        }

        // Area fills first (below lines).
        for s in &req.series {
            if !s.fill {
                continue;
            }
            let segments = value_segments_indexed(&s.values);
            for (start_idx, seg_vals) in segments {
                let color = resolve_series_color(&s.color, &palette);
                let area_data: Vec<(f64, f64)> = seg_vals
                    .iter()
                    .enumerate()
                    .map(|(i, v)| ((start_idx + i) as f64, *v))
                    .collect();
                if area_data.len() >= 2 {
                    let _ = chart.draw_series(AreaSeries::new(
                        area_data.into_iter(),
                        ymin,
                        color.mix(0.12),
                    ));
                }
            }
        }

        // Data lines.
        for s in &req.series {
            let segments = value_segments_indexed(&s.values);
            for (start_idx, seg_vals) in segments {
                let color = resolve_series_color(&s.color, &palette);
                let line_data: Vec<(f64, f64)> = seg_vals
                    .iter()
                    .enumerate()
                    .map(|(i, v)| ((start_idx + i) as f64, *v))
                    .collect();
                if line_data.len() >= 2 {
                    let style = color.mix(0.85).stroke_width(2);
                    let _ = chart.draw_series(LineSeries::new(line_data.into_iter(), style));
                }
            }
        }

        drop(chart);
        root.present().ok();
        Ok(())
    }

    fn render_bar(
        &mut self,
        canvas: &HtmlCanvasElement,
        req: &BarDrawRequest,
    ) -> Result<(), String> {
        if req.labels.is_empty() || req.series.is_empty() {
            return Err("empty bar draw request".to_string());
        }

        let palette = ChartPalette::for_theme(self.theme);
        size_canvas_to_css(canvas, req.width_px, req.height_px);
        let backend =
            CanvasBackend::with_canvas_object(canvas.clone()).ok_or("failed to create backend")?;
        let root = backend.into_drawing_area();
        root.fill(&palette.bg).ok();

        let n = req.labels.len();
        let mut ymax = 0.0f64;
        for s in &req.series {
            for v in s.values.iter().flatten() {
                if v.is_finite() && *v > ymax {
                    ymax = *v;
                }
            }
        }
        ymax = (ymax * 1.1).max(1.0);

        let x_max = n.max(1) as f64;

        let mut chart = ChartBuilder::on(&root)
            .margin(MARGIN)
            .set_all_label_area_size(36)
            .build_cartesian_2d(0.0f64..x_max, 0.0f64..ymax)
            .map_err(|e| e.to_string())?;

        chart
            .configure_mesh()
            .max_light_lines(4)
            .bold_line_style(palette.grid.mix(0.2))
            .light_line_style(palette.grid.mix(0.1))
            .axis_style(ShapeStyle::from(&palette.muted).stroke_width(1))
            .label_style(mesh_label(&palette))
            .x_label_formatter(&|x| {
                let idx = *x as usize;
                req.labels.get(idx).cloned().unwrap_or_default()
            })
            .y_label_formatter(&|y| format!("{:.0}{}", y, req.y_unit))
            .draw()
            .map_err(|e| e.to_string())?;

        let n_series = req.series.len().max(1);
        let bucket_w = 0.8;
        let bar_w = bucket_w / n_series as f64;

        for (j, s) in req.series.iter().enumerate() {
            let color = resolve_series_color(&s.color, &palette);
            for (i, v) in s.values.iter().enumerate() {
                let Some(val) = v.filter(|v| v.is_finite() && *v > 0.0) else {
                    continue;
                };
                let x0 = i as f64 + (1.0 - bucket_w) / 2.0 + j as f64 * bar_w;
                let x1 = x0 + bar_w * 0.9;
                let _ = chart.draw_series(std::iter::once(Rectangle::new(
                    [(x0, 0.0f64), (x1, val)],
                    color.mix(0.85).filled(),
                )));
            }
        }

        drop(chart);
        root.present().ok();
        Ok(())
    }
}
