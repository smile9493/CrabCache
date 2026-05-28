use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use crate::components::chart::interaction::{
    bucket_center_pct, bucket_width_pct, tooltip_position_style, value_top_pct,
};
use super::line_chart::{format_tooltip_value, mouse_to_svg_x};

static HISTOGRAM_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, PartialEq)]
struct Bin {
    range_start: f64,
    range_end: f64,
    count: usize,
}

#[derive(Clone, PartialEq)]
struct HistogramGeom {
    bins: Vec<Bin>,
    max_count: f64,
    n: usize,
    bar_w: f64,
    pad: f64,
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
    #[prop(default = true)] interactive: bool,
    y_unit: &'static str,
    empty_message: &'static str,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let hover_pending: RwSignal<Option<usize>> = RwSignal::new(None);
    let hover_raf_scheduled = RwSignal::new(false);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();

    let bins_sig = Signal::derive(move || auto_bins(&values.get(), bin_count));

    let chart_geom = Memo::new(move |_| {
        let bins = bins_sig.get();
        if bins.is_empty() || bins.iter().all(|b| b.count == 0) {
            return None;
        }
        let max_count = bins.iter().map(|b| b.count).max().unwrap_or(1) as f64;
        let n = bins.len();
        let bar_w = 100.0 / n as f64;
        let pad = bar_w * 0.1;
        Some(HistogramGeom {
            bins,
            max_count,
            n,
            bar_w,
            pad,
        })
    });

