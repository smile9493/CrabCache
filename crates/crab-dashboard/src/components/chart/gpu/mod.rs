//! GPU rendering spike module.
//!
//! This module is intentionally feature-gated and optional. It provides
//! a thin skeleton for a future wgpu renderer while keeping the current
//! production path on Canvas/SVG.

pub mod buffers;
pub mod renderer;

pub use renderer::GpuLineRenderer;
