//! Horizontal categorical bar chart (Canvas rendering).

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use crate::components::chart::canvas_render;
use crate::theme::use_theme_signal;

static HORIZONTAL_BAR_CHART_ID: AtomicUsize = AtomicUsize::new(0);

#[component]
pub fn HorizontalBarChart(
    labels: Signal<Vec<String>>,
    values: Signal<Vec<f64>>,
    #[prop(default = 520)] width: u32,
    #[prop(default = 180)] height_px: u32,
    empty_message: &'static str,
) -> impl IntoView {
    let chart_id = HORIZONTAL_BAR_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("horizontal-bar-chart-summary-{}", chart_id);
    let theme = use_theme_signal();
    let canvas_ref: NodeRef<leptos::html::Canvas> = NodeRef::new();

    Effect::new(move |_| {
        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas_dom: web_sys::HtmlCanvasElement = canvas_el.dyn_into().unwrap();
        let _ = theme.get();
        let lbls = labels.get();
        let vals = values.get();
        canvas_render::render_horizontal_bar_chart(
            &canvas_dom,
            &lbls,
            &vals,
            theme.get(),
            width,
            height_px,
        );
    });

    let summary_id_clone = summary_id.clone();

    view! {
        <div class="horizontal-bar-chart">
            {move || {
                let lbls = labels.get();
                let vals = values.get();
                let data_summary = if lbls.is_empty() || vals.is_empty() {
                    "Empty horizontal bar chart.".to_string()
                } else {
                    let max_val = vals.iter().copied().filter(|v| v.is_finite()).fold(0.0_f64, f64::max);
                    let entries: Vec<String> = lbls.iter().zip(vals.iter())
                        .filter(|(_, v)| **v > 0.0 && v.is_finite())
                        .map(|(l, v)| format!("{}: {:.1}", l, v))
                        .collect();
                    format!(
                        "Horizontal bar chart with {} categories. Max value: {:.1}. Entries: {}.",
                        lbls.len(),
                        max_val,
                        entries.join(", ")
                    )
                };

                if lbls.is_empty() || vals.is_empty() || !vals.iter().any(|v| *v > 0.0 && v.is_finite()) {
                    view! {
                        <div class="text-center py-6 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any()
                } else {
                    view! {
                        <>
                            <div id={summary_id_clone.clone()} class="sr-only">{data_summary}</div>
                            <canvas
                                node_ref=canvas_ref
                                style=format!("width: 100%; height: {}px", height_px)
                                role="img"
                                aria-label="Horizontal bar chart"
                                aria-describedby={summary_id_clone.clone()}
                            />
                        </>
                    }.into_any()
                }
            }}
        </div>
    }
}
