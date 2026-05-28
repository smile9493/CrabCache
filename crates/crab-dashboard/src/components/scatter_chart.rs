//! Scatter plot (Canvas + hover tooltip) — Financial Grade.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

pub use crate::components::chart::core::ScatterPoint;
use crate::components::chart::core::scatter_range;
use crate::components::chart::canvas_render;
use crate::theme::use_theme_signal;

static SCATTER_CHART_ID: AtomicUsize = AtomicUsize::new(0);

fn format_precise_value(v: f64) -> String {
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

#[component]
pub fn ScatterChart(
    points: Signal<Vec<ScatterPoint>>,
    x_label: String,
    y_label: String,
    #[prop(default = 220)] height_px: u32,
    empty_message: &'static str,
    #[prop(default = None)] fit_line: Option<(f64, f64)>,
) -> impl IntoView {
    let chart_id = SCATTER_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("scatter-chart-summary-{}", chart_id);
    let theme = use_theme_signal();
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let mouse_pos: RwSignal<Option<(f64, f64)>> = RwSignal::new(None);
    let plot_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let canvas_ref: NodeRef<leptos::html::Canvas> = NodeRef::new();
    // Avoid StoredValue: it can panic if accessed after scope disposal.
    // Labels are immutable per component instance.
    let x_label = std::sync::Arc::new(x_label);
    let y_label = std::sync::Arc::new(y_label);

    // Draw canvas whenever data or theme changes.
    Effect::new({
        let x_label = std::sync::Arc::clone(&x_label);
        let y_label = std::sync::Arc::clone(&y_label);
        move |_| {
        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas_dom: web_sys::HtmlCanvasElement = canvas_el.dyn_into().unwrap();
        let _ = theme.get();
        let pts = points.get();
        let x_l = x_label.as_ref();
        let y_l = y_label.as_ref();
        canvas_render::render_scatter(&canvas_dom, &pts, theme.get(), x_l, y_l, fit_line);
        }
    });

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(host) = plot_ref.get() else { return };
        let rect = host.get_bounding_client_rect();
        let mx = ev.client_x() as f64 - rect.left();
        let my = ev.client_y() as f64 - rect.top();
        mouse_pos.set(Some((mx, my)));

        let Some(canvas_el) = canvas_ref.get() else {
            hover_index.set(None);
            return;
        };
        let canvas_dom: web_sys::HtmlCanvasElement = canvas_el.dyn_into().unwrap();
        let c_rect = canvas_dom.get_bounding_client_rect();
        let width = c_rect.width();
        let height = c_rect.height();
        if width <= 0.0 || height <= 0.0 {
            hover_index.set(None);
            return;
        }
        let rel_x = ((ev.client_x() as f64 - c_rect.left()) / width).clamp(0.0, 1.0);
        let rel_y = ((ev.client_y() as f64 - c_rect.top()) / height).clamp(0.0, 1.0);
        let pts = points.get_untracked();
        if pts.is_empty() {
            hover_index.set(None);
            return;
        }
        let (xmin, xmax, ymin, ymax) = scatter_range(&pts);
        let plot_x = xmin + rel_x * (xmax - xmin);
        let plot_y = ymax - rel_y * (ymax - ymin);

        let mut closest = 0usize;
        let mut min_dist = f64::MAX;
        for (i, p) in pts.iter().enumerate() {
            if !(p.x.is_finite() && p.y.is_finite()) {
                continue;
            }
            let dx = (p.x - plot_x) / (xmax - xmin).max(1.0);
            let dy = (p.y - plot_y) / (ymax - ymin).max(1.0);
            let dist = dx * dx + dy * dy;
            if dist < min_dist {
                min_dist = dist;
                closest = i;
            }
        }
        if min_dist < 0.08 {
            hover_index.set(Some(closest));
        } else {
            hover_index.set(None);
        }
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
        mouse_pos.set(None);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let pts = points.get_untracked();
        let n = pts.len();
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
    };

    let on_focus = move |_: web_sys::FocusEvent| {
        let pts = points.get_untracked();
        if !pts.is_empty() && hover_index.get_untracked().is_none() {
            hover_index.set(Some(0));
        }
    };

    let on_blur = move |_: web_sys::FocusEvent| {
        hover_index.set(None);
        mouse_pos.set(None);
    };

    let summary_id_clone = summary_id.clone();

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 24)>
            {move || {
                let x_label = std::sync::Arc::clone(&x_label);
                let y_label = std::sync::Arc::clone(&y_label);
                let pts = points.get();
                let x_l = x_label.as_ref();
                let y_l = y_label.as_ref();
                let data_summary = if pts.is_empty() {
                    "Empty scatter chart.".to_string()
                } else {
                    let (xmin, xmax, ymin, ymax) = scatter_range(&pts);
                    format!(
                        "Scatter chart with {} data points. {} range: {:.1} to {:.1}. {} range: {:.1} to {:.1}.",
                        pts.len(), x_l, xmin, xmax, y_l, ymin, ymax
                    )
                };

                if pts.is_empty() {
                    view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any()
                } else {
                    view! {
                        <div class="line-chart-plot" style="position: relative; width: 100%">
                            <div id={summary_id_clone.clone()} class="sr-only">{data_summary}</div>
                            <canvas
                                node_ref=canvas_ref
                                style=format!("width: 100%; height: {}px; cursor: crosshair", height_px)
                                role="img"
                                aria-label="Scatter chart"
                                aria-describedby={summary_id_clone.clone()}
                            />
                            <div
                                node_ref=plot_ref
                                style="position: absolute; inset: 0; cursor: crosshair"
                                tabindex="0"
                                on:mousemove=on_mousemove
                                on:mouseleave=on_mouseleave
                                on:keydown=on_keydown
                                on:focus=on_focus
                                on:blur=on_blur
                            />
                            {move || {
                                let idx = hover_index.get();
                                let pos = mouse_pos.get();
                                let pts = points.get();
                                idx.and_then(|i| pts.get(i)).and_then(|p| {
                                    let pos = pos?;
                                    let x_l = x_label.as_ref();
                                    let y_l = y_label.as_ref();
                                    let container_width = plot_ref.get()
                                        .map(|el| el.get_bounding_client_rect().width())
                                        .unwrap_or(400.0);
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
                                    Some(view! {
                                        <div
                                            class="chart-tooltip"
                                            style=format!("position: absolute; {}", pos_style)
                                        >
                                            <div class="chart-tooltip-label">{p.label.clone()}</div>
                                            <div class="chart-tooltip-row">
                                                <span class="chart-tooltip-row-name">
                                                    <span>{"X"}</span>
                                                </span>
                                                <span class="chart-tooltip-value">
                                                    {format!("{}: {}", x_l, format_precise_value(p.x))}
                                                </span>
                                            </div>
                                            <div class="chart-tooltip-row">
                                                <span class="chart-tooltip-row-name">
                                                    <span>{"Y"}</span>
                                                </span>
                                                <span class="chart-tooltip-value">
                                                    {format!("{}: {}", y_l, format_precise_value(p.y))}
                                                </span>
                                            </div>
                                        </div>
                                    })
                                })
                            }}
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}
