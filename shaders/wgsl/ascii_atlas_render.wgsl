// ascii_atlas_render.wgsl — Glyph atlas render pass.
//
// Replaces the debug visualization. Renders at full output resolution.
// For each fragment: determine cell, sample glyph id, map to atlas UV,
// sample atlas glyph, output RGBA.
//
// glyph_index = round(normalized_value * (N - 1))
// atlas_col = glyph_index % atlas_columns
// atlas_row = glyph_index / atlas_columns
//
// Nearest sampling throughout for pixel purity.

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

struct AtlasMetadata {
    cell_width: f32,
    cell_height: f32,
    atlas_columns: f32,
    _pad: f32,
}

@group(0) @binding(0) var glyph_tex: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;
@group(0) @binding(2) var<uniform> globals: PostGlobals;
@group(0) @binding(3) var<uniform> params: AsciiParams;
@group(0) @binding(4) var atlas_tex: texture_2d<f32>;
@group(0) @binding(5) var atlas_sampler: sampler;
@group(0) @binding(6) var<uniform> atlas_meta: AtlasMetadata;

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
    // Vertex UV spans [0,2]^2; normalize to [0,1] for pixel coordinates
    let ndc_x = input.uv.x / 2.0;
    let ndc_y = input.uv.y / 2.0;
    let pixel_x = ndc_x * globals.resolution.x;
    let pixel_y = ndc_y * globals.resolution.y;

    let cell_width_px = globals.resolution.x / params.grid_size.x;
    let cell_height_px = globals.resolution.y / params.grid_size.y;

    let cell_col = u32(floor(pixel_x / cell_width_px));
    let cell_row = u32(floor(pixel_y / cell_height_px));

    let cell_center_uv_x = (f32(cell_col) + 0.5) / params.grid_size.x;
    let cell_center_uv_y = (f32(cell_row) + 0.5) / params.grid_size.y;
    let cell_center_uv = vec2<f32>(cell_center_uv_x, cell_center_uv_y);

    let normalized_id = textureSample(glyph_tex, glyph_sampler, cell_center_uv).r;

    let n = max(params.ramp_len, 2.0);
    let glyph_index = u32(round(normalized_id * (n - 1.0)));

    let atlas_col = glyph_index % u32(atlas_meta.atlas_columns);
    let atlas_row = glyph_index / u32(atlas_meta.atlas_columns);

    let atlas_cols = atlas_meta.atlas_columns;
    let glyph_count = max(u32(n), 1u);
    let atlas_rows = (glyph_count + u32(atlas_cols) - 1u) / u32(atlas_cols);
    let atlas_rows_f = max(f32(atlas_rows), 1.0);

    let fx = (pixel_x - f32(cell_col) * cell_width_px) / cell_width_px;
    let fy = (pixel_y - f32(cell_row) * cell_height_px) / cell_height_px;

    let u = (f32(atlas_col) + clamp(fx, 0.0, 1.0)) / atlas_cols;
    let v = (f32(atlas_row) + clamp(fy, 0.0, 1.0)) / atlas_rows_f;

    let atlas_uv = vec2<f32>(u, v);
    let color = textureSample(atlas_tex, atlas_sampler, atlas_uv);

    return color;
}
