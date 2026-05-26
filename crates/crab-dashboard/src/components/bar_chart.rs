//! Grouped vertical bar chart (time series / usage trends).

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::line_chart::{ChartSeries, format_tooltip_value, mouse_to_svg_x, y_range};

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
        let Some(svg_el) = svg_ref.get() else {
            return;
        };
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

    view! {
        <div class="line-chart-wrap bar-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let labels = x_labels.get();
                let all_series = series.get();
                if labels.is_empty() || all_series.is_empty() {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }
                    .into_any();
                }
                let has_point = all_series.iter().any(|s| {
                    s.values
                        .iter()
                        .any(|v| matches!(v, Some(x) if *x > 0.0 && x.is_finite()))
                });
                if !has_point {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }
                    .into_any();
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

                let hover_idx = hover_index.get();
                let tooltip_data = hover_idx.and_then(|idx| {
                    if idx >= labels.len() {
                        return None;
                    }
                    let label = labels[idx].clone();
                    let values: Vec<(String, String, &'static str)> = all_series
                        .iter()
                        .filter_map(|s| {
                            let v = s.values.get(idx).and_then(|opt| *opt)?;
                            Some((s.label.clone(), format_tooltip_value(v), s.color))
                        })
                        .collect();
                    if values.is_empty() {
                        None
                    } else {
                        Some((idx, label, values))
                    }
                });

                let crosshair_x = hover_idx.map(|idx| idx as f64 * bucket_w + bucket_w / 2.0);

                view! {
                    <div class="line-chart-plot" style="position: relative">
                        <svg
                            node_ref=svg_ref
                            class="line-chart-svg bar-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px", height_px)
                            on:mousemove=on_mousemove
                            on:mouseleave=on_mouseleave
                        >
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
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
                                            x=x
                                            y=y
                                            width=bar_w
                                            height=h.max(0.15)
                                            fill=color
                                            opacity="0.88"
                                            rx="0.35"
                                        />
                                    })
                                })
                            }).collect_view()}
                            {crosshair_x.map(|cx| view! {
                                <line
                                    x1=cx
                                    y1="0"
                                    x2=cx
                                    y2="40"
                                    stroke="var(--cc-text-muted)"
                                    stroke-width="0.3"
                                    stroke-dasharray="1.5 1.5"
                                    vector-effect="non-scaling-stroke"
                                    style="opacity: 0.5"
                                />
                            })}
                            <rect x="0" y="0" width="100" height="40" fill="transparent" style="cursor: crosshair" />
                        </svg>
                        {tooltip_data.map(|(idx, label, values)| {
                            let pct = (idx as f64 + 0.5) / n as f64 * 100.0;
                            let side = if pct > 75.0 { "right" } else { "left" };
                            let pos_style = if side == "right" {
                                format!("right: {:.1}%", 100.0 - pct)
                            } else {
                                format!("left: {:.1}%", pct)
                            };
                            view! {
                                <div
                                    class="chart-tooltip"
                                    style=format!(
                                        "position: absolute; top: 8px; {}; pointer-events: none; z-index: 10",
                                        pos_style
                                    )
                                >
                                    <div
                                        class="chart-tooltip-label"
                                        style="font-size: 0.6875rem; color: var(--cc-text-muted); margin-bottom: 0.25rem; font-family: var(--font-mono)"
                                    >
                                        {label}
                                    </div>
                                    {values
                                        .into_iter()
                                        .map(|(name, val, color)| {
                                            view! {
                                                <div
                                                    class="chart-tooltip-row"
                                                    style="display: flex; align-items: center; gap: 0.35rem; font-size: 0.75rem; line-height: 1.4"
                                                >
                                                    <span style=format!(
                                                        "width: 0.5rem; height: 0.5rem; border-radius: 2px; background: {}; flex-shrink: 0",
                                                        color
                                                    )></span>
                                                    <span style="color: var(--cc-text-muted)">{name}:</span>
                                                    <span style="font-family: var(--font-mono); font-weight: 600; color: var(--cc-text)">
                                                        {val}
                                                    </span>
                                                </div>
                                            }
                                        })
                                        .collect_view()}
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
                            labels
                                .into_iter()
                                .enumerate()
                                .filter_map(move |(i, l)| {
                                    if tick_count <= 8
                                        || i == 0
                                        || i == tick_count - 1
                                        || i % (tick_count / 6).max(1) == 0
                                    {
                                        Some(view! { <span class="line-chart-x-tick">{l}</span> })
                                    } else {
                                        None
                                    }
                                })
                                .collect_view()
                        }}
                    </div>
                    <div class="line-chart-legend">
                        {all_series
                            .into_iter()
                            .map(|s| {
                                view! {
                                    <span class="line-chart-legend-item">
                                        <span
                                            class="line-chart-legend-swatch"
                                            style=format!("background: {}", s.color)
                                        ></span>
                                        {s.label.clone()}
                                    </span>
                                }
                            })
                            .collect_view()}
                    </div>
                }
                .into_any()
            }}
        </div>
    }
}
