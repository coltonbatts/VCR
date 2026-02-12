// ascii_quantize.wgsl — Quantize pass: per-cell luma → glyph index.
//
// Renders at cell-grid resolution (cols x rows). Reads luma from the R
// channel of the luma texture and maps it to a discrete glyph index.
//
// Glyph mapping: id = clamp(floor((1.0 - luma) * N), 0, N-1)
// Dark pixels (low luma) → high glyph id (dense characters like @#%)
// Light pixels (high luma) → low glyph id (sparse characters like space/dot)
//
// Output: glyph index stored as normalized float in R channel.
//   R = id / (N - 1)    so downstream can recover id = round(R * (N - 1))

struct PostGlobals {
    resolution: vec2<f32>,
    time: f32,
    frame_index: u32,
    seed: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct AsciiParams {
    grid_size: vec2<f32>,    // (cols, rows) as float
    ramp_len: f32,           // number of glyphs in ramp
    _pad: f32,
}

@group(0) @binding(0) var luma_tex: texture_2d<f32>;
@group(0) @binding(1) var luma_sampler: sampler;
@group(0) @binding(2) var<uniform> globals: PostGlobals;
@group(0) @binding(3) var<uniform> params: AsciiParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0)
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0)
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let luma = textureSample(luma_tex, luma_sampler, input.uv).r;

    let n = max(params.ramp_len, 2.0);
    // Dark → dense glyph (high id), light → sparse glyph (low id)
    let id = clamp(floor((1.0 - luma) * n), 0.0, n - 1.0);

    // Normalize to [0, 1] range for Rgba8Unorm storage
    let normalized = id / (n - 1.0);

    return vec4<f32>(normalized, 0.0, 0.0, 1.0);
}
