//! Canvas-based bar chart component — drop-in replacement for `BarChart`.
//!
//! Renders on HTML5 Canvas via Plotters `CanvasBackend` for better performance.
//! Interaction (hover, tooltip) uses positioned HTML overlays.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use crate::components::chart::canvas_renderer::CanvasLineRenderer;
use crate::components::chart::core::ChartSeries;
use crate::components::chart::interaction::bucket_tooltip_rows;
use crate::components::chart::renderer::{BarDrawRequest, ChartRenderer};
use crate::theme::use_theme_signal;

static CANVAS_BAR_CHART_ID: AtomicUsize = AtomicUsize::new(0);

#[component]
pub fn CanvasBarChart(
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = "")] y_unit: &'static str,
    #[prop(default = true)] interactive: bool,
    empty_message: &'static str,
) -> impl IntoView {
    let chart_id = CANVAS_BAR_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("canvas-bar-chart-summary-{}", chart_id);
    let theme = use_theme_signal();
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let mouse_pos: RwSignal<Option<(f64, f64)>> = RwSignal::new(None);
    let canvas_ref: NodeRef<leptos::html::Canvas> = NodeRef::new();
    let plot_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    // Draw canvas whenever data or theme changes.
    Effect::new(move |_| {
        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas_dom: web_sys::HtmlCanvasElement = match canvas_el.dyn_into() {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = theme.get();
        let labels = x_labels.get();
        let raw_series = series.get();
        let has_point = raw_series.iter().any(|s| {
            s.values
                .iter()
                .any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite()))
        });
        if labels.is_empty() || raw_series.is_empty() || !has_point {
            return;
        }
        let req = BarDrawRequest {
            labels,
            series: raw_series,
            y_unit,
            width_px: 640,
            height_px,
        };
        let mut renderer = CanvasLineRenderer::new(theme.get());
        let _ = renderer.render_bar(&canvas_dom, &req);
    });

    let on_mousemove = move |ev: web_sys::MouseEvent| {
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
        let n = labels.len();
        if n == 0 {
            hover_index.set(None);
            return;
        }
        let idx = (rel_x * n as f64).floor() as usize;
        let idx = idx.min(n.saturating_sub(1));
        let raw_series = series.get_untracked();
        let values = bucket_tooltip_rows(&raw_series, idx);
        if values.is_empty() {
            hover_index.set(None);
        } else {
            hover_index.set(Some(idx));
        }
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
        mouse_pos.set(None);
    };

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 24)>
            {move || {
                let labels = x_labels.get();
                let raw_series = series.get();
                let has_point = raw_series
                    .iter()
                    .any(|s| s.values.iter().any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite())));

                let data_summary = if labels.is_empty() || raw_series.is_empty() {
                    "Empty bar chart.".to_string()
                } else {
                    format!(
                        "Bar chart with {} series and {} categories.",
                        raw_series.len(),
                        labels.len()
                    )
                };

                if labels.is_empty() || raw_series.is_empty() || !has_point {
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
                                aria-label="Bar chart"
                                aria-describedby={summary_id.clone()}
                            />
                            {if interactive {
                                view! {
                                    <div
                                        node_ref=plot_ref
                                        style="position: absolute; inset: 0; cursor: crosshair"
                                        on:mousemove=on_mousemove
                                        on:mouseleave=on_mouseleave
                                    />
                                }.into_any()
                            } else {
                                ().into_any()
                            }}
                            {move || {
                                if !interactive {
                                    return ().into_any();
                                }
                                let Some(idx) = hover_index.get() else {
                                    return ().into_any();
                                };
                                let labels = x_labels.get();
                                let raw_series = series.get();
                                if idx >= labels.len() {
                                    return ().into_any();
                                }
                                let values = bucket_tooltip_rows(&raw_series, idx);
                                if values.is_empty() {
                                    return ().into_any();
                                }
                                let label = labels[idx].clone();
                                let n = labels.len();
                                let band_left = (idx as f64 / n as f64) * 100.0;
                                let band_w = (1.0 / n as f64) * 100.0;
                                let container_width = plot_ref.get()
                                    .map(|el| el.get_bounding_client_rect().width())
                                    .unwrap_or(400.0);
                                let pos = mouse_pos.get().unwrap_or((0.0, 0.0));
                                let tooltip_left = pos.0 + 16.0;
                                let flip = tooltip_left > container_width - 180.0;
                                let pos_style = if flip {
                                    format!("right: {:.0}px; top: {:.0}px",
                                        container_width - pos.0 + 12.0,
                                        pos.1.max(8.0))
                                } else {
                                    format!("left: {:.0}px; top: {:.0}px",
                                        tooltip_left, pos.1.max(8.0))
                                };

                                view! {
                                    <>
                                        <div
                                            class="chart-hover-band"
                                            style=format!("left: {:.2}%; width: {:.2}%", band_left, band_w)
                                        ></div>
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
