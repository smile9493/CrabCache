//! Grouped vertical bar chart (time series / usage trends).

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use super::line_chart::{ChartSeries, format_tooltip_value, mouse_to_svg_x, y_range};
use crate::components::chart::interaction::{
    bucket_center_pct, bucket_tooltip_rows, bucket_width_pct, column_max_value,
    tooltip_position_style, value_top_pct,
};

static BAR_CHART_ID: AtomicUsize = AtomicUsize::new(0);

#[component]
pub fn BarChart(
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = "")] y_unit: &'static str,
    #[prop(default = true)] interactive: bool,
    empty_message: &'static str,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();
    let plot_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    let chart_geom = Memo::new(move |_| {
        let labels = x_labels.get();
        let all_series = series.get();
        if labels.is_empty() || all_series.is_empty() {
            return None;
        }
        let has_point = all_series.iter().any(|s| {
            s.values
                .iter()
                .any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite()))
        });
        if !has_point {
            return None;
        }
        let (ymin, ymax) = y_range(&all_series);
        const W: f64 = 100.0;
        const H: f64 = 40.0;
        let n = labels.len().max(1);
        let bucket_w = W / n as f64;
        let series_count = all_series.len().max(1);
        let inner_pad = bucket_w * 0.12;
        let usable = (bucket_w - inner_pad * 2.0).max(0.5);
        let slot = usable / series_count as f64;
        let bar_w = slot * 0.82;
        let span = (ymax - ymin).max(1.0);
        Some(BarChartGeom {
            labels,
            all_series,
            ymin,
            ymax,
            span,
            n,
            bucket_w,
            inner_pad,
            slot,
            bar_w,
            h: H,
        })
    });

    let queue_hover = move |idx: Option<usize>| {
        hover_index.set(idx);
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
        const W: f64 = 100.0;
        let bucket_w = W / geom.n as f64;
        let idx = ((svg_x / bucket_w).floor() as usize).min(geom.n.saturating_sub(1));
        queue_hover(Some(idx));
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
        <div class="line-chart-wrap bar-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let chart_id = BAR_CHART_ID.fetch_add(1, Ordering::Relaxed);
                let summary_id = format!("bar-chart-summary-{}", chart_id);
                let Some(geom) = chart_geom.get() else {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }
                    .into_any();
                };
                let BarChartGeom {
                    labels,
                    all_series,
                    ymin,
                    ymax,
                    span,
                    n: _,
                    bucket_w,
                    inner_pad,
                    slot,
                    bar_w,
                    h,
                } = geom;
                let series_names: Vec<String> = all_series.iter().map(|s| s.label.clone()).collect();
                let data_summary = format!(
                    "Bar chart with {} categories and {} series: {}. Y range: {:.0} to {:.0}{}.",
                    labels.len(),
                    series_names.len(),
                    series_names.join(", "),
                    ymin,
                    ymax,
                    if y_unit.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", y_unit)
                    }
                );
                let y_hint = if y_unit.is_empty() {
                    format!("{:.0}\u{2013}{:.0}", ymin, ymax)
                } else {
                    format!("{:.0}\u{2013}{:.0} {}", ymin, ymax, y_unit)
                };
                let tab_idx = if interactive { "0" } else { "-1" };

                view! {
                    <div
                        class="line-chart-plot"
                        node_ref=plot_ref
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
                            class="line-chart-svg bar-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px; cursor: {}", height_px, if interactive { "crosshair" } else { "default" })
                            role="img"
                            aria-label="Bar chart"
                            aria-describedby=summary_id
                        >
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="10" x2="100" y2="10" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="20" x2="100" y2="20" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="30" x2="100" y2="30" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            {all_series.iter().enumerate().flat_map(|(j, s)| {
                                s.values.iter().enumerate().filter_map(move |(i, opt)| {
                                    let v = match opt {
                                        Some(x) if x.is_finite() && *x > 0.0 => *x,
                                        _ => return None,
                                    };
                                    let group_x = i as f64 * bucket_w;
                                    let x = group_x + inner_pad + j as f64 * slot + (slot - bar_w) / 2.0;
                                    let norm = ((v - ymin) / span).clamp(0.0, 1.0);
                                    let bar_h = norm * h;
                                    let y = h - bar_h;
                                    let color = s.color.clone();
                                    Some(view! {
                                        <rect
                                            x=x y=y
                                            width=bar_w height=bar_h.max(0.15)
                                            fill=color opacity="0.88"
                                            rx="0.35"
                                        />
                                    })
                                })
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
                            if idx >= geom.labels.len() {
                                return ().into_any();
                            }
                            let n = geom.n;
                            let band_w = bucket_width_pct(n);
                            let band_left = idx as f64 / n as f64 * 100.0;
                            let center_pct = bucket_center_pct(idx, n);
                            let values = bucket_tooltip_rows(&geom.all_series, idx);
                            if values.is_empty() {
                                return ().into_any();
                            }
                            let label = geom.labels[idx].clone();
                            let pos_style = tooltip_position_style(idx, n);
                            let y_val = column_max_value(&geom.all_series, idx);
                            let top_pct =
                                y_val.map(|v| value_top_pct(v, geom.ymin, geom.ymax));

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
                                        let val = y_val.unwrap_or(0.0);
                                        view! {
                                            <div
                                                class="chart-crosshair-h"
                                                style=format!("top: {:.1}%", top)
                                            ></div>
                                            <div
                                                class="chart-axis-label"
                                                style=format!("top: {:.1}%", top)
                                            >
                                                {format_tooltip_value(val)}
                                            </div>
                                        }
                                    })}
                                    <div
                                        class="chart-tooltip"
                                        style=format!("position: absolute; top: 8px; {}", pos_style)
                                    >
                                        <div class="chart-tooltip-label">{label}</div>
                                        {values.into_iter().map(|(name, val, color, _raw)| {
                                            view! {
                                                <div class="chart-tooltip-row">
                                                    <span class="chart-tooltip-row-name">
                                                        <span
                                                            class="chart-tooltip-dot"
                                                            style=format!("background: {}", color)
                                                        ></span>
                                                        <span>{name}</span>
                                                    </span>
                                                    <span class="chart-tooltip-value">{val}</span>
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                </>
                            }
                            .into_any()
                        }}

                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">{y_hint}</div>
                    </div>
                    <div class="line-chart-x-labels">
                        {{
                            let tick_count = labels.len();
                            labels.into_iter().enumerate().filter_map(move |(i, l)| {
                                if tick_count <= 8
                                    || i == 0
                                    || i == tick_count - 1
                                    || i % (tick_count / 6).max(1) == 0
                                {
                                    Some(view! { <span class="line-chart-x-tick">{l}</span> })
                                } else {
                                    None
                                }
                            }).collect_view()
                        }}
                    </div>
                    <div class="line-chart-legend">
                        {all_series.into_iter().map(|s| {
                            view! {
                                <span class="line-chart-legend-item">
                                    <span
                                        class="line-chart-legend-swatch"
                                        style=format!("background: {}", s.color)
                                    ></span>
                                    {s.label.clone()}
                                </span>
                            }
                        }).collect_view()}
                    </div>
                }
                .into_any()
            }}
        </div>
    }
}

#[derive(Clone, PartialEq)]
struct BarChartGeom {
    labels: Vec<String>,
    all_series: Vec<ChartSeries>,
    ymin: f64,
    ymax: f64,
    span: f64,
    n: usize,
    bucket_w: f64,
    inner_pad: f64,
    slot: f64,
    bar_w: f64,
    h: f64,
}
