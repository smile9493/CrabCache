//! Canvas-based line chart component — drop-in replacement for `LineChart`.
//!
//! Renders on HTML5 Canvas via Plotters `CanvasBackend` for better performance
//! with large datasets. Interaction (hover, tooltip) uses positioned HTML overlays.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use crate::components::chart::canvas_renderer::CanvasLineRenderer;
use crate::components::chart::core::{ChartSeries, ThresholdLine, downsample_series, y_range};
use crate::components::chart::interaction::{
    bucket_tooltip_rows_with_pricing, line_band_style, line_center_pct,
    line_tooltip_position_style, value_top_pct,
};
use crate::components::chart::renderer::{ChartRenderer, LineDrawRequest};
use crate::theme::use_theme_signal;

static CANVAS_LINE_CHART_ID: AtomicUsize = AtomicUsize::new(0);

/// Maximum data points before downsampling kicks in.
const MAX_CHART_POINTS: usize = 200;

#[derive(Clone)]
struct ChartGeom {
    labels: Vec<String>,
    all_series: Vec<ChartSeries>,
    ymin: f64,
    ymax: f64,
    n: usize,
}

fn prepare_geom(
    labels: Vec<String>,
    raw_series: Vec<ChartSeries>,
    y_min: Option<f64>,
    y_max: Option<f64>,
) -> Option<ChartGeom> {
    let all_series: Vec<ChartSeries> = if labels.len() > MAX_CHART_POINTS {
        raw_series
            .into_iter()
            .map(|s| ChartSeries {
                label: s.label,
                color: s.color,
                values: downsample_series(&s.values, MAX_CHART_POINTS),
                dashed: s.dashed,
                fill: s.fill,
            })
            .collect()
    } else {
        raw_series
    };
    if labels.is_empty() || all_series.is_empty() {
        return None;
    }
    let has_point = all_series
        .iter()
        .any(|s| s.values.iter().any(|v| matches!(v, Some(x) if *x > 0.0)));
    if !has_point {
        return None;
    }
    let (auto_ymin, auto_ymax) = y_range(&all_series);
    let ymin = y_min.unwrap_or(auto_ymin);
    let ymax = y_max.unwrap_or(auto_ymax);
    let n = labels.len().max(1);
    Some(ChartGeom {
        labels,
        all_series,
        ymin,
        ymax,
        n,
    })
}

