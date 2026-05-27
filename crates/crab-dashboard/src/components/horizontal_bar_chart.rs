//! Horizontal categorical bar chart (Plotters SVG).

use leptos::prelude::*;

use crate::components::chart::svg_render;
use crate::theme::use_theme_signal;

#[component]
pub fn HorizontalBarChart(
    labels: Signal<Vec<String>>,
    values: Signal<Vec<f64>>,
    #[prop(default = 520)] width: u32,
    #[prop(default = 180)] height_px: u32,
    empty_message: &'static str,
) -> impl IntoView {
    let theme = use_theme_signal();

    let svg = Signal::derive(move || {
        let _ = theme.get();
        svg_render::render_horizontal_bar_chart(
            &labels.get(),
            &values.get(),
            theme.get(),
            width,
            height_px,
        )
    });

    view! {
        <div class="horizontal-bar-chart">
            {move || {
                if let Some(doc) = svg.get() {
                    view! {
                        <div class="waterfall-chart-svg-host" prop:inner_html=doc />
                    }.into_any()
                } else {
                    view! {
                        <div class="text-center py-6 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any()
                }
            }}
        </div>
    }
}
