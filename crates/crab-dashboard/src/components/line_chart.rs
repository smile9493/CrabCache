use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

pub use super::chart::core::ChartSeries;
pub use super::chart::core::ThresholdLine;
pub(super) use super::chart::core::{format_tooltip_value, mouse_to_svg_x, value_segments_indexed, y_range};
use super::chart::core::downsample_series;
use crate::components::chart::interaction::{
    bucket_tooltip_rows, line_band_style, line_center_pct, line_tooltip_position_style,
    value_top_pct,
};

static LINE_CHART_ID: AtomicUsize = AtomicUsize::new(0);

/// Maximum data points before downsampling kicks in.
const MAX_CHART_POINTS: usize = 200;

fn scale_segment(
    values: &[f64],
    ymin: f64,
    ymax: f64,
    height: f64,
    x_offset: f64,
    x_step: f64,
) -> String {
    if values.is_empty() {
        return String::new();
    }
    let span = (ymax - ymin).max(1.0);
    let n = values.len();
    let step = if n > 1 { x_step } else { 0.0 };
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = x_offset + i as f64 * step;
            let norm = ((v - ymin) / span).clamp(0.0, 1.0);
            let y = height - norm * height;
            format!("{x:.2},{y:.2}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format a numeric value with full precision for the financial tooltip.
fn format_precise_value(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.2}", v)
    } else if v >= 1.0 {
        format!("{:.2}", v)
    } else if v >= 0.01 {
        format!("{:.4}", v)
    } else {
        format!("{:.6}", v)
    }
}

