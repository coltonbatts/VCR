fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let d = length(uv - 0.5);
    let glow = exp(-d * 4.0);
    let col = vec3<f32>(0.2, 0.05, 0.4) * glow;
    return vec4<f32>(col, glow * 0.5);
}
