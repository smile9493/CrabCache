// P3-2: Request waterfall chart — horizontal bars showing stage durations.

use leptos::prelude::*;

#[derive(Clone)]
pub struct WaterfallStage {
    pub label: String,
    pub duration_ms: f64,
}

/// Horizontal waterfall chart showing request lifecycle stages.
/// Color-coded: < 10ms green, 10-100ms amber, > 100ms red.
#[component]
pub fn WaterfallChart(stages: Vec<WaterfallStage>) -> impl IntoView {
    if stages.is_empty() {
        return view! { <div class="text-center py-6 text-theme-muted text-sm">"No stage data"</div> }.into_any();
    }

    let max_ms = stages
        .iter()
        .map(|s| s.duration_ms)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    view! {
        <div class="waterfall-chart" style="display: flex; flex-direction: column; gap: 0.5rem">
            {stages.into_iter().map(|stage| {
                let pct = (stage.duration_ms / max_ms * 100.0).min(100.0).max(0.5);
                let color = if stage.duration_ms < 10.0 {
                    "var(--cc-success)"
                } else if stage.duration_ms < 100.0 {
                    "var(--cc-warning)"
                } else {
                    "var(--cc-error)"
                };
                let label = stage.label.clone();
                let ms_str = if stage.duration_ms >= 1000.0 {
                    format!("{:.2}s", stage.duration_ms / 1000.0)
                } else {
                    format!("{:.1}ms", stage.duration_ms)
                };
                view! {
                    <div class="waterfall-row" style="display: flex; align-items: center; gap: 0.75rem">
                        <span style="min-width: 120px; font-size: 0.75rem; color: var(--cc-text-muted); text-align: right; font-family: var(--font-mono)">{label}</span>
                        <div style="flex: 1; height: 1.25rem; background: var(--cc-bg-elevated); border-radius: var(--radius-sm); overflow: hidden; position: relative">
                            <div style=format!("width: {:.1}%; height: 100%; background: {}; border-radius: var(--radius-sm); transition: width 0.3s ease", pct, color)></div>
                        </div>
                        <span style="min-width: 60px; font-size: 0.75rem; font-family: var(--font-mono); color: var(--cc-text)">{ms_str}</span>
                    </div>
                }
            }).collect_view()}
        </div>
    }.into_any()
}
