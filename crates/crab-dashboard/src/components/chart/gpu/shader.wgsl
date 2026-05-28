// Minimal WGSL placeholder for future line rendering pipeline.

struct VsOut {
  @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_main(@location(0) a_pos: vec2<f32>) -> VsOut {
  var out: VsOut;
  out.pos = vec4<f32>(a_pos, 0.0, 1.0);
  return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
  return vec4<f32>(0.95, 0.45, 0.30, 1.0);
}
