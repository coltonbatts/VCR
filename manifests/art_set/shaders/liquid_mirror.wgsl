// Digital Liquid Mirror
// Planar water ripples with tech glitches on alpha

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let dist = length(p);
    // Multiple ripples
    var ripple = 0.0;
    ripple += sin(dist * 30.0 - time * 5.0) * 0.1;
    ripple += sin(length(p - vec2<f32>(0.2, 0.1)) * 20.0 - time * 3.0) * 0.05;
    
    // Tech glitch
    let glitch_line = step(0.98, hash(vec2<f32>(floor(uv.y * 100.0), floor(time * 15.0))));
    let glitch_offset = glitch_line * 0.05 * (hash(vec2<f32>(time, 0.0)) - 0.5);
    
    let mask = smoothstep(0.4, 0.38, dist);
    let color = mix(vec3<f32>(0.1, 0.4, 0.8), vec3<f32>(1.0, 1.0, 1.0), smoothstep(0.0, 0.1, ripple + glitch_offset));
    let alpha = mask * (0.3 + glitch_line * 0.5);
    
    return vec4<f32>(color * alpha, alpha);
}
