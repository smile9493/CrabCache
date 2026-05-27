use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

static LINE_CHART_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct ChartSeries {
    pub label: String,
    pub color: &'static str,
    pub values: Vec<Option<f64>>,
    pub dashed: bool,
    /// When true, renders a filled area below the line.
    pub fill: bool,
}

/// Horizontal threshold reference line.
#[derive(Clone)]
pub struct ThresholdLine {
    pub value: f64,
    pub label: String,
    pub color: &'static str,
}

fn value_segments_indexed(values: &[Option<f64>]) -> Vec<(usize, Vec<f64>)> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut current = Vec::new();
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(x) if x.is_finite() && *x > 0.0 => {
                if current.is_empty() {
                    start = i;
                }
                current.push(*x);
            }
            _ => {
                if !current.is_empty() {
                    segments.push((start, current));
                    current = Vec::new();
                }
            }
        }
    }
    if !current.is_empty() {
        segments.push((start, current));
    }
    segments
}

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

pub(crate) fn y_range(series: &[ChartSeries]) -> (f64, f64) {
    let mut ymin = f64::MAX;
    let mut ymax = f64::MIN;
    for s in series {
        for v in &s.values {
            if let Some(x) = v
                && *x > 0.0
                && x.is_finite()
            {
                ymin = ymin.min(*x);
                ymax = ymax.max(*x);
            }
        }
    }
    if ymax <= ymin {
        ymin = 0.0;
        ymax = 1.0;
    }
    let pad = (ymax - ymin) * 0.1;
    ((ymin - pad).max(0.0), ymax + pad)
}

/// Convert a mouse event's client X to SVG viewBox X coordinate.
/// Uses the inverse of the SVG's screen CTM (SvgMatrix).
pub(crate) fn mouse_to_svg_x(
    ev: &web_sys::MouseEvent,
    svg: &web_sys::SvgsvgElement,
) -> Option<f64> {
    let ctm = svg.get_screen_ctm()?;
    let inv = ctm.inverse().ok()?;
    let cx = ev.client_x() as f64;
    let cy = ev.client_y() as f64;
    let a = inv.a() as f64;
    let b = inv.b() as f64;
    let c = inv.c() as f64;
    let d = inv.d() as f64;
    let e = inv.e() as f64;
    let f = inv.f() as f64;
    let det = a * d - b * c;
    if det.abs() < 1e-10 {
        return None;
    }
    Some((d * (cx - e) - c * (cy - f)) / det)
}

