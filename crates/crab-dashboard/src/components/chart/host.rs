//! Leptos wrapper: Plotters SVG output + hover overlay and tooltips.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::core::ChartSeries;
use super::interaction::series_tooltip_at;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChartHoverMode {
    /// One bucket per categorical label (bar charts).
    Bucket,
    /// Nearest X index on a line chart (0..n-1).
    Line,
}

#[component]
pub fn PlottersChartFrame(
    #[prop(into)] svg: Signal<Option<String>>,
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    hover_mode: ChartHoverMode,
    #[prop(default = 260)] height_px: u32,
    empty_message: &'static str,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    let point_count = Signal::derive(move || x_labels.get().len());

    let tooltip_rows = Signal::derive(move || {
        let _ = hover_index.get();
        series_tooltip_at(&x_labels.get(), &series.get(), hover_index.get())
    });

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(host) = svg_ref.get() else {
            return;
        };
        let Some(svg_el) = host.query_selector("svg").ok().flatten() else {
            hover_index.set(None);
            return;
        };
        let Ok(html) = svg_el.dyn_into::<web_sys::HtmlElement>() else {
            hover_index.set(None);
            return;
        };
        let rect = html.get_bounding_client_rect();
        let width = rect.width();
        if width <= 0.0 {
            hover_index.set(None);
            return;
        }
        let rel_x = ((ev.client_x() as f64 - rect.left()) / width).clamp(0.0, 1.0);
        let n = point_count.get_untracked();
        if n == 0 {
            hover_index.set(None);
            return;
        }
        let idx = match hover_mode {
            ChartHoverMode::Bucket => (rel_x * n as f64).floor() as usize,
            ChartHoverMode::Line => {
                if n == 1 {
                    0
                } else {
                    (rel_x * (n - 1) as f64).round() as usize
                }
            }
        };
        let idx = idx.min(n.saturating_sub(1));
        hover_index.set(Some(idx));
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
    };

    view! {
        <div class="line-chart-wrap plotters-chart-wrap" style=format!("min-height: {}px", height_px + 24)>
            {move || {
                if let Some(svg_doc) = svg.get() {
                    view! {
                        <div style="position: relative; width: 100%">
                            <div
                                node_ref=svg_ref
                                class="chart-plotters-svg"
                                style="width: 100%; cursor: crosshair"
                                prop:inner_html=svg_doc
                                on:mousemove=on_mousemove
                                on:mouseleave=on_mouseleave
                            />
                            {tooltip_rows.get().map(|(label, values)| {
                                let pct = hover_index.get().map(|idx| {
                                    let n = point_count.get().max(1);
                                    if n <= 1 {
                                        50.0
                                    } else {
                                        idx as f64 / (n - 1) as f64 * 100.0
                                    }
                                }).unwrap_or(0.0);
                                let side = if pct > 75.0 { "right" } else { "left" };
                                let pos_style = if side == "right" {
                                    format!("right: {:.1}%", 100.0 - pct)
                                } else {
                                    format!("left: {:.1}%", pct)
                                };
                                view! {
                                    <div
                                        class="chart-tooltip"
                                        style=format!(
                                            "position: absolute; top: 8px; {}; pointer-events: none; z-index: 10",
                                            pos_style
                                        )
                                    >
                                        <div class="chart-tooltip-label" style="font-size: 0.6875rem; color: var(--cc-text-muted); margin-bottom: 0.25rem; font-family: var(--font-mono)">
                                            {label}
                                        </div>
                                        {values.into_iter().map(|(name, val, color)| {
                                            view! {
                                                <div class="chart-tooltip-row" style="display: flex; align-items: center; gap: 0.35rem; font-size: 0.75rem; line-height: 1.4">
                                                    <span style=format!("width: 0.5rem; height: 0.5rem; border-radius: 50%; background: {}; flex-shrink: 0", color)></span>
                                                    <span style="color: var(--cc-text-muted)">{name}:</span>
                                                    <span style="font-family: var(--font-mono); font-weight: 600; color: var(--cc-text)">{val}</span>
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                }
                            })}
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

