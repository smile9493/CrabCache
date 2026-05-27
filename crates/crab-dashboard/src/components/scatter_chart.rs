//! Scatter plot (Plotters SVG + hover tooltip) — Financial Grade.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

pub use crate::components::chart::core::ScatterPoint;
use crate::components::chart::core::{scatter_range};
use crate::components::chart::svg_render;
use crate::theme::use_theme_signal;

static SCATTER_CHART_ID: AtomicUsize = AtomicUsize::new(0);

/// Format a numeric value with full precision for the financial tooltip.
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
    // Track mouse position relative to container for tooltip positioning
    let mouse_pos: RwSignal<Option<(f64, f64)>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let x_label_stored = StoredValue::new(x_label);
    let y_label_stored = StoredValue::new(y_label);

    let svg = Signal::derive(move || {
        let _ = theme.get();
        svg_render::render_scatter(
            &points.get(),
            theme.get(),
            &x_label_stored.get_value(),
            &y_label_stored.get_value(),
            fit_line,
        )
    });

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(host) = svg_ref.get() else { return };

        // Track mouse position relative to the container for tooltip placement
        let rect = host.get_bounding_client_rect();
        let mx = ev.client_x() as f64 - rect.left();
        let my = ev.client_y() as f64 - rect.top();
        mouse_pos.set(Some((mx, my)));

        let Some(svg_el) = host.query_selector("svg").ok().flatten() else {
            hover_index.set(None);
            return;
        };
        let Ok(html) = svg_el.dyn_into::<web_sys::HtmlElement>() else {
            hover_index.set(None);
            return;
        };
        let svg_rect = html.get_bounding_client_rect();
        let width = svg_rect.width();
        let height = svg_rect.height();
        if width <= 0.0 || height <= 0.0 {
            hover_index.set(None);
            return;
        }
        let rel_x = ((ev.client_x() as f64 - svg_rect.left()) / width).clamp(0.0, 1.0);
        let rel_y = ((ev.client_y() as f64 - svg_rect.top()) / height).clamp(0.0, 1.0);
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
            if !(p.x.is_finite() && p.y.is_finite()) { continue; }
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
        if n == 0 { return; }
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
        <div class="line-chart-wrap plotters-chart-wrap" style=format!("min-height: {}px", height_px + 24)>
            {move || {
                let pts = points.get();
                let x_l = x_label_stored.get_value();
                let y_l = y_label_stored.get_value();
                let data_summary = if pts.is_empty() {
                    "Empty scatter chart.".to_string()
                } else {
                    let (xmin, xmax, ymin, ymax) = scatter_range(&pts);
                    format!(
                        "Scatter chart with {} data points. {} range: {:.1} to {:.1}. {} range: {:.1} to {:.1}.",
                        pts.len(), x_l, xmin, xmax, y_l, ymin, ymax
                    )
                };

                if let Some(doc) = svg.get() {
                    view! {
                        <div class="line-chart-plot" style="position: relative; width: 100%">
                            <div id={summary_id_clone.clone()} class="sr-only">{data_summary}</div>
                            <div
                                node_ref=svg_ref
                                class="chart-plotters-svg"
                                style=format!("width: 100%; cursor: crosshair; min-height: {}px", height_px)
                                prop:inner_html=doc
                                role="img"
                                aria-label="Scatter chart"
                                aria-describedby={summary_id_clone.clone()}
                                tabindex="0"
                                on:mousemove=on_mousemove
                                on:mouseleave=on_mouseleave
                                on:keydown=on_keydown
                                on:focus=on_focus
                                on:blur=on_blur
                            />
                            // Financial tooltip positioned at mouse cursor
                            {move || {
                                let idx = hover_index.get();
                                let pos = mouse_pos.get();
                                let pts = points.get();
                                idx.and_then(|i| pts.get(i)).and_then(|p| {
                                    let pos = pos?;
                                    let x_l = x_label_stored.get_value();
                                    let y_l = y_label_stored.get_value();
                                    // Position tooltip: flip if near right edge
                                    let container_width = svg_ref.get()
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
                } else {
                    view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any()
                }
            }}
        </div>
    }
}
