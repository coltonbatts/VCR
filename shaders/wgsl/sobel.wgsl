// sobel.wgsl — Sobel edge detection post-processing shader.
// Computes grayscale edge intensity using a 3x3 Sobel kernel.

struct PostGlobals {
    resolution: vec2<f32>,
    time: f32,
    frame_index: u32,
    seed: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct SobelParams {
    strength: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> globals: PostGlobals;
@group(0) @binding(3) var<uniform> params: SobelParams;

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

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(1.0 / globals.resolution.x, 1.0 / globals.resolution.y);
    let uv = input.uv;

    // Sample 3x3 neighbourhood luminance
    let tl = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>(-texel.x,  texel.y)).rgb);
    let tc = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>( 0.0,      texel.y)).rgb);
    let tr = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>( texel.x,  texel.y)).rgb);
    let ml = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>(-texel.x,  0.0    )).rgb);
    let mr = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>( texel.x,  0.0    )).rgb);
    let bl = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>(-texel.x, -texel.y)).rgb);
    let bc = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>( 0.0,     -texel.y)).rgb);
    let br = luminance(textureSample(input_tex, input_sampler, uv + vec2<f32>( texel.x, -texel.y)).rgb);

    // Sobel kernels
    let gx = -tl - 2.0 * ml - bl + tr + 2.0 * mr + br;
    let gy = -tl - 2.0 * tc - tr + bl + 2.0 * bc + br;

    let edge = clamp(sqrt(gx * gx + gy * gy) * params.strength, 0.0, 1.0);

    // Preserve alpha from center sample
    let center_alpha = textureSample(input_tex, input_sampler, uv).a;
    return vec4<f32>(vec3<f32>(edge), center_alpha);
}
