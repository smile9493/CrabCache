use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::line_chart::format_tooltip_value;

#[derive(Clone)]
pub struct ScatterPoint {
    pub x: f64,
    pub y: f64,
    pub color: &'static str,
    pub label: String,
}

fn scatter_range(points: &[ScatterPoint]) -> (f64, f64, f64, f64) {
    let mut xmin = f64::MAX;
    let mut xmax = f64::MIN;
    let mut ymin = f64::MAX;
    let mut ymax = f64::MIN;
    for p in points {
        if p.x.is_finite() && p.y.is_finite() {
            xmin = xmin.min(p.x);
            xmax = xmax.max(p.x);
            ymin = ymin.min(p.y);
            ymax = ymax.max(p.y);
        }
    }
    if xmax <= xmin {
        xmax = xmin + 1.0;
    }
    if ymax <= ymin {
        ymax = ymin + 1.0;
    }
    let pad_x = (xmax - xmin) * 0.1;
    let pad_y = (ymax - ymin) * 0.1;
    (
        (xmin - pad_x).max(0.0),
        xmax + pad_x,
        (ymin - pad_y).max(0.0),
        ymax + pad_y,
    )
}

#[component]
pub fn ScatterChart(
    points: Signal<Vec<ScatterPoint>>,
    x_label: String,
    y_label: String,
    #[prop(default = 220)] height_px: u32,
    empty_message: &'static str,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(svg_el) = svg_ref.get() else { return };
        let svg_dom: web_sys::SvgsvgElement = svg_el.dyn_into().unwrap();
        let pts = points.get_untracked();
        if pts.is_empty() {
            hover_index.set(None);
            return;
        }
        let (xmin, xmax, ymin, ymax) = scatter_range(&pts);
        let ctm = svg_dom.get_screen_ctm();
        let inv = ctm.as_ref().and_then(|c| c.inverse().ok());
        let (Some(_ctm), Some(inv)) = (ctm, inv) else {
            hover_index.set(None);
            return;
        };
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
            hover_index.set(None);
            return;
        }
        let svg_x = (d * (cx - e) - c * (cy - f)) / det;
        let svg_y = (a * (cy - f) - b * (cx - e)) / det;

        let mut closest = 0usize;
        let mut min_dist = f64::MAX;
        for (i, p) in pts.iter().enumerate() {
            let px = ((p.x - xmin) / (xmax - xmin) * 96.0 + 2.0).clamp(0.0, 100.0);
            let py = (40.0 - (p.y - ymin) / (ymax - ymin) * 36.0 - 2.0).clamp(0.0, 40.0);
            let dist = (svg_x - px).powi(2) + (svg_y - py).powi(2);
            if dist < min_dist {
                min_dist = dist;
                closest = i;
            }
        }
        if min_dist < 25.0 {
            hover_index.set(Some(closest));
        } else {
            hover_index.set(None);
        }
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
    };

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let pts = points.get();
                if pts.is_empty() {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any();
                }
                let (xmin, xmax, ymin, ymax) = scatter_range(&pts);
                let hover_idx = hover_index.get();

                view! {
                    <div style="position: relative">
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
                            {pts.iter().enumerate().map(|(i, p)| {
                                let cx = ((p.x - xmin) / (xmax - xmin) * 96.0 + 2.0).clamp(0.0, 100.0);
                                let cy = (40.0 - (p.y - ymin) / (ymax - ymin) * 36.0 - 2.0).clamp(0.0, 40.0);
                                let r = if hover_idx == Some(i) { 1.8 } else { 1.0 };
                                let stroke_w = if hover_idx == Some(i) { "0.8" } else { "0.4" };
                                view! {
                                    <circle
                                        cx=format!("{:.2}", cx)
                                        cy=format!("{:.2}", cy)
                                        r=format!("{:.1}", r)
                                        fill=p.color
                                        stroke="var(--cc-bg-card)"
                                        stroke-width=stroke_w
                                    />
                                }
                            }).collect_view()}
                            <rect x="0" y="0" width="100" height="40" fill="transparent" style="cursor: crosshair" />
                        </svg>
                        {hover_idx.and_then(|idx| {
                            let p = pts.get(idx)?;
                            let px = ((p.x - xmin) / (xmax - xmin) * 96.0 + 2.0).clamp(0.0, 100.0);
                            let pct = px;
                            let side = if pct > 75.0 { "right" } else { "left" };
                            let pos_style = if side == "right" {
                                format!("right: {:.1}%", 100.0 - pct)
                            } else {
                                format!("left: {:.1}%", pct)
                            };
                            Some(view! {
                                <div
                                    class="chart-tooltip"
                                    style=format!("position: absolute; top: 8px; {}; pointer-events: none; z-index: 10", pos_style)
                                >
                                    <div style="font-size: 0.6875rem; color: var(--cc-text-muted); margin-bottom: 0.25rem; font-family: var(--font-mono)">
                                        {p.label.clone()}
                                    </div>
                                    <div style="font-size: 0.75rem; font-weight: 600; color: var(--cc-text); font-family: var(--font-mono)">
                                        {format!("{}: {}, {}: {}", x_label, format_tooltip_value(p.x), y_label, format_tooltip_value(p.y))}
                                    </div>
                                </div>
                            })
                        })}
                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">
                            {format!("{}: {:.0} - {:.0}, {}: {:.0} - {:.0}", x_label, xmin, xmax, y_label, ymin, ymax)}
                        </div>
                    </div>
                    <div class="line-chart-x-labels">
                        <span class="line-chart-x-tick">{format!("{:.0}", xmin)}</span>
                        <span class="line-chart-x-tick">{format!("{:.0}", (xmin + xmax) / 2.0)}</span>
                        <span class="line-chart-x-tick">{format!("{:.0}", xmax)}</span>
                    </div>
                }.into_any()
            }}
        </div>
    }
}
