//! GPU buffer payloads for the line-rendering spike.

/// Normalized XY vertices in clip-space-ish range prepared for GPU upload.
#[derive(Clone, Default)]
pub struct LineVertexBuffer {
    pub vertices: Vec<[f32; 2]>,
}

impl LineVertexBuffer {
    pub fn from_samples(samples: &[(f64, f64)]) -> Self {
        let vertices = samples
            .iter()
            .map(|(x, y)| [*x as f32, *y as f32])
            .collect();
        Self { vertices }
    }
}
