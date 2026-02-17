// Data Vortex
// Swirly vortex of square particles on alpha

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let dist = length(p);
    let angle = atan2(p.y, p.x) + dist * 5.0 - time * 2.0;
    
    var color = vec3<f32>(0.0);
    var alpha = 0.0;
    
    for(var i = 0; i < 5; i++) {
        let r = 0.1 * f32(i + 1);
        let spiral_p = vec2<f32>(cos(angle + f32(i)), sin(angle + f32(i))) * r;
        let d = length(p - spiral_p);
        
        let particle = smoothstep(0.02, 0.015, d);
        let p_col = mix(vec3<f32>(0.2, 0.6, 1.0), vec3<f32>(1.0, 1.0, 1.0), hash(vec2<f32>(f32(i), time)));
        color += p_col * particle;
        alpha = max(alpha, particle);
    }
    
    alpha = max(alpha, exp(-dist * 8.0) * 0.2);
    color += vec3<f32>(0.0, 0.3, 0.7) * alpha;
    
    return vec4<f32>(color, alpha);
}
