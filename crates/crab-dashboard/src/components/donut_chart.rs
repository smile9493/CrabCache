// Donut chart component for cache hit visualization.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

// Re-export so existing `crate::components::donut_chart::DonutSegment` paths still work.
pub use crate::components::chart::core::DonutSegment;

static DONUT_CHART_ID: AtomicUsize = AtomicUsize::new(0);

/// SVG donut chart rendered with stroke-dasharray technique.
#[component]
pub fn DonutChart(
    segments: Vec<DonutSegment>,
    #[prop(default = String::new())] center_label: String,
    #[prop(default = 160)] size: u32,
) -> impl IntoView {
    let chart_id = DONUT_CHART_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("donut-chart-summary-{}", chart_id);

    if segments.is_empty() {
        return view! { <div class="text-center py-6 text-theme-muted text-sm">"No data"</div> }
            .into_any();
    }

    let total: f64 = segments.iter().map(|s| s.value).sum();
    if total <= 0.0 {
        return view! { <div class="text-center py-6 text-theme-muted text-sm">"No data"</div> }
            .into_any();
    }

    let r = 40.0_f64;
    let circumference = 2.0 * std::f64::consts::PI * r;
    let stroke_width = 10.0;
    let center = size as f64 / 2.0;

    // Generate data summary for screen readers
    let segment_descriptions: Vec<String> = segments
        .iter()
        .map(|s| {
            let pct = if total > 0.0 {
                s.value / total * 100.0
            } else {
                0.0
            };
            format!("{}: {:.1}%", s.label, pct)
        })
        .collect();
    let data_summary = format!(
        "Donut chart with {} segments: {}.",
        segments.len(),
        segment_descriptions.join(", ")
    );

    // Build segments as offset+dash pairs
    let mut accumulated = 0.0_f64;
    let circle_views: Vec<_> = segments
        .iter()
        .map(|seg| {
            let fraction = seg.value / total;
            let dash_len = fraction * circumference;
            let gap_len = circumference - dash_len;
            let offset = -accumulated * circumference + circumference * 0.25; // start at top
            accumulated += fraction;
            let dasharray = format!("{:.2} {:.2}", dash_len, gap_len);
            let color = seg.color;
            view! {
                <circle
                    cx=center
                    cy=center
                    r=r
                    fill="none"
                    stroke=color
                    stroke-width=stroke_width
                    stroke-dasharray=dasharray
                    stroke-dashoffset=format!("{:.2}", offset)
                    stroke-linecap="butt"
                />
            }
        })
        .collect();

    let size_str = size.to_string();

    view! {
        <div class="donut-chart-container">
            <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
            <svg
                width=size_str.clone()
                height=size_str
                viewBox=format!("0 0 {} {}", size, size)
                class="donut-chart-svg"
                role="img"
                aria-label="Donut chart"
                aria-describedby={summary_id}
            >
                {circle_views}
                {if !center_label.is_empty() {
                    view! {
                        <text
                            x=format!("{}", center)
                            y=format!("{}", center)
                            text-anchor="middle"
                            dominant-baseline="central"
                            class="donut-chart-center-label"
                            fill="var(--cc-text)"
                            font-size="14"
                            font-weight="600"
                        >
                            {center_label}
                        </text>
                    }.into_any()
                } else {
                    ().into_any()
                }}
            </svg>
            <div class="donut-chart-legend">
                {segments.into_iter().map(|seg| {
                    let pct = if total > 0.0 { seg.value / total * 100.0 } else { 0.0 };
                    view! {
                        <span class="donut-legend-item">
                            <span class="donut-legend-dot" style=format!("background: {}", seg.color)></span>
                            {format!("{} ({:.1}%)", seg.label, pct)}
                        </span>
                    }
                }).collect_view()}
            </div>
        </div>
    }.into_any()
}
