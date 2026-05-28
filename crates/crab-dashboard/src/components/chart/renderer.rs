//! Renderer abstraction for chart drawing backends.

use crate::components::chart::core::ChartSeries;

/// Backend-neutral line chart draw request.
#[derive(Clone)]
pub struct LineDrawRequest {
    pub labels: Vec<String>,
    pub series: Vec<ChartSeries>,
    pub y_unit: &'static str,
}

/// Shared interface for future Canvas/GPU rendering backends.
pub trait ChartRenderer {
    /// Draw a single line chart payload to the currently bound surface.
    fn render_line(&mut self, req: &LineDrawRequest) -> Result<(), String>;
}
