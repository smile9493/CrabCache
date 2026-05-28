//! Experimental wgpu-backed renderer for a single line chart.
//!
//! The implementation is intentionally minimal and feature-gated (`gpu-spike`).
//! In non-gpu builds, this file provides a no-op fallback so the dashboard
//! compiles unchanged.

use crate::components::chart::renderer::{ChartRenderer, LineDrawRequest};

#[cfg(feature = "gpu-spike")]
pub struct GpuLineRenderer {
    pub ready: bool,
}

#[cfg(not(feature = "gpu-spike"))]
pub struct GpuLineRenderer {
    pub ready: bool,
}

impl GpuLineRenderer {
    pub fn new() -> Self {
        Self { ready: false }
    }
}

impl Default for GpuLineRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartRenderer for GpuLineRenderer {
    fn render_line(&mut self, req: &LineDrawRequest) -> Result<(), String> {
        if req.labels.is_empty() || req.series.is_empty() {
            return Err("empty line draw request".to_string());
        }
        // Spike placeholder: production wiring remains Canvas-first.
        self.ready = true;
        Ok(())
    }
}