    let queue_hover = move |idx: Option<usize>| {
        hover_pending.set(idx);
        if hover_raf_scheduled.get_untracked() {
            return;
        }
        hover_raf_scheduled.set(true);
        let next = wasm_bindgen::closure::Closure::once(move || {
            hover_raf_scheduled.set(false);
            hover_index.set(hover_pending.get_untracked());
        });
        let next_js = next.into_js_value();
        let _ = web_sys::window()
            .unwrap()
            .request_animation_frame(next_js.unchecked_ref());
    };

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        if !interactive {
            return;
        }
        let Some(svg_el) = svg_ref.get() else {
            return;
        };
        let svg_dom: web_sys::SvgsvgElement = svg_el.dyn_into().unwrap();
        let Some(svg_x) = mouse_to_svg_x(&ev, &svg_dom) else {
            queue_hover(None);
            return;
        };
        let Some(geom) = chart_geom.get() else {
            queue_hover(None);
            return;
        };
        let idx = (svg_x / geom.bar_w).floor() as usize;
        if idx < geom.n {
            queue_hover(Some(idx));
        } else {
            queue_hover(None);
        }
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        queue_hover(None);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if !interactive {
            return;
        }
        let Some(geom) = chart_geom.get() else {
            return;
        };
        let n = geom.n;
        if n == 0 {
            return;
        }
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
        if !interactive {
            return;
        }
        if chart_geom.get().is_some() && hover_index.get_untracked().is_none() {
            hover_index.set(Some(0));
        }
    };

    let on_blur = move |_: web_sys::FocusEvent| {
        queue_hover(None);
    };

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let chart_id = HISTOGRAM_ID.fetch_add(1, Ordering::Relaxed);
                let summary_id = format!("histogram-summary-{}", chart_id);
                let Some(geom) = chart_geom.get() else {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }
                    .into_any();
                };
                let HistogramGeom {
                    bins,
                    max_count,
                    n,
                    bar_w,
                    pad,
                } = geom;
                let total_requests: usize = bins.iter().map(|b| b.count).sum();
                let data_summary = format!(
                    "Histogram with {} bins and {} total requests. Y range: 0 to {} {}.",
                    n,
                    total_requests,
                    format_tooltip_value(max_count),
                    y_unit
                );
                let y_hint = format!("0 \u{2013} {} {}", format_tooltip_value(max_count), y_unit);
                let tab_idx = if interactive { "0" } else { "-1" };

                view! {
                    <div
                        class="line-chart-plot"
                        style="position: relative"
                        on:mousemove=on_mousemove
                        on:mouseleave=on_mouseleave
                        on:keydown=on_keydown
                        on:focus=on_focus
                        on:blur=on_blur
                        tabindex=tab_idx
                        role="group"
                    >
                        <div id=summary_id.clone() class="sr-only">{data_summary}</div>
                        <svg
                            node_ref=svg_ref
                            class="line-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px; cursor: {}", height_px, if interactive { "crosshair" } else { "default" })
                            role="img"
                            aria-label="Histogram"
                            aria-describedby=summary_id
                        >
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="10.5" x2="100" y2="10.5" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="21" x2="100" y2="21" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="31.5" x2="100" y2="31.5" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            {bins.iter().enumerate().map(|(i, bin)| {
                                let h = if max_count > 0.0 {
                                    (bin.count as f64 / max_count) * 38.0
                                } else {
                                    0.0
                                };
                                let x = i as f64 * bar_w + pad;
                                let w = (bar_w - 2.0 * pad).max(0.5);
                                let y = 40.0 - h;
                                view! {
                                    <rect
                                        x=format!("{:.2}", x)
                                        y=format!("{:.2}", y)
                                        width=format!("{:.2}", w)
                                        height=format!("{:.2}", h)
                                        fill="var(--cc-border-light)"
                                        rx="0.35"
                                    />
                                }
                            }).collect_view()}
                            <rect x="0" y="0" width="100" height="40" fill="transparent" />
                        </svg>

                        {move || {
                            if !interactive {
                                return ().into_any();
                            }
                            let Some(geom) = chart_geom.get() else {
                                return ().into_any();
                            };
                            let Some(idx) = hover_index.get() else {
                                return ().into_any();
                            };
                            let Some(bin) = geom.bins.get(idx) else {
                                return ().into_any();
                            };
                            let n = geom.n;
                            let band_w = bucket_width_pct(n);
                            let band_left = idx as f64 / n as f64 * 100.0;
                            let center_pct = bucket_center_pct(idx, n);
                            let label = format!(
                                "{} \u{2013} {}",
                                format_tooltip_value(bin.range_start),
                                format_tooltip_value(bin.range_end)
                            );
                            let pos_style = tooltip_position_style(idx, n);
                            let top_pct = if bin.count > 0 && geom.max_count > 0.0 {
                                Some(value_top_pct(
                                    bin.count as f64,
                                    0.0,
                                    geom.max_count,
                                ))
                            } else {
                                None
                            };

                            view! {
                                <>
                                    <div
                                        class="chart-hover-band"
                                        style=format!("left: {:.2}%; width: {:.2}%", band_left, band_w)
                                    ></div>
                                    <div
                                        class="chart-crosshair-v"
                                        style=format!("left: {:.2}%", center_pct)
                                    ></div>
                                    {top_pct.map(|top| {
                                        view! {
                                            <div
                                                class="chart-crosshair-h"
                                                style=format!("top: {:.1}%", top)
                                            ></div>
                                            <div
                                                class="chart-axis-label"
                                                style=format!("top: {:.1}%", top)
                                            >
                                                {format_tooltip_value(bin.count as f64)}
                                            </div>
                                        }
                                    })}
                                    <div
                                        class="chart-tooltip"
                                        style=format!("position: absolute; top: 8px; {}", pos_style)
                                    >
                                        <div class="chart-tooltip-label">{label}</div>
                                        <div class="chart-tooltip-row">
                                            <span class="chart-tooltip-row-name">
                                                <span
                                                    class="chart-tooltip-dot"
                                                    style="background: var(--accent-primary)"
                                                ></span>
                                                <span>{"Requests"}</span>
                                            </span>
                                            <span class="chart-tooltip-value">{format!("{}", bin.count)}</span>
                                        </div>
                                    </div>
                                </>
                            }
                            .into_any()
                        }}

                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">{y_hint}</div>
                    </div>
                    <div class="line-chart-x-labels">
                        {bins.into_iter().enumerate().filter_map(|(i, bin)| {
                            if n <= 8 || i == 0 || i == n - 1 || i % (n / 6).max(1) == 0 {
                                Some(view! {
                                    <span class="line-chart-x-tick">{format_tooltip_value(bin.range_start)}</span>
                                })
                            } else {
                                None
                            }
                        }).collect_view()}
                    </div>
                }
                .into_any()
            }}
        </div>
    }
}
