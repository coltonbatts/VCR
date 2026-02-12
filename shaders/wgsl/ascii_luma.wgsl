// ascii_luma.wgsl — Analyze pass: RGBA input → per-cell average luma.
//
// Renders at cell-grid resolution (cols x rows). Each fragment covers one
// terminal cell. Samples a 4x4 grid within the cell region of the input
// texture and computes average Rec.709 luminance.

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

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
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
    let uv = input.uv;

    // Cell dimensions in UV space
    let cell_uv = vec2<f32>(1.0 / params.grid_size.x, 1.0 / params.grid_size.y);

    // Top-left corner of this cell in input texture UV space
    let cell_origin = floor(uv * params.grid_size) * cell_uv;

    // Sample a 4x4 grid within the cell and average the luminance.
    // Offsets are at (0.125, 0.375, 0.625, 0.875) within cell — evenly spaced, centered.
    var luma_sum: f32 = 0.0;
    let sample_count: f32 = 16.0;

    for (var sy: u32 = 0u; sy < 4u; sy = sy + 1u) {
        for (var sx: u32 = 0u; sx < 4u; sx = sx + 1u) {
            let offset = vec2<f32>(
                (f32(sx) + 0.5) * 0.25,
                (f32(sy) + 0.5) * 0.25
            );
            let sample_uv = cell_origin + offset * cell_uv;
            let color = textureSample(input_tex, input_sampler, sample_uv);
            // Rec.709 luminance (assumes linear RGB)
            let luma = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
            luma_sum = luma_sum + luma;
        }
    }

    let avg_luma = clamp(luma_sum / sample_count, 0.0, 1.0);

    // Store luma in R channel
    return vec4<f32>(avg_luma, 0.0, 0.0, 1.0);
}
