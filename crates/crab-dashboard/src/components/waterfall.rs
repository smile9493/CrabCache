//! Request lifecycle waterfall (Plotters horizontal bars).

use leptos::prelude::*;

pub use crate::components::chart::core::WaterfallStage;
use crate::components::chart::svg_render;
use crate::theme::use_theme_signal;

#[component]
pub fn WaterfallChart(stages: Vec<WaterfallStage>) -> impl IntoView {
    let theme = use_theme_signal();
    let stored = StoredValue::new(stages);

    let svg = Signal::derive(move || {
        let _ = theme.get();
        svg_render::render_waterfall(&stored.get_value(), theme.get())
    });

    view! {
        <div class="waterfall-chart">
            {move || {
                if let Some(doc) = svg.get() {
                    view! {
                        <div class="waterfall-chart-svg-host" prop:inner_html=doc />
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