#[component]
pub fn CanvasLineChart(
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = "ms")] y_unit: &'static str,
    empty_message: &'static str,
    #[prop(default = Vec::new())] thresholds: Vec<ThresholdLine>,
    #[prop(default = true)] interactive: bool,
    #[prop(default = None)] y_min: Option<f64>,
    #[prop(default = None)] y_max: Option<f64>,
    #[prop(default = Vec::new())] series_price_per_million: Vec<Option<f64>>,
) -> impl IntoView {
    let chart_id = CANVAS_LINE_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("canvas-line-chart-summary-{}", chart_id);
    let theme = use_theme_signal();
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let mouse_pos: RwSignal<Option<(f64, f64)>> = RwSignal::new(None);
    let canvas_ref: NodeRef<leptos::html::Canvas> = NodeRef::new();
    let plot_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let thresholds = std::sync::Arc::new(thresholds);
    let series_price_per_million = std::sync::Arc::new(series_price_per_million);

    // Draw canvas whenever data or theme changes.
    Effect::new({
        let thresholds = std::sync::Arc::clone(&thresholds);
        move |_| {
            let Some(canvas_el) = canvas_ref.get() else {
                return;
            };
            let canvas_dom: web_sys::HtmlCanvasElement = canvas_el.dyn_into().unwrap();
            let _ = theme.get();
            let labels = x_labels.get();
            let raw_series = series.get();
            let Some(geom) = prepare_geom(labels.clone(), raw_series, y_min, y_max) else {
                return;
            };
            let req = LineDrawRequest {
                labels: geom.labels,
                series: geom.all_series,
                y_unit,
                thresholds: (*thresholds).clone(),
                width_px: 640,
                height_px,
            };
            let mut renderer = CanvasLineRenderer::new(theme.get());
            let _ = renderer.render_line(&canvas_dom, &req);
        }
    });

    // Hover: map mouse X to data index (Callback is Clone — safe inside reactive view).
    let pricing_mouse = std::sync::Arc::clone(&series_price_per_million);
    let on_mousemove = leptos::callback::Callback::new(move |ev: web_sys::MouseEvent| {
        if !interactive {
            return;
        }
        let Some(host) = plot_ref.get() else {
            return;
        };
        let rect = host.get_bounding_client_rect();
        let mx = ev.client_x() as f64 - rect.left();
        let my = ev.client_y() as f64 - rect.top();
        mouse_pos.set(Some((mx, my)));

        let container_w = rect.width();
        if container_w <= 0.0 {
            hover_index.set(None);
            return;
        }
        let rel_x = ((ev.client_x() as f64 - rect.left()) / container_w).clamp(0.0, 1.0);

        let labels = x_labels.get_untracked();
        let raw_series = series.get_untracked();
        let Some(geom) = prepare_geom(labels, raw_series, y_min, y_max) else {
            hover_index.set(None);
            return;
        };
        let n = geom.n;
        let idx = (rel_x * (n as f64 - 1.0)).round() as usize;
        let idx = idx.min(n.saturating_sub(1));
        let values =
            bucket_tooltip_rows_with_pricing(&geom.all_series, idx, pricing_mouse.as_ref());
        if values.is_empty() {
            hover_index.set(None);
        } else {
            hover_index.set(Some(idx));
        }
    });

    let on_mouseleave = leptos::callback::Callback::new(move |_: web_sys::MouseEvent| {
        hover_index.set(None);
        mouse_pos.set(None);
    });

    let on_keydown = leptos::callback::Callback::new(move |ev: web_sys::KeyboardEvent| {
        let labels = x_labels.get_untracked();
        let n = labels.len();
        if n == 0 {
            return;
        }
        let current = hover_index.get_untracked();
        let new_idx = match ev.key().as_str() {
            "ArrowLeft" => match current {
                Some(idx) if idx > 0 => Some(idx - 1),
                None => Some(n - 1),
                _ => current,
            },
            "ArrowRight" => match current {
                Some(idx) if idx < n - 1 => Some(idx + 1),
                None => Some(0),
                _ => current,
            },
            "Escape" => None,
            _ => return,
        };
        hover_index.set(new_idx);
        ev.prevent_default();
    });

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 24)>
            {move || {
                let pricing = std::sync::Arc::clone(&series_price_per_million);
                let labels = x_labels.get();
                let raw_series = series.get();
                let geom = prepare_geom(labels, raw_series, y_min, y_max);

                let data_summary = match &geom {
                    None => "Empty line chart.".to_string(),
                    Some(g) => format!(
                        "Line chart with {} series and {} data points.",
                        g.all_series.len(),
                        g.labels.len()
                    ),
                };

                if geom.is_none() {
                    view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any()
                } else {
                    view! {
                        <div class="line-chart-plot" style="position: relative; width: 100%">
                            <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
                            <canvas
                                node_ref=canvas_ref
                                style=format!("width: 100%; height: {}px; cursor: {}", height_px, if interactive { "crosshair" } else { "default" })
                                role="img"
                                aria-label="Line chart"
                                aria-describedby={summary_id.clone()}
                            />
                            {if interactive {
                                view! {
                                    <div
                                        node_ref=plot_ref
                                        style="position: absolute; inset: 0; cursor: crosshair"
                                        tabindex="0"
                                        on:mousemove=move |ev| on_mousemove.run(ev)
                                        on:mouseleave=move |ev| on_mouseleave.run(ev)
                                        on:keydown=move |ev| on_keydown.run(ev)
                                    />
                                }.into_any()
                            } else {
                                ().into_any()
                            }}
                            {move || {
                                if !interactive {
                                    return ().into_any();
                                }
                                let Some(g) = prepare_geom(x_labels.get(), series.get(), y_min, y_max) else {
                                    return ().into_any();
                                };
                                let Some(idx) = hover_index.get() else {
                                    return ().into_any();
                                };
                                if idx >= g.labels.len() {
                                    return ().into_any();
                                }
                                let prices = std::sync::Arc::clone(&pricing);
                                let values = bucket_tooltip_rows_with_pricing(
                                    &g.all_series,
                                    idx,
                                    prices.as_ref(),
                                );
                                if values.is_empty() {
                                    return ().into_any();
                                }
                                let label = g.labels[idx].clone();
                                let (band_left, band_w) = line_band_style(idx, g.n);
                                let pos_style = line_tooltip_position_style(idx, g.n);
                                let primary_y = g
                                    .all_series
                                    .first()
                                    .and_then(|s| s.values.get(idx).and_then(|opt| *opt))
                                    .filter(|v| v.is_finite() && *v > 0.0);
                                let top_pct = primary_y.map(|v| value_top_pct(v, g.ymin, g.ymax));

                                view! {
                                    <>
                                        <div
                                            class="chart-hover-band"
                                            style=format!("left: {:.2}%; width: {:.2}%", band_left, band_w)
                                        ></div>
                                        {top_pct.map(|pct| view! {
                                            <div
                                                class="chart-hover-dot"
                                                style=format!(
                                                    "left: {:.2}%; top: {:.2}%",
                                                    line_center_pct(idx, g.n), pct
                                                )
                                            ></div>
                                        })}
                                        <div
                                            class="chart-tooltip"
                                            style=format!("position: absolute; {}", pos_style)
                                        >
                                            <div class="chart-tooltip-label">{label}</div>
                                            {values.iter().map(|(name, fmt_val, color, _)| {
                                                let name = name.clone();
                                                let fmt_val = fmt_val.clone();
                                                let color = color.clone();
                                                view! {
                                                    <div class="chart-tooltip-row">
                                                        <span class="chart-tooltip-row-name">
                                                            <span
                                                                class="chart-tooltip-dot"
                                                                style=format!("background: {}", color)
                                                            ></span>
                                                            {name}
                                                        </span>
                                                        <span class="chart-tooltip-value">{fmt_val}</span>
                                                    </div>
                                                }
                                            }).collect_view()}
                                        </div>
                                    </>
                                }.into_any()
                            }}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
