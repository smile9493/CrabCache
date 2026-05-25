use leptos::prelude::*;

#[derive(Clone)]
pub struct ChartSeries {
    pub label: String,
    pub color: &'static str,
    pub values: Vec<Option<f64>>,
    pub dashed: bool,
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

#[component]
pub fn LineChart(
    x_labels: Signal<Vec<String>>,
    series: Signal<Vec<ChartSeries>>,
    #[prop(default = 220)] height_px: u32,
    #[prop(default = "ms")] y_unit: &'static str,
    empty_message: &'static str,
) -> impl IntoView {
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

                view! {
                    <div class="line-chart-plot">
                        <svg
                            class="line-chart-svg"
                            viewBox="0 0 100 40"
                            preserveAspectRatio="xMidYMid meet"
                            style=format!("height: {}px", height_px)
                        >
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            {all_series.iter().flat_map(|s| {
                                let segs = value_segments_indexed(&s.values);
                                segs.into_iter().map(move |(start_idx, seg_vals)| {
                                    let x_offset = start_idx as f64 * x_step;
                                    let points = scale_segment(
                                        &seg_vals,
                                        ymin,
                                        ymax,
                                        H,
                                        x_offset,
                                        x_step,
                                    );
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
                        </svg>
                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">
                            {format!("{:.0}–{:.0} {}", ymin, ymax, y_unit)}
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
            },
            ChartSeries {
                label: output_label.clone(),
                color: "var(--info)",
                values: output_values.get(),
                dashed: false,
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
