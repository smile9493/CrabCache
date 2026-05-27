// P2-2: Sparkline component — minimal inline trend line.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static SPARKLINE_ID: AtomicUsize = AtomicUsize::new(0);

/// Minimal sparkline SVG — no axes, no labels, just a trend line.
#[component]
pub fn Sparkline(
    values: Vec<f64>,
    #[prop(default = "var(--cc-accent)")] color: &'static str,
    #[prop(default = 64)] width: u32,
    #[prop(default = 24)] height: u32,
) -> impl IntoView {
    let chart_id = SPARKLINE_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("sparkline-summary-{}", chart_id);

    let valid: Vec<f64> = values
        .into_iter()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    if valid.len() < 2 {
        return view! { <div style=format!("width: {}px; height: {}px", width, height)></div> }
            .into_any();
    }

    let min = valid.iter().cloned().fold(f64::MAX, f64::min);
    let max = valid.iter().cloned().fold(f64::MIN, f64::max);
    let span = (max - min).max(1.0);
    let w = width as f64;
    let h = height as f64;
    let n = valid.len();
    let step = if n > 1 { w / (n - 1) as f64 } else { 0.0 };

    // Generate data summary for screen readers
    let data_summary = format!(
        "Sparkline trend with {} data points. Range: {:.1} to {:.1}.",
        n, min, max
    );

    let points: String = valid
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = i as f64 * step;
            let norm = (v - min) / span;
            let y = h - norm * h * 0.85 - h * 0.075; // 7.5% padding top/bottom
            format!("{:.1},{:.1}", x, y)
        })
        .collect::<Vec<_>>()
        .join(" ");

    let viewbox = format!("0 0 {} {}", width, height);
    let width_str = width.to_string();
    let height_str = height.to_string();

    view! {
        <>
            <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
            <svg
                class="sparkline-svg"
                width=width_str
                height=height_str
                viewBox=viewbox
                preserveAspectRatio="none"
                style="display: block"
                role="img"
                aria-label="Sparkline trend"
                aria-describedby={summary_id}
            >
                <polyline
                    points=points
                    fill="none"
                    stroke=color
                    stroke-width="1.5"
                    vector-effect="non-scaling-stroke"
                    stroke-linejoin="round"
                    stroke-linecap="round"
                />
            </svg>
        </>
    }
    .into_any()
}