/// Format a numeric value for tooltip display — abbreviated (axis hint).
pub(crate) fn format_tooltip_value(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
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
        let idx = if n == 1 {
            0
        } else {
            let step = W / (n - 1) as f64;
            ((svg_x / step).round() as usize).min(n - 1)
        };

        hover_index.set(Some(idx));
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        let labels = x_labels.get_untracked();
        let n = labels.len();
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
        let labels = x_labels.get_untracked();
        if !labels.is_empty() && hover_index.get_untracked().is_none() {
            hover_index.set(Some(0));
        }
    };

    let on_blur = move |_: web_sys::FocusEvent| {
        hover_index.set(None);
    };

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let chart_id = LINE_CHART_ID.fetch_add(1, Ordering::Relaxed);
                let summary_id = format!("line-chart-summary-{}", chart_id);
                let labels = x_labels.get();
                let all_series = series.get();
                if labels.is_empty() || all_series.is_empty() {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any();
                }
                let has_point = all_series.iter().any(|s| {
                    s.values.iter().any(|v| matches!(v, Some(x) if *x > 0.0))
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
                let x_step = if n > 1 { W / (n - 1) as f64 } else { 0.0 };
                let span = (ymax - ymin).max(1.0);

                let series_names: Vec<String> = all_series.iter().map(|s| s.label.clone()).collect();
                let data_summary = format!(
                    "Line chart with {} data points and {} series: {}. Y range: {:.0} to {:.0} {}.",
                    labels.len(),
                    series_names.len(),
                    series_names.join(", "),
                    ymin, ymax, y_unit
                );

                let hover_idx = hover_index.get();

                // Build tooltip data with precise values
                let tooltip_data = hover_idx.and_then(|idx| {
                    if idx >= labels.len() { return None; }
                    let label = labels[idx].clone();
                    let values: Vec<(String, String, &'static str, f64)> = all_series.iter().filter_map(|s| {
                        let v = s.values.get(idx).and_then(|opt| *opt)?;
                        Some((s.label.clone(), format_precise_value(v), s.color, v))
                    }).collect();
                    if values.is_empty() { None } else { Some((idx, label, values)) }
                });

                // Crosshair X position
                let crosshair_x = hover_idx.map(|idx| {
                    if n > 1 { idx as f64 * x_step } else { W / 2.0 }
                });

                // Find the primary (first) series value at hover for horizontal crosshair
                let crosshair_y = hover_idx.and_then(|idx| {
                    let v = all_series.first()?.values.get(idx).and_then(|opt| *opt)?;
                    if v.is_finite() && v > 0.0 {
                        let norm = ((v - ymin) / span).clamp(0.0, 1.0);
                        Some(H - norm * H)
                    } else {
                        None
                    }
                });

                // Y value at crosshair for axis label
                let crosshair_y_value = hover_idx.and_then(|idx| {
                    let v = all_series.first()?.values.get(idx).and_then(|opt| *opt)?;
                    if v.is_finite() && v > 0.0 { Some(v) } else { None }
                });

                view! {
                    <div class="line-chart-plot" style="position: relative">
                        <div id={summary_id.clone()} class="sr-only">{data_summary}</div>
                        <svg
                            node_ref=svg_ref
                            class="line-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px", height_px)
                            role="img"
                            aria-label="Line chart"
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
                            // Horizontal grid lines (subtle Y guides)
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

                            // === Financial Crosshair ===
                            // Hover column highlight band
                            {crosshair_x.map(|cx| {
                                let half = if n > 1 { x_step * 0.4 } else { 2.0 };
                                view! {
                                    <rect
                                        x={cx - half} y="0"
                                        width={half * 2.0} height="40"
                                        fill="var(--cc-text-muted)"
                                        opacity="0.04"
                                    />
                                }
                            })}
                            // Vertical crosshair line
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
                            // Horizontal crosshair line (follows primary series)
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

                            // Highlight dots — larger, with glow ring
                            {hover_idx.map(|idx| {
                                all_series.iter().filter_map(move |s| {
                                    let v = s.values.get(idx).and_then(|opt| *opt)?;
                                    let cx = if n > 1 { idx as f64 * x_step } else { W / 2.0 };
                                    let norm = ((v - ymin) / span).clamp(0.0, 1.0);
                                    let cy = H - norm * H;
                                    let color = s.color;
                                    Some(view! {
                                        <>
                                            // Glow ring
                                            <circle
                                                cx=cx cy=cy r="2.5"
                                                fill=color opacity="0.15"
                                            />
                                            // Solid dot
                                            <circle
                                                cx=cx cy=cy r="1.5"
                                                fill=color
                                                stroke="var(--cc-bg-card)"
                                                stroke-width="0.7"
                                            />
                                        </>
                                    })
                                }).collect_view()
                            })}

                            // Invisible hit area
                            <rect
                                x="0" y="0" width="100" height="40"
                                fill="transparent"
                                style="cursor: crosshair"
                            />
                        </svg>

                        // Y-axis value label (appears on hover)
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

                        // Financial tooltip (positioned outside SVG)
                        {tooltip_data.map(|(idx, label, values)| {
                            let pct = if n > 1 { idx as f64 / (n - 1) as f64 * 100.0 } else { 50.0 };
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
                            {format!("{:.0}\u{2013}{:.0} {}", ymin, ymax, y_unit)}
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

#[component]
pub fn TokenLineChart(
    x_labels: Signal<Vec<String>>,
    input_values: Signal<Vec<Option<f64>>>,
    output_values: Signal<Vec<Option<f64>>>,
    input_label: String,
    output_label: String,
    #[prop(default = 220)] height_px: u32,
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
            empty_message=empty_message
        />
    }
}
