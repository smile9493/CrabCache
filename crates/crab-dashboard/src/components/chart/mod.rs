//! Dashboard charts — shared types, theme palette, and Plotters SVG rendering.
//!
//! Reference implementation: local `plotters/` clone (gitignored). Production uses the
//! crates.io `plotters` dependency with `svg_backend` only (no bitmap/fonts).

pub mod canvas_render;
pub mod core;
pub mod renderer;
pub mod canvas_renderer;
#[cfg(feature = "gpu-spike")]
pub mod gpu;
pub mod host;
pub mod interaction;
pub mod svg_render;
pub mod theme;

pub use core::{
    ChartSeries, DonutSegment, ScatterPoint, ThresholdLine, WaterfallStage, format_tooltip_value,
    mouse_to_svg_x, scatter_range, sparkline_hit_rate_pct, sparkline_requests, sparkline_tokens,
    trend_points, value_segments_indexed, waterfall_stages_from_log, y_range,
};
pub use host::PlottersChartFrame;
pub use theme::{resolve_series_color, ChartPalette};
