use leptos::prelude::*;
use wasm_bindgen::JsCast;

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

fn y_range(series: &[ChartSeries]) -> (f64, f64) {
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
fn mouse_to_svg_x(ev: &web_sys::MouseEvent, svg: &web_sys::SvgsvgElement) -> Option<f64> {
    let ctm = svg.get_screen_ctm()?;
    let inv = ctm.inverse().ok()?;
    let pt = svg.create_svg_point();
    pt.set_x(ev.client_x() as f32);
    pt.set_y(ev.client_y() as f32);
    let transformed = pt.matrix_transform(&inv);
    Some(transformed.x() as f64)
}

/// Format a numeric value for tooltip display.
fn format_tooltip_value(v: f64) -> String {
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

    // We need a node ref for the SVG to do coordinate transforms
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(svg_el) = svg_ref.get() else { return };
        let svg_dom: web_sys::SvgsvgElement = svg_el.dyn_into().unwrap();
        let Some(svg_x) = mouse_to_svg_x(&ev, &svg_dom) else {
            hover_index.set(None);
            return;
        };

        // Get current labels to determine data point count
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

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
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

                // Tooltip data derivation
                let hover_idx = hover_index.get();
                let tooltip_data = hover_idx.and_then(|idx| {
                    if idx >= labels.len() { return None; }
                    let label = labels[idx].clone();
                    let values: Vec<(String, String, &'static str)> = all_series.iter().filter_map(|s| {
                        let v = s.values.get(idx).and_then(|opt| *opt)?;
                        Some((s.label.clone(), format_tooltip_value(v), s.color))
                    }).collect();
                    if values.is_empty() { None } else { Some((idx, label, values)) }
                });

                // Crosshair X position in viewBox coords
                let crosshair_x = hover_idx.map(|idx| {
                    if n > 1 { idx as f64 * x_step } else { W / 2.0 }
                });

                view! {
                    <div class="line-chart-plot" style="position: relative">
                        <svg
                            node_ref=svg_ref
                            class="line-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px", height_px)
                            on:mousemove=on_mousemove
                            on:mouseleave=on_mouseleave
                        >
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            // Area fills (rendered before lines so lines appear on top)
                            {all_series.iter().filter(|s| s.fill).flat_map(|s| {
                                let segs = value_segments_indexed(&s.values);
                                segs.into_iter().map(move |(start_idx, seg_vals)| {
                                    let x_offset = start_idx as f64 * x_step;
                                    let line_points = scale_segment(&seg_vals, ymin, ymax, H, x_offset, x_step);
                                    // Build closed polygon: line points + bottom-right + bottom-left
                                    let first_x = x_offset;
                                    let last_x = x_offset + (seg_vals.len().saturating_sub(1)) as f64 * x_step;
                                    let poly = format!("{} {:.2},{:.2} {:.2},{:.2}", line_points, last_x, H, first_x, H);
                                    view! {
                                        <polygon
                                            points=poly
                                            fill=s.color
                                            opacity="0.12"
                                        />
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
                                let span = (ymax - ymin).max(1.0);
                                let norm = ((t.value - ymin) / span).clamp(0.0, 1.0);
                                let y = H - norm * H;
                                if y < 0.0 || y > H { return None; }
                                let color = t.color;
                                let label = t.label.clone();
                                Some(view! {
                                    <>
                                        <line
                                            x1="0" y1=y x2="100" y2=y
                                            stroke=color
                                            stroke-width="0.4"
                                            stroke-dasharray="3 2"
                                            vector-effect="non-scaling-stroke"
                                            opacity="0.7"
                                        />
                                        <text
                                            x="1" y={format!("{:.1}", y - 0.8)}
                                            fill=color
                                            font-size="2.2"
                                            font-family="var(--font-mono)"
                                        >
                                            {label}
                                        </text>
                                    </>
                                })
                            }).collect_view()}
                            // Crosshair line
                            {crosshair_x.map(|cx| view! {
                                <line
                                    x1=cx y1="0" x2=cx y2="40"
                                    stroke="var(--cc-text-muted)"
                                    stroke-width="0.3"
                                    stroke-dasharray="1.5 1.5"
                                    vector-effect="non-scaling-stroke"
                                    style="opacity: 0.6"
                                />
                            })}
                            // Highlight dots
                            {hover_idx.map(|idx| {
                                all_series.iter().filter_map(move |s| {
                                    let v = s.values.get(idx).and_then(|opt| *opt)?;
                                    let cx = if n > 1 { idx as f64 * x_step } else { W / 2.0 };
                                    let span = (ymax - ymin).max(1.0);
                                    let norm = ((v - ymin) / span).clamp(0.0, 1.0);
                                    let cy = H - norm * H;
                                    let color = s.color;
                                    Some(view! {
                                        <circle
                                            cx=cx cy=cy r="1.2"
                                            fill=color
                                            stroke="var(--cc-bg-card)"
                                            stroke-width="0.6"
                                        />
                                    })
                                }).collect_view()
                            })}
                            // Invisible hit area for mouse events (must be on top)
                            <rect
                                x="0" y="0" width="100" height="40"
                                fill="transparent"
                                style="cursor: crosshair"
                            />
                        </svg>
                        // Tooltip overlay (positioned in CSS)
                        {tooltip_data.map(|(idx, label, values)| {
                            // Position: use the crosshair X percentage
                            let pct = if n > 1 { idx as f64 / (n - 1) as f64 * 100.0 } else { 50.0 };
                            // Flip to left if past 75%
                            let side = if pct > 75.0 { "right" } else { "left" };
                            let pos_style = if side == "right" {
                                format!("right: {:.1}%", 100.0 - pct)
                            } else {
                                format!("left: {:.1}%", pct)
                            };
                            view! {
                                <div
                                    class="chart-tooltip"
                                    style=format!("position: absolute; top: 8px; {}; pointer-events: none; z-index: 10", pos_style)
                                >
                                    <div class="chart-tooltip-label" style="font-size: 0.6875rem; color: var(--cc-text-muted); margin-bottom: 0.25rem; font-family: var(--font-mono)">
                                        {label}
                                    </div>
                                    {values.into_iter().map(|(name, val, color)| {
                                        view! {
                                            <div class="chart-tooltip-row" style="display: flex; align-items: center; gap: 0.35rem; font-size: 0.75rem; line-height: 1.4">
                                                <span style=format!("width: 0.5rem; height: 0.5rem; border-radius: 50%; background: {}; flex-shrink: 0", color)></span>
                                                <span style="color: var(--cc-text-muted)">{name}:</span>
                                                <span style="font-family: var(--font-mono); font-weight: 600; color: var(--cc-text)">{val}</span>
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
                                Some(view! {
                                    <span class="line-chart-x-tick">{l}</span>
                                })
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
