//! Financial-style candlestick (OHLC) chart for token volume visualization.
//!
//! Renders green (up) and red (down) candles using SVG, with hover tooltips
//! and keyboard navigation for detailed OHLC data observation.

use leptos::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use wasm_bindgen::JsCast;

use super::chart::core::{format_tooltip_value, mouse_to_svg_x};
use super::chart::interaction::{line_band_style, line_tooltip_position_style, value_top_pct};

static KLINE_CHART_ID: AtomicUsize = AtomicUsize::new(0);

/// A single OHLC data point.
#[derive(Clone, Debug, PartialEq)]
pub struct CandlestickPoint {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// Precomputed candle geometry in SVG coordinate space.
#[derive(Clone, PartialEq)]
struct CandleGeom {
    /// Center X of this candle (in 0..100 space).
    cx: f64,
    /// Half-width of the candle body.
    half_w: f64,
    /// Y positions (in 0..H space, H=40).
    body_top: f64,
    body_bottom: f64,
    wick_top: f64,
    wick_bottom: f64,
    /// Whether close >= open (green/positive).
    is_up: bool,
    /// Original OHLC values.
    point: CandlestickPoint,
}

#[derive(Clone, PartialEq)]
struct KlineChartGeom {
    labels: Vec<String>,
    candles: Vec<CandleGeom>,
    ymin: f64,
    ymax: f64,
    span: f64,
    n: usize,
    candle_w: f64,
}

#[component]
pub fn CandlestickChart(
    x_labels: Signal<Vec<String>>,
    ohlc: Signal<Vec<Option<CandlestickPoint>>>,
    #[prop(default = 260)] height_px: u32,
    #[prop(default = "tokens")] y_unit: &'static str,
    empty_message: &'static str,
    #[prop(default = true)] interactive: bool,
) -> impl IntoView {
    let hover_index: RwSignal<Option<usize>> = RwSignal::new(None);
    let svg_ref: NodeRef<leptos::svg::Svg> = NodeRef::new();

    let chart_geom = Memo::new(move |_| {
        let labels = x_labels.get();
        let points = ohlc.get();
        if labels.is_empty() || points.is_empty() {
            return None;
        }
        let n = labels.len().min(points.len());
        if n == 0 {
            return None;
        }

        // Compute Y range from all high/low values.
        let mut ymin = f64::MAX;
        let mut ymax = f64::MIN;
        let mut has_data = false;
        for p in points.iter().take(n) {
            if let Some(ohlc) = p {
                ymin = ymin.min(ohlc.low);
                ymax = ymax.max(ohlc.high);
                has_data = true;
            }
        }
        if !has_data {
            return None;
        }
        if ymax <= ymin {
            ymin = 0.0;
            ymax = ymax.max(1.0);
        }
        let pad = (ymax - ymin) * 0.12;
        ymin = (ymin - pad).max(0.0);
        ymax += pad;
        let span = (ymax - ymin).max(1.0);

        const W: f64 = 100.0;
        const H: f64 = 40.0;
        let candle_w = if n > 1 {
            (W / n as f64 * 0.65).min(8.0)
        } else {
            10.0
        };
        let half_w = candle_w / 2.0;

        let candles: Vec<CandleGeom> = points
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, p)| {
                let cx = if n > 1 {
                    i as f64 / (n - 1) as f64 * W
                } else {
                    W / 2.0
                };
                if let Some(ohlc) = p {
                    let is_up = ohlc.close >= ohlc.open;
                    let body_top_px =
                        H - ((ohlc.open.max(ohlc.close) - ymin) / span).clamp(0.0, 1.0) * H;
                    let body_bot_px =
                        H - ((ohlc.open.min(ohlc.close) - ymin) / span).clamp(0.0, 1.0) * H;
                    let wick_top_px = H - ((ohlc.high - ymin) / span).clamp(0.0, 1.0) * H;
                    let wick_bot_px = H - ((ohlc.low - ymin) / span).clamp(0.0, 1.0) * H;
                    CandleGeom {
                        cx,
                        half_w,
                        body_top: body_top_px,
                        body_bottom: body_bot_px,
                        wick_top: wick_top_px,
                        wick_bottom: wick_bot_px,
                        is_up,
                        point: ohlc.clone(),
                    }
                } else {
                    CandleGeom {
                        cx,
                        half_w,
                        body_top: H,
                        body_bottom: H,
                        wick_top: H,
                        wick_bottom: H,
                        is_up: true,
                        point: CandlestickPoint {
                            open: 0.0,
                            high: 0.0,
                            low: 0.0,
                            close: 0.0,
                        },
                    }
                }
            })
            .collect();

        Some(KlineChartGeom {
            labels: labels[..n].to_vec(),
            candles,
            ymin,
            ymax,
            span,
            n,
            candle_w,
        })
    });

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        if !interactive {
            return;
        }
        let Some(svg_el) = svg_ref.get() else {
            return;
        };
        let svg_dom: web_sys::SvgsvgElement = svg_el.dyn_into().unwrap();
        let Some(svg_x) = mouse_to_svg_x(&ev, &svg_dom) else {
            hover_index.set(None);
            return;
        };
        let Some(geom) = chart_geom.get() else {
            hover_index.set(None);
            return;
        };
        if geom.n == 0 {
            hover_index.set(None);
            return;
        }
        // Find nearest candle center.
        let mut best_idx = 0;
        let mut best_dist = f64::MAX;
        for (i, c) in geom.candles.iter().enumerate() {
            let dist = (svg_x - c.cx).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }
        // Only highlight if within half a candle width.
        if best_dist <= geom.candle_w {
            hover_index.set(Some(best_idx));
        } else {
            hover_index.set(None);
        }
    };

    let on_mouseleave = move |_: web_sys::MouseEvent| {
        hover_index.set(None);
    };

    let on_keydown = move |ev: web_sys::KeyboardEvent| {
        if !interactive {
            return;
        }
        let Some(geom) = chart_geom.get() else {
            return;
        };
        if geom.n == 0 {
            return;
        }
        let current = hover_index.get_untracked();
        let new_idx = match ev.key().as_str() {
            "ArrowLeft" => match current {
                Some(idx) if idx > 0 => Some(idx - 1),
                None => Some(geom.n - 1),
                _ => current,
            },
            "ArrowRight" => match current {
                Some(idx) if idx < geom.n - 1 => Some(idx + 1),
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
        if interactive && chart_geom.get().is_some() && hover_index.get_untracked().is_none() {
            hover_index.set(Some(0));
        }
    };

    let on_blur = move |_: web_sys::FocusEvent| {
        hover_index.set(None);
    };

    view! {
        <div class="line-chart-wrap" style=format!("min-height: {}px", height_px + 48)>
            {move || {
                let chart_id = KLINE_CHART_ID.fetch_add(1, Ordering::Relaxed);
                let summary_id = format!("kline-chart-summary-{}", chart_id);
                let Some(geom) = chart_geom.get() else {
                    return view! {
                        <div class="text-center py-10 text-theme-muted text-sm">{empty_message}</div>
                    }.into_any();
                };
                let y_hint = format!("{:.0}\u{2013}{:.0} {}", geom.ymin, geom.ymax, y_unit);
                let tab_idx = if interactive { "0" } else { "-1" };
                let data_summary = format!(
                    "Candlestick chart with {} data points. Y range: {:.0} to {:.0} {}.",
                    geom.n, geom.ymin, geom.ymax, y_unit
                );

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
                            aria-label="Candlestick chart"
                            aria-describedby=summary_id
                        >
                            // Grid axes
                            <line x1="0" y1="40" x2="100" y2="40" class="line-chart-grid" />
                            <line x1="0" y1="0" x2="0" y2="40" class="line-chart-grid" />
                            // Horizontal grid lines
                            <line x1="0" y1="10" x2="100" y2="10" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="20" x2="100" y2="20" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />
                            <line x1="0" y1="30" x2="100" y2="30" stroke="var(--cc-border-light)" stroke-width="0.15" stroke-dasharray="1 2" vector-effect="non-scaling-stroke" opacity="0.4" />

                            // Candles
                            {geom.candles.iter().map(|c| {
                                let fill_color = if c.is_up { "var(--cc-success)" } else { "var(--cc-error)" };
                                let stroke_color = if c.is_up { "var(--cc-success)" } else { "var(--cc-error)" };
                                let body_h = (c.body_bottom - c.body_top).max(0.1);
                                view! {
                                    <g>
                                        // Wick (high-low line)
                                        <line
                                            x1=format!("{:.2}", c.cx)
                                            y1=format!("{:.2}", c.wick_top)
                                            x2=format!("{:.2}", c.cx)
                                            y2=format!("{:.2}", c.wick_bottom)
                                            stroke=stroke_color
                                            stroke-width="0.3"
                                            vector-effect="non-scaling-stroke"
                                        />
                                        // Body (open-close rectangle)
                                        <rect
                                            x=format!("{:.2}", c.cx - c.half_w)
                                            y=format!("{:.2}", c.body_top)
                                            width=format!("{:.2}", c.half_w * 2.0)
                                            height=format!("{:.2}", body_h)
                                            fill=fill_color
                                            stroke=stroke_color
                                            stroke-width="0.15"
                                            vector-effect="non-scaling-stroke"
                                            rx="0.3"
                                        />
                                    </g>
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
                            if idx >= geom.candles.len() {
                                return ().into_any();
                            }
                            let c = &geom.candles[idx];
                            let label = geom.labels.get(idx).cloned().unwrap_or_default();
                            let center_pct = c.cx;
                            let (band_left, band_w) = line_band_style(idx, geom.n);
                            let pos_style = line_tooltip_position_style(idx, geom.n);
                            // Show crosshair at the close price.
                            let close_top = value_top_pct(c.point.close, geom.ymin, geom.ymax);

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
                                    <div
                                        class="chart-crosshair-h"
                                        style=format!("top: {:.1}%", close_top)
                                    ></div>
                                    <div
                                        class="chart-axis-label"
                                        style=format!("top: {:.1}%", close_top)
                                    >
                                        {format_tooltip_value(c.point.close)}
                                    </div>
                                    <div
                                        class="chart-tooltip"
                                        style=format!("position: absolute; top: 8px; {}", pos_style)
                                    >
                                        <div class="chart-tooltip-label">{label}</div>
                                        <div class="chart-tooltip-row">
                                            <span class="chart-tooltip-row-name">
                                                <span class="chart-tooltip-dot" style="background: var(--cc-text-muted)"></span>
                                                <span>"Open"</span>
                                            </span>
                                            <span class="chart-tooltip-value">{format_precise(c.point.open)}</span>
                                        </div>
                                        <div class="chart-tooltip-row">
                                            <span class="chart-tooltip-row-name">
                                                <span class="chart-tooltip-dot" style="background: var(--cc-success)"></span>
                                                <span>"High"</span>
                                            </span>
                                            <span class="chart-tooltip-value">{format_precise(c.point.high)}</span>
                                        </div>
                                        <div class="chart-tooltip-row">
                                            <span class="chart-tooltip-row-name">
                                                <span class="chart-tooltip-dot" style="background: var(--cc-error)"></span>
                                                <span>"Low"</span>
                                            </span>
                                            <span class="chart-tooltip-value">{format_precise(c.point.low)}</span>
                                        </div>
                                        <div class="chart-tooltip-row">
                                            <span class="chart-tooltip-row-name">
                                                <span class="chart-tooltip-dot" style=format!("background: {}", if c.is_up { "var(--cc-success)" } else { "var(--cc-error)" })></span>
                                                <span>"Close"</span>
                                            </span>
                                            <span class="chart-tooltip-value">{format_precise(c.point.close)}</span>
                                        </div>
                                        <div class="chart-tooltip-row" style="opacity: 0.7; font-size: 0.85em">
                                            <span class="chart-tooltip-row-name">
                                                <span>"Range"</span>
                                            </span>
                                            <span class="chart-tooltip-value">
                                                {format_precise(c.point.high - c.point.low)}
                                            </span>
                                        </div>
                                    </div>
                                </>
                            }.into_any()
                        }}

                        <div class="line-chart-y-hint text-xs text-theme-muted font-mono">{y_hint}</div>
                    </div>
                    <div class="line-chart-x-labels">
                        {{
                            let tick_count = geom.labels.len();
                            geom.labels.into_iter().enumerate().filter_map(move |(i, l)| {
                                if tick_count <= 8 || i == 0 || i == tick_count - 1 || i % (tick_count / 6).max(1) == 0 {
                                    Some(view! { <span class="line-chart-x-tick">{l}</span> })
                                } else {
                                    None
                                }
                            }).collect_view()
                        }}
                    </div>
                    <div class="line-chart-legend">
                        <span class="line-chart-legend-item">
                            <span class="line-chart-legend-swatch" style="background: var(--cc-success)"></span>
                            "Up (close >= open)"
                        </span>
                        <span class="line-chart-legend-item">
                            <span class="line-chart-legend-swatch" style="background: var(--cc-error)"></span>
                            "Down (close < open)"
                        </span>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

fn format_precise(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.0}", v)
    } else if v >= 1.0 {
        format!("{:.2}", v)
    } else {
        format!("{:.4}", v)
    }
}