#[component]
pub fn LineChart(
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = "ms")] y_unit: &'static str,
    empty_message: &'static str,
    #[prop(default = Vec::new())] thresholds: Vec<ThresholdLine>,
    #[prop(default = true)] interactive: bool,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let hover_pending: RwSignal<Option<usize>> = RwSignal::new(None);
    let hover_raf_scheduled = RwSignal::new(false);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();
    let thresholds_stored = StoredValue::new(thresholds);

    let chart_geom = Memo::new(move |_| {
        let labels = x_labels.get();
        let raw_series = series.get();
        let all_series: Vec<ChartSeries> = if labels.len() > MAX_CHART_POINTS {
            raw_series
                .into_iter()
                .map(|s| ChartSeries {
                    label: s.label,
                    color: s.color,
                    values: downsample_series(&s.values, MAX_CHART_POINTS),
                    dashed: s.dashed,
                    fill: s.fill,
                })
                .collect()
        } else {
            raw_series
        };
        if labels.is_empty() || all_series.is_empty() {
            return None;
        }
        let has_point = all_series.iter().any(|s| {
            s.values.iter().any(|v| matches!(v, Some(x) if *x > 0.0))
        });
        if !has_point {
            return None;
        }
        let (ymin, ymax) = y_range(&all_series);
        const W: f64 = 100.0;
        let n = labels.len().max(1);
        let x_step = if n > 1 { W / (n - 1) as f64 } else { 0.0 };
        let span = (ymax - ymin).max(1.0);
        Some(LineChartGeom {
            labels,
            all_series,
            ymin,
            ymax,
            span,
            n,
            x_step,
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
        let n = geom.n;
        if n == 0 {
            queue_hover(None);
            return;
        }
        const W: f64 = 100.0;
        let idx = if n == 1 {
            0
        } else {
            let step = W / (n - 1) as f64;
            ((svg_x / step).round() as usize).min(n - 1)
        };
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
            "ArrowLeft" => {
                match current {
                    Some(idx) if idx > 0 => Some(idx - 1),
                    None => Some(n - 1),
                    _ => current,
                }
            }
            "ArrowRight" => {
                match current {
                    Some(idx) if idx < n - 1 => Some(idx + 1),
                    None => Some(0),
                    _ => current,
                }
            }
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
                let chart_id = LINE_CHART_ID.fetch_add(1, Ordering::Relaxed);
                let summary_id = format!("line-chart-summary-{}", chart_id);
                let Some(geom) = chart_geom.get() else {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }
                    .into_any();
                };
                let LineChartGeom {
                    labels,
                    all_series,
                    ymin,
                    ymax,
                    span,
                    n: _,
                    x_step,
                } = geom;
                const H: f64 = 40.0;
                let thresholds = thresholds_stored.get_value();
                let series_names: Vec<String> = all_series.iter().map(|s| s.label.clone()).collect();
                let data_summary = format!(
                    "Line chart with {} data points and {} series: {}. Y range: {:.0} to {:.0} {}.",
                    labels.len(),
                    series_names.len(),
                    series_names.join(", "),
                    ymin,
                    ymax,
                    y_unit
                );
                let y_hint = format!("{:.0}\u{2013}{:.0} {}", ymin, ymax, y_unit);
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
                            aria-label="Line chart"
                            aria-describedby=summary_id
                        >
                            // Grid axes
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            // Horizontal grid lines (subtle Y guides)
                            <line x1="0" y1="10" x2="100" y2="10" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="20" x2="100" y2="20" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="30" x2="100" y2="30" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            // Area fills
                            {all_series.iter().filter(|s| s.fill).flat_map(|s| {
                                let segs = value_segments_indexed(&s.values);
                                segs.into_iter().map(move |(start_idx, seg_vals)| {
                                    let x_offset = start_idx as f64 * x_step;
                                    let line_points = scale_segment(&seg_vals, ymin, ymax, H, x_offset, x_step);
                                    let first_x = x_offset;
                                    let last_x = x_offset + (seg_vals.len().saturating_sub(1)) as f64 * x_step;
                                    let poly = format!("{} {:.2},{:.2} {:.2},{:.2}", line_points, last_x, H, first_x, H);
                                    view! {
                                        <polygon points=poly fill=s.color opacity="0.12" />
                                    }
                                })
                            }).collect_view()}
                            // Data lines
                            {all_series.iter().flat_map(|s| {
                                let segs = value_segments_indexed(&s.values);
                                segs.into_iter().map(move |(start_idx, seg_vals)| {
                                    let x_offset = start_idx as f64 * x_step;
                                    let points = scale_segment(&seg_vals, ymin, ymax, H, x_offset, x_step);
                                    let dash = if s.dashed { "4 3" } else { "none" };
                                    view! {
                                        <polyline
                                            points=points
                                            fill="none"
                                            stroke=s.color
                                            stroke-width="1.5"
                                            vector-effect="non-scaling-stroke"
                                            stroke-dasharray=dash
                                        />
                                    }
                                })
                            }).collect_view()}
                            // Threshold reference lines
                            {thresholds.iter().filter_map(|t| {
                                let norm = ((t.value - ymin) / span).clamp(0.0, 1.0);
                                let y = H - norm * H;
                                if !(0.0..=H).contains(&y) { return None; }
                                let color = t.color;
                                let label = t.label.clone();
                                Some(view! {
                                    <>
                                        <line
                                            x1="0" y1=y x2="100" y2=y
                                            stroke=color stroke-width="0.4"
                                            stroke-dasharray="3 2"
                                            vector-effect="non-scaling-stroke"
                                            opacity="0.7"
                                        />
                                        <text
                                            x="1" y={format!("{:.1}", y - 0.8)}
                                            fill=color font-size="2.2"
                                            font-family="var(--font-mono)"
                                        >{label}</text>
                                    </>
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
                            let values = bucket_tooltip_rows(&geom.all_series, idx);
                            if values.is_empty() {
                                return ().into_any();
                            }
                            let label = geom.labels[idx].clone();
                            let (band_left, band_w) = line_band_style(idx, geom.n);
                            let center_pct = line_center_pct(idx, geom.n);
                            let pos_style = line_tooltip_position_style(idx, geom.n);
                            let primary_y = geom
                                .all_series
                                .first()
                                .and_then(|s| s.values.get(idx).and_then(|opt| *opt))
                                .filter(|v| v.is_finite() && *v > 0.0);
                            let top_pct = primary_y.map(|v| value_top_pct(v, geom.ymin, geom.ymax));

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
                                        let val = primary_y.unwrap_or(0.0);
                                        view! {
                                            <>
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
                                            </>
                                        }
                                    })}
                                    {geom.all_series.iter().filter_map(|s| {
                                        let v = s.values.get(idx).and_then(|opt| *opt)?;
                                        if !v.is_finite() {
                                            return None;
                                        }
                                        let top = value_top_pct(v, geom.ymin, geom.ymax);
                                        let left = line_center_pct(idx, geom.n);
                                        let color = s.color;
                                        Some(view! {
                                            <span
                                                class="chart-hover-dot"
                                                style=format!(
                                                    "left: {:.2}%; top: {:.2}%; background: {}",
                                                    left, top, color
                                                )
                                            ></span>
                                        })
                                    }).collect_view()}
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
                            if tick_count <= 8 || i == 0 || i == tick_count - 1 || i % (tick_count / 6).max(1) == 0 {
                                Some(view! { <span class="line-chart-x-tick">{l}</span> })
                            } else {
                                None
                            }
                        }).collect_view()
                        }}
                    </div>
                    <div class="line-chart-legend">
                        {all_series.into_iter().map(|s| {
                            let dash = if s.dashed { " (miss)" } else { "" };
                            view! {
                                <span class="line-chart-legend-item">
                                    <span
                                        class="line-chart-legend-swatch"
                                        style=format!("background: {}", s.color)
                                    ></span>
                                    {format!("{}{}", s.label, dash)}
                                </span>
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[derive(Clone, PartialEq)]
struct LineChartGeom {
    labels: Vec<String>,
    all_series: Vec<ChartSeries>,
    ymin: f64,
    ymax: f64,
    span: f64,
    n: usize,
    x_step: f64,
}

#[component]
pub fn TokenLineChart(
    x_labels: Signal<Vec<String>>,
    input_values: Signal<Vec<Option<f64>>>,
    output_values: Signal<Vec<Option<f64>>>,
    input_label: String,
    output_label: String,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = true)] interactive: bool,
    empty_message: &'static str,
) -> impl IntoView {
    let series = Signal::derive(move || {
        vec![
            ChartSeries {
                label: input_label.clone(),
                color: "var(--accent-primary)",
                values: input_values.get(),
                dashed: false,
                fill: false,
            },
            ChartSeries {
                label: output_label.clone(),
                color: "var(--info)",
                values: output_values.get(),
                dashed: false,
                fill: false,
            },
        ]
    });
    view! {
        <LineChart
            x_labels=x_labels
            series=series
            height_px=height_px
            y_unit="tokens"
            interactive=interactive
            empty_message=empty_message
        />
    }
}
