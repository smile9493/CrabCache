//! Request lifecycle waterfall (Canvas rendering).

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

pub use crate::components::chart::core::WaterfallStage;
use crate::components::chart::canvas_render;
use crate::theme::use_theme_signal;

static WATERFALL_CHART_ID: AtomicUsize = AtomicUsize::new(0);

#[component]
pub fn WaterfallChart(stages: Vec<WaterfallStage>) -> impl IntoView {
    let chart_id = WATERFALL_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("waterfall-chart-summary-{}", chart_id);
    let theme = use_theme_signal();
    // Avoid StoredValue: it can panic if accessed after scope disposal.
    let stages = std::sync::Arc::new(stages);
    let canvas_ref: NodeRef<leptos::html::Canvas> = NodeRef::new();

    let stage_descriptions: Vec<String> = stages
        .iter()
        .map(|s| format!("{}: {:.1}ms", s.label, s.duration_ms))
        .collect();
    let data_summary = format!(
        "Waterfall chart with {} stages: {}.",
        stages.len(),
        stage_descriptions.join(", ")
    );

    Effect::new({
        let stages = std::sync::Arc::clone(&stages);
        move |_| {
        let Some(canvas_el) = canvas_ref.get() else {
            return;
        };
        let canvas_dom: web_sys::HtmlCanvasElement = canvas_el.dyn_into().unwrap();
        let _ = theme.get();
        canvas_render::render_waterfall(&canvas_dom, stages.as_ref(), theme.get());
        }
    });

    view! {
        <div class="waterfall-chart">
            <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
            {move || {
                if stages.is_empty() {
                    view! {
                        <div class="text-center py-6 text-theme-muted text-sm">"No stage data"</div>
                    }.into_any()
                } else {
                    view! {
                        <canvas
                            node_ref=canvas_ref
                            style="width: 100%; min-height: 120px"
                            role="img"
                            aria-label="Waterfall chart"
                            aria-describedby={summary_id.clone()}
                        />
                    }.into_any()
                }
            }}
        </div>
    }
}
