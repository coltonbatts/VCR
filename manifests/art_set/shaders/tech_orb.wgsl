// Tech Pulse Orb
// Sphere with pulsing core and outer shells on alpha

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let dist = length(p);
    
    // Core
    let core_pulse = 0.1 + 0.02 * sin(time * 10.0);
    let core = smoothstep(core_pulse, core_pulse - 0.01, dist);
    
    // Outer shells
    let shell1 = smoothstep(0.25, 0.24, dist) * smoothstep(0.23, 0.24, dist);
    let shell2 = smoothstep(0.35, 0.34, dist) * smoothstep(0.33, 0.34, dist) * step(0.5, sin(atan2(p.y, p.x) * 5.0 + time));
    
    var color = vec3<f32>(1.0, 1.0, 1.0) * core;
    color += vec3<f32>(0.0, 0.8, 1.0) * shell1;
    color += vec3<f32>(0.5, 0.2, 1.0) * shell2;
    
    let alpha = max(core, max(shell1, shell2));
    let glow = exp(-dist * 5.0) * 0.2;
    
    return vec4<f32>(color + vec3<f32>(0.2, 0.4, 1.0) * glow, max(alpha, glow));
}
