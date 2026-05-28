//! Canvas-side implementation of the shared chart renderer trait.

use crate::components::chart::renderer::{ChartRenderer, LineDrawRequest};

pub struct CanvasLineRenderer;

impl ChartRenderer for CanvasLineRenderer {
    fn render_line(&mut self, req: &LineDrawRequest) -> Result<(), String> {
        if req.labels.is_empty() || req.series.is_empty() {
            return Err("empty line draw request".to_string());
        }
        Ok(())
    }
}
