//! Grouped vertical bar chart (time series / usage trends) — Financial Grade.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use super::line_chart::{ChartSeries, format_tooltip_value, mouse_to_svg_x, y_range};

static BAR_CHART_ID: AtomicUsize = AtomicUsize::new(0);

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
pub fn BarChart(
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = "")] y_unit: &'static str,
    empty_message: &'static str,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(svg_el) = svg_ref.get() else { return };
        let svg_dom: web_sys::SvgsvgElement = svg_el.dyn_into().unwrap();
        let Some(svg_x) = mouse_to_svg_x(&ev, &svg_dom) else {
            hover_index.set(None);
            return;
        };
        let labels = x_labels.get_untracked();
        let n = labels.len();
        if n == 0 {
            hover_index.set(None);
            return;
        }
        const W: f64 = 100.0;
        let bucket_w = W / n as f64;
        let idx = ((svg_x / bucket_w).floor() as usize).min(n.saturating_sub(1));
        hover_index.set(Some(idx));
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let labels = x_labels.get_untracked();
        let n = labels.len();
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
        let labels = x_labels.get_untracked();
        if !labels.is_empty() && hover_index.get_untracked().is_none() {
            hover_index.set(Some(0));
        }
    };

    let on_blur = move |_: web_sys::FocusEvent| {
        hover_index.set(None);
    };

    view! {
        <div class="line-chart-wrap bar-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let chart_id = BAR_CHART_ID.fetch_add(1, Ordering::Relaxed);
                let summary_id = format!("bar-chart-summary-{}", chart_id);
                let labels = x_labels.get();
                let all_series = series.get();
                if labels.is_empty() || all_series.is_empty() {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any();
                }
                let has_point = all_series.iter().any(|s| {
                    s.values.iter().any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite()))
                });
                if !has_point {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any();
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

                let series_names: Vec<String> = all_series.iter().map(|s| s.label.clone()).collect();
                let data_summary = format!(
                    "Bar chart with {} categories and {} series: {}. Y range: {:.0} to {:.0}{}.",
                    labels.len(),
                    series_names.len(),
                    series_names.join(", "),
                    ymin, ymax,
                    if y_unit.is_empty() { String::new() } else { format!(" {}", y_unit) }
                );

                let hover_idx = hover_index.get();

                // Tooltip data with precise values
                let tooltip_data = hover_idx.and_then(|idx| {
                    if idx >= labels.len() { return None; }
                    let label = labels[idx].clone();
                    let values: Vec<(String, String, &'static str, f64)> = all_series
                        .iter()
                        .filter_map(|s| {
                            let v = s.values.get(idx).and_then(|opt| *opt)?;
                            Some((s.label.clone(), format_precise_value(v), s.color, v))
                        })
                        .collect();
                    if values.is_empty() { None } else { Some((idx, label, values)) }
                });

                let crosshair_x = hover_idx.map(|idx| idx as f64 * bucket_w + bucket_w / 2.0);

                // Max value in hovered column for horizontal crosshair
                let crosshair_y = hover_idx.and_then(|idx| {
                    let max_val = all_series.iter().filter_map(|s| {
                        let v = s.values.get(idx).and_then(|opt| *opt)?;
                        if v.is_finite() && v > 0.0 { Some(v) } else { None }
                    }).fold(0.0_f64, f64::max);
                    if max_val > 0.0 {
                        let norm = ((max_val - ymin) / span).clamp(0.0, 1.0);
                        Some(H - norm * H)
                    } else {
                        None
                    }
                });

                let crosshair_y_value = hover_idx.and_then(|idx| {
                    let max_val = all_series.iter().filter_map(|s| {
                        let v = s.values.get(idx).and_then(|opt| *opt)?;
                        if v.is_finite() && v > 0.0 { Some(v) } else { None }
                    }).fold(0.0_f64, f64::max);
                    if max_val > 0.0 { Some(max_val) } else { None }
                });

                view! {
                    <div class="line-chart-plot" style="position: relative">
                        <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
                        <svg
                            node_ref=svg_ref
                            class="line-chart-svg bar-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px", height_px)
                            role="img"
                            aria-label="Bar chart"
                            aria-describedby={summary_id}
                            tabindex="0"
                            on:mousemove=on_mousemove
                            on:mouseleave=on_mouseleave
                            on:keydown=on_keydown
                            on:focus=on_focus
                            on:blur=on_blur
                        >
                            // Grid axes
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            // Horizontal grid lines
                            {move || {
                                let steps = 4;
                                (1..steps).map(|i| {
                                    let y = H * i as f64 / steps as f64;
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
                            {all_series.iter().enumerate().flat_map(|(j, s)| {
                                s.values.iter().enumerate().filter_map(move |(i, opt)| {
                                    let v = match opt {
                                        Some(x) if x.is_finite() && *x > 0.0 => *x,
                                        _ => return None,
                                    };
                                    let group_x = i as f64 * bucket_w;
                                    let x = group_x + inner_pad + j as f64 * slot + (slot - bar_w) / 2.0;
                                    let norm = ((v - ymin) / span).clamp(0.0, 1.0);
                                    let h = norm * H;
                                    let y = H - h;
                                    let color = s.color;
                                    Some(view! {
                                        <rect
                                            x=x y=y
                                            width=bar_w height=h.max(0.15)
                                            fill=color opacity="0.88"
                                            rx="0.35"
                                        />
                                    })
                                })
                            }).collect_view()}

                            // === Financial Crosshair ===
                            // Hover column highlight band
                            {crosshair_x.map(|cx| {
                                let half = bucket_w / 2.0;
                                view! {
                                    <rect
                                        x={cx - half} y="0"
                                        width={bucket_w} height="40"
                                        fill="var(--cc-text-muted)"
                                        opacity="0.04"
                                    />
                                }
                            })}
                            // Vertical crosshair
                            {crosshair_x.map(|cx| view! {
                                <line
                                    x1=cx y1="0" x2=cx y2="40"
                                    stroke="var(--cc-text-muted)"
                                    stroke-width="0.25"
                                    stroke-dasharray="1.5 1"
                                    vector-effect="non-scaling-stroke"
                                    opacity="0.7"
                                />
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

                            // Invisible hit area
                            <rect
                                x="0" y="0" width="100" height="40"
                                fill="transparent" style="cursor: crosshair"
                            />
                        </svg>

                        // Y-axis value label
                        {crosshair_y_value.map(|val| {
                            let pct = ((val - ymin) / span).clamp(0.0, 1.0);
                            let top_pct = (1.0 - pct) * 100.0;
                            view! {
                                <div
                                    class="chart-axis-label"
                                    style=format!("top: {:.1}%", top_pct)
                                >
                                    {format_tooltip_value(val)}
                                </div>
                            }
                        })}

                        // Financial tooltip
                        {tooltip_data.map(|(idx, label, values)| {
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
                            }
                        })}

                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">
                            {if y_unit.is_empty() {
                                format!("{:.0}\u{2013}{:.0}", ymin, ymax)
                            } else {
                                format!("{:.0}\u{2013}{:.0} {}", ymin, ymax, y_unit)
                            }}
                        </div>
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
                }.into_any()
            }}
        </div>
    }
}
