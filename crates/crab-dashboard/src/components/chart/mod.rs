//! Dashboard charts — shared types, theme palette, and Plotters SVG rendering.
//!
//! Reference implementation: local `plotters/` clone (gitignored). Production uses the
//! crates.io `plotters` dependency with `svg_backend` only (no bitmap/fonts).

pub mod canvas_render;
pub mod canvas_renderer;
pub mod core;
#[cfg(feature = "gpu-spike")]
pub mod gpu;
pub mod interaction;
pub mod renderer;
pub mod svg_render;
pub mod theme;

pub use core::{
    ChartSeries, DonutSegment, ScatterPoint, ThresholdLine, WaterfallStage, format_tooltip_value,
    mouse_to_svg_x, scatter_range, sparkline_hit_rate_pct, sparkline_requests, sparkline_tokens,
    trend_points, value_segments_indexed, waterfall_stages_from_log, y_range,
};
pub use theme::{ChartPalette, resolve_series_color};
