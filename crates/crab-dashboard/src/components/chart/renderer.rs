//! Renderer abstraction for chart drawing backends.

use crate::components::chart::core::{ChartSeries, ThresholdLine};

/// Backend-neutral line chart draw request.
#[derive(Clone)]
pub struct LineDrawRequest {
    pub labels: Vec<String>,
    pub series: Vec<ChartSeries>,
    pub y_unit: &'static str,
    pub thresholds: Vec<ThresholdLine>,
    pub width_px: u32,
    pub height_px: u32,
    /// Optional explicit Y axis range. When set, the renderer uses these instead
    /// of computing min/max from the data, keeping the rendered axis consistent
    /// with tooltip coordinate calculations.
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
}

/// Backend-neutral bar chart draw request.
#[derive(Clone)]
pub struct BarDrawRequest {
    pub labels: Vec<String>,
    pub series: Vec<ChartSeries>,
    pub y_unit: &'static str,
    pub width_px: u32,
    pub height_px: u32,
}

/// Shared interface for Canvas rendering backends.
pub trait ChartRenderer {
    /// Draw a line chart onto the given canvas element.
    fn render_line(
        &mut self,
        canvas: &web_sys::HtmlCanvasElement,
        req: &LineDrawRequest,
    ) -> Result<(), String>;

    /// Draw a bar chart onto the given canvas element.
    fn render_bar(
        &mut self,
        canvas: &web_sys::HtmlCanvasElement,
        req: &BarDrawRequest,
    ) -> Result<(), String>;
}
