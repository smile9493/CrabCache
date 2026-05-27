use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use super::line_chart::{format_tooltip_value, mouse_to_svg_x};

static HISTOGRAM_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
struct Bin {
    range_start: f64,
    range_end: f64,
    count: usize,
}

fn auto_bins(values: &[f64], bin_count: usize) -> Vec<Bin> {
    if values.is_empty() || bin_count == 0 {
        return Vec::new();
    }
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    for &v in values {
        if v.is_finite() {
            min = min.min(v);
            max = max.max(v);
        }
    }
    if max <= min {
        max = min + 1.0;
    }
    let span = max - min;
    let bin_width = span / bin_count as f64;
    let mut bins: Vec<Bin> = (0..bin_count)
        .map(|i| Bin {
            range_start: min + i as f64 * bin_width,
            range_end: min + (i + 1) as f64 * bin_width,
            count: 0,
        })
        .collect();
    for &v in values {
        if !v.is_finite() {
            continue;
        }
        let idx = ((v - min) / bin_width).floor() as usize;
        let idx = idx.min(bin_count - 1);
        bins[idx].count += 1;
    }
    bins
}

#[component]
pub fn HistogramChart(
    values: Signal<Vec<f64>>,
    #[prop(default = 10)] bin_count: usize,
    #[prop(default = 200)] height_px: u32,
    y_unit: &'static str,
    empty_message: &'static str,
) -> impl IntoView {
    let chart_id = HISTOGRAM_ID.fetch_add(1, Ordering::Relaxed);
    let summary_id = format!("histogram-summary-{}", chart_id);
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();

    let bins_sig = Signal::derive(move || auto_bins(&values.get(), bin_count));

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(svg_el) = svg_ref.get() else { return };
        let svg_dom: web_sys::SvgsvgElement = svg_el.dyn_into().unwrap();
        let Some(svg_x) = mouse_to_svg_x(&ev, &svg_dom) else {
            hover_index.set(None);
            return;
        };
        let n = bins_sig.get_untracked().len();
        if n == 0 {
            hover_index.set(None);
            return;
        }
        let bar_w = 100.0 / n as f64;
        let idx = (svg_x / bar_w).floor() as usize;
        if idx < n {
            hover_index.set(Some(idx));
        } else {
            hover_index.set(None);
        }
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let bins = bins_sig.get_untracked();
        let n = bins.len();
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
        let bins = bins_sig.get_untracked();
        if !bins.is_empty() && hover_index.get_untracked().is_none() {
            hover_index.set(Some(0));
        }
    };

    let on_blur = move |_: web_sys::FocusEvent| {
        hover_index.set(None);
    };

    let summary_id_clone = summary_id.clone();

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let bins = bins_sig.get();
                if bins.is_empty() || bins.iter().all(|b| b.count == 0) {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any();
                }
                let max_count = bins.iter().map(|b| b.count).max().unwrap_or(1) as f64;
                let n = bins.len();
                let bar_w = 100.0 / n as f64;
                let pad = bar_w * 0.1;
                let hover_idx = hover_index.get();

                let total_requests: usize = bins.iter().map(|b| b.count).sum();
                let data_summary = format!(
                    "Histogram with {} bins and {} total requests. Y range: 0 to {} {}.",
                    n, total_requests, format_tooltip_value(max_count), y_unit
                );

                let tooltip_data = hover_idx.and_then(|idx| {
                    let bin = bins.get(idx)?;
                    let label = format!("{} \u{2013} {}",
                        format_tooltip_value(bin.range_start),
                        format_tooltip_value(bin.range_end));
                    Some((idx, label, bin.count))
                });

                // Y value for horizontal crosshair (max count at hover)
                let crosshair_y = hover_idx.and_then(|idx| {
                    let bin = bins.get(idx)?;
                    if bin.count > 0 {
                        let norm = (bin.count as f64 / max_count).clamp(0.0, 1.0);
                        Some(40.0 - norm * 38.0)
                    } else {
                        None
                    }
                });

                view! {
                    <div class="line-chart-plot" style="position: relative">
                        <div id={summary_id_clone.clone()} class="sr-only">{data_summary}</div>
                        <svg
                            node_ref=svg_ref
                            class="line-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px", height_px)
                            role="img"
                            aria-label="Histogram"
                            aria-describedby={summary_id_clone.clone()}
                            tabindex="0"
                            on:mousemove=on_mousemove
                            on:mouseleave=on_mouseleave
                            on:keydown=on_keydown
                            on:focus=on_focus
                            on:blur=on_blur
                        >
                            // Grid
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            // Horizontal grid lines
                            {move || {
                                let steps = 4;
                                (1..steps).map(|i| {
                                    let y = 40.0 - (38.0 * i as f64 / steps as f64);
                                    view! {
                                        <line
                                            x1="0" y1=y x2="100" y2=y
                                            stroke="var(--cc-border-light)"
                                            stroke-width="0.15"
                                            stroke-dasharray="1 2"
                                            vector-effect="non-scaling-stroke"
                                            opacity="0.4"
                                        />
                                    }
                                }).collect_view()
                            }}
                            // Bars
                            {bins.iter().enumerate().map(|(i, bin)| {
                                let h = if max_count > 0.0 { (bin.count as f64 / max_count) * 38.0 } else { 0.0 };
                                let x = i as f64 * bar_w + pad;
                                let w = (bar_w - 2.0 * pad).max(0.5);
                                let y = 40.0 - h;
                                let is_hover = hover_idx == Some(i);
                                let color = if is_hover { "var(--accent-primary)" } else { "var(--cc-border-light)" };
                                view! {
                                    <rect
                                        x=format!("{:.2}", x)
                                        y=format!("{:.2}", y)
                                        width=format!("{:.2}", w)
                                        height=format!("{:.2}", h)
                                        fill=color
                                        rx="0.35"
                                    />
                                }
                            }).collect_view()}

                            // === Financial Crosshair ===
                            // Hover column highlight
                            {hover_idx.map(|idx| {
                                let x = idx as f64 * bar_w;
                                view! {
                                    <rect
                                        x=x y="0"
                                        width=bar_w height="40"
                                        fill="var(--cc-text-muted)"
                                        opacity="0.04"
                                    />
                                }
                            })}
                            // Vertical crosshair
                            {hover_idx.map(|idx| {
                                let cx = idx as f64 * bar_w + bar_w / 2.0;
                                view! {
                                    <line
                                        x1=cx y1="0" x2=cx y2="40"
                                        stroke="var(--cc-text-muted)"
                                        stroke-width="0.25"
                                        stroke-dasharray="1.5 1"
                                        vector-effect="non-scaling-stroke"
                                        opacity="0.7"
                                    />
                                }
                            })}
                            // Horizontal crosshair
                            {crosshair_y.map(|cy| view! {
                                <line
                                    x1="0" y1=cy x2="100" y2=cy
                                    stroke="var(--cc-text-muted)"
                                    stroke-width="0.2"
                                    stroke-dasharray="1 1.5"
                                    vector-effect="non-scaling-stroke"
                                    opacity="0.5"
                                />
                            })}

                            <rect x="0" y="0" width="100" height="40" fill="transparent" style="cursor: crosshair" />
                        </svg>

                        // Y-axis value label
                        {crosshair_y.map(|cy| {
                            let val_pct = ((40.0 - cy) / 38.0).clamp(0.0, 1.0);
                            let count_val = val_pct * max_count;
                            let top_pct = (cy / 40.0) * 100.0;
                            view! {
                                <div
                                    class="chart-axis-label"
                                    style=format!("top: {:.1}%", top_pct)
                                >
                                    {format_tooltip_value(count_val)}
                                </div>
                            }
                        })}

                        // Financial tooltip
                        {tooltip_data.map(|(idx, label, count)| {
                            let pct = (idx as f64 + 0.5) / n as f64 * 100.0;
                            let side = if pct > 70.0 { "right" } else { "left" };
                            let pos_style = if side == "right" {
                                format!("right: {:.1}%", 100.0 - pct)
                            } else {
                                format!("left: {:.1}%", pct)
                            };
                            view! {
                                <div
                                    class="chart-tooltip"
                                    style=format!("position: absolute; top: 8px; {}", pos_style)
                                >
                                    <div class="chart-tooltip-label">{label}</div>
                                    <div class="chart-tooltip-row">
                                        <span class="chart-tooltip-row-name">
                                            <span class="chart-tooltip-dot" style="background: var(--accent-primary)"></span>
                                            <span>{"Requests"}</span>
                                        </span>
                                                <span class="chart-tooltip-value">{format!("{}", count)}</span>
                                    </div>
                                </div>
                            }
                        })}

                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">
                            {format!("0 \u{2013} {} {}", format_tooltip_value(max_count), y_unit)}
                        </div>
                    </div>
                    <div class="line-chart-x-labels">
                        {bins.iter().enumerate().filter_map(|(i, bin)| {
                            if n <= 8 || i == 0 || i == n - 1 || i % (n / 6).max(1) == 0 {
                                Some(view! {
                                    <span class="line-chart-x-tick">{format_tooltip_value(bin.range_start)}</span>
                                })
                            } else {
                                None
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </div>
    }
}
