// ascii_debug.wgsl — Debug visualization pass: glyph index → RGBA grayscale.
//
// Renders at full output resolution. Each pixel looks up which cell it
// belongs to, samples the glyph index texture at that cell, and converts
// back to a grayscale value for visual verification.
//
// Recovery: id = round(R * (N - 1))
// Debug gray: gray = 1.0 - (id / (N - 1))
//
// This produces a stable quantization field preview without atlas rendering.

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

@group(0) @binding(0) var glyph_tex: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
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
    // Map this output pixel to the center of the corresponding cell
    // Use nearest-neighbor sampling by snapping to cell center
    let cell_coord = floor(input.uv * params.grid_size);
    let cell_center_uv = (cell_coord + 0.5) / params.grid_size;

    let normalized_id = textureSample(glyph_tex, glyph_sampler, cell_center_uv).r;

    let n = max(params.ramp_len, 2.0);
    let id = round(normalized_id * (n - 1.0));

    // Convert back to grayscale for debug visualization
    // id=0 (sparse/light) → gray=1.0 (white), id=N-1 (dense/dark) → gray=0.0 (black)
    let gray = 1.0 - (id / (n - 1.0));

    return vec4<f32>(gray, gray, gray, 1.0);
}
