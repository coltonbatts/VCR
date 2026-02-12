// ascii_cell_bayer_dither.wgsl — Cell pass: ordered Bayer dithering before quantization.
//
// Renders at cell-grid resolution. Applies a subtle ordered dither to the luma
// field to break up banding without destroying structure.
//
// Algorithm (deterministic, purely a function of (x,y) and constants):
//   threshold = (bayer8[(x % 8, y % 8)] + 0.5) / 64.0
//   dither_strength = 1.0 / ramp_len
//   adjusted = clamp(luma + (threshold - 0.5) * dither_strength, 0..1)
//
// Dither is subtle (scaled to glyph step size). Do not animate thresholds.

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

// Standard 8x8 Bayer matrix (values 0..63), row-major.
// Avoids dynamic array indexing; uses explicit switch for WGSL compatibility.
fn bayer8_at(ix: u32, iy: u32) -> f32 {
    let idx = (iy % 8u) * 8u + (ix % 8u);
    switch idx {
        case 0u: { return 0.0; }
        case 1u: { return 32.0; }
        case 2u: { return 8.0; }
        case 3u: { return 40.0; }
        case 4u: { return 2.0; }
        case 5u: { return 34.0; }
        case 6u: { return 10.0; }
        case 7u: { return 42.0; }
        case 8u: { return 48.0; }
        case 9u: { return 16.0; }
        case 10u: { return 56.0; }
        case 11u: { return 24.0; }
        case 12u: { return 50.0; }
        case 13u: { return 18.0; }
        case 14u: { return 58.0; }
        case 15u: { return 26.0; }
        case 16u: { return 12.0; }
        case 17u: { return 44.0; }
        case 18u: { return 4.0; }
        case 19u: { return 36.0; }
        case 20u: { return 14.0; }
        case 21u: { return 46.0; }
        case 22u: { return 6.0; }
        case 23u: { return 38.0; }
        case 24u: { return 60.0; }
        case 25u: { return 28.0; }
        case 26u: { return 52.0; }
        case 27u: { return 20.0; }
        case 28u: { return 62.0; }
        case 29u: { return 30.0; }
        case 30u: { return 54.0; }
        case 31u: { return 22.0; }
        case 32u: { return 3.0; }
        case 33u: { return 35.0; }
        case 34u: { return 11.0; }
        case 35u: { return 43.0; }
        case 36u: { return 1.0; }
        case 37u: { return 33.0; }
        case 38u: { return 9.0; }
        case 39u: { return 41.0; }
        case 40u: { return 51.0; }
        case 41u: { return 19.0; }
        case 42u: { return 59.0; }
        case 43u: { return 27.0; }
        case 44u: { return 49.0; }
        case 45u: { return 17.0; }
        case 46u: { return 57.0; }
        case 47u: { return 25.0; }
        case 48u: { return 15.0; }
        case 49u: { return 47.0; }
        case 50u: { return 7.0; }
        case 51u: { return 39.0; }
        case 52u: { return 13.0; }
        case 53u: { return 45.0; }
        case 54u: { return 5.0; }
        case 55u: { return 37.0; }
        case 56u: { return 63.0; }
        case 57u: { return 31.0; }
        case 58u: { return 55.0; }
        case 59u: { return 23.0; }
        case 60u: { return 61.0; }
        case 61u: { return 29.0; }
        case 62u: { return 53.0; }
        case 63u: { return 21.0; }
        default: { return 0.0; }
    }
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let grid = params.grid_size;
    let uv = input.uv;

    // Cell coordinates (same convention as quantize: uv in 0..1 maps to grid)
    let coord = floor(uv * grid);
    let cx = u32(coord.x);
    let cy = u32(coord.y);

    let cell_uv = vec2<f32>(1.0 / grid.x, 1.0 / grid.y);
    let uv_center = (coord + vec2<f32>(0.5, 0.5)) * cell_uv;
    let luma = textureSample(luma_tex, luma_sampler, uv_center).r;

    let bayer_val = bayer8_at(cx, cy);
    let threshold = (bayer_val + 0.5) / 64.0;
    let dither_strength = 1.0 / max(params.ramp_len, 2.0);
    let adjusted = clamp(luma + (threshold - 0.5) * dither_strength, 0.0, 1.0);

    return vec4<f32>(adjusted, 0.0, 0.0, 1.0);
}
