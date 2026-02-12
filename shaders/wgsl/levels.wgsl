// levels.wgsl — Simple levels correction post-processing shader.
// Applies: color = pow(clamp(color + lift, 0.0, 1.0) * gain, vec3(gamma))

struct PostGlobals {
    resolution: vec2<f32>,
    time: f32,
    frame_index: u32,
    seed: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct LevelsParams {
    gamma: f32,
    lift: f32,
    gain: f32,
    _pad: f32,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> globals: PostGlobals;
@group(0) @binding(3) var<uniform> params: LevelsParams;

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
    let color = textureSample(input_tex, input_sampler, input.uv);
    let lifted = clamp(color.rgb + vec3<f32>(params.lift), vec3<f32>(0.0), vec3<f32>(1.0));
    let gained = clamp(lifted * params.gain, vec3<f32>(0.0), vec3<f32>(1.0));
    // Avoid pow with non-positive base by clamping above 0
    let corrected = pow(max(gained, vec3<f32>(0.0001)), vec3<f32>(1.0 / max(params.gamma, 0.0001)));
    return vec4<f32>(clamp(corrected, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
}
