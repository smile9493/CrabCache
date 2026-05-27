//! Request lifecycle waterfall (Plotters horizontal bars).

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub use crate::components::chart::core::WaterfallStage;
use crate::components::chart::svg_render;
use crate::theme::use_theme_signal;

static WATERFALL_CHART_ID: AtomicUsize = AtomicUsize::new(0);

#[component]
pub fn WaterfallChart(stages: Vec<WaterfallStage>) -> impl IntoView {
    let chart_id = WATERFALL_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("waterfall-chart-summary-{}", chart_id);
    let theme = use_theme_signal();
    let stored = StoredValue::new(stages.clone());

    // Generate data summary for screen readers
    let stage_descriptions: Vec<String> = stages
        .iter()
        .map(|s| format!("{}: {:.1}ms", s.label, s.duration_ms))
        .collect();
    let data_summary = format!(
        "Waterfall chart with {} stages: {}.",
        stages.len(),
        stage_descriptions.join(", ")
    );

    let svg = Signal::derive(move || {
        let _ = theme.get();
        svg_render::render_waterfall(&stored.get_value(), theme.get())
    });

    view! {
        <div class="waterfall-chart">
            <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
            {move || {
                if let Some(doc) = svg.get() {
                    view! {
                        <div
                            class="waterfall-chart-svg-host"
                            prop:inner_html=doc
                            role="img"
                            aria-label="Waterfall chart"
                            aria-describedby={summary_id.clone()}
                        />
                    }.into_any()
                } else {
                    view! {
                        <div class="text-center py-6 text-theme-muted text-sm">"No stage data"</div>
                    }.into_any()
                }
            }}
        </div>
    }
}
