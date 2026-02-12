// ascii_cell_edge_boost.wgsl — Cell pass: edge-aware luma adjustment.
//
// Renders at cell-grid resolution. Computes a simple finite-difference edge
// strength and darkens edges to produce crisper ASCII silhouettes.
//
// Algorithm:
//   dx = abs(luma(x+1,y) - luma(x,y))
//   dy = abs(luma(x,y+1) - luma(x,y))
//   edge = clamp((dx + dy) * edge_gain, 0..1)
//   adjusted = clamp(luma - edge * boost, 0..1)
//
// Constants: edge_gain = 2.0, boost = 0.25

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
    ramp_len: f32,
    _pad: f32,
}

const EDGE_GAIN: f32 = 2.0;
const BOOST: f32 = 0.25;

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
    let grid = params.grid_size;
    let uv = input.uv;

    // Cell coordinates (same convention as quantize: uv in 0..1 maps to grid)
    let cell_uv = vec2<f32>(1.0 / grid.x, 1.0 / grid.y);
    let coord = floor(uv * grid);
    let cx = u32(coord.x);
    let cy = u32(coord.y);
    let cols = u32(grid.x);
    let rows = u32(grid.y);

    // Sample luma at (x,y), (x+1,y), (x,y+1) — use cell centers in texel space
    let uv_center = (coord + vec2<f32>(0.5, 0.5)) * cell_uv;
    let luma = textureSample(luma_tex, luma_sampler, uv_center).r;

    // At right/bottom edges, use center so gradient contribution is zero
    var uv_right = uv_center;
    if coord.x + 1.0 < grid.x {
        uv_right = vec2<f32>((coord.x + 1.5) * cell_uv.x, (coord.y + 0.5) * cell_uv.y);
    }
    let luma_right = textureSample(luma_tex, luma_sampler, uv_right).r;

    var uv_down = uv_center;
    if coord.y + 1.0 < grid.y {
        uv_down = vec2<f32>((coord.x + 0.5) * cell_uv.x, (coord.y + 1.5) * cell_uv.y);
    }
    let luma_down = textureSample(luma_tex, luma_sampler, uv_down).r;

    let dx = abs(luma_right - luma);
    let dy = abs(luma_down - luma);
    let edge = clamp((dx + dy) * EDGE_GAIN, 0.0, 1.0);
    let adjusted = clamp(luma - edge * BOOST, 0.0, 1.0);

    return vec4<f32>(adjusted, 0.0, 0.0, 1.0);
}
