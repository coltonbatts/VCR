// Floating Geometric Cluster
// Orbiting spheres with pulsing colors on alpha

fn hash3(p: vec3<f32>) -> vec3<f32> {
    var q = vec3<f32>(
        dot(p, vec3<f32>(127.1, 311.7, 74.7)),
        dot(p, vec3<f32>(269.5, 183.3, 246.1)),
        dot(p, vec3<f32>(113.5, 271.9, 124.6))
    );
    return fract(sin(q) * 43758.5453123);
}

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    var color = vec3<f32>(0.0);
    var alpha = 0.0;
    
    for(var i = 0; i < 8; i++) {
        let h = hash3(vec3<f32>(f32(i), 123.4, 567.8));
        let angle = time * (h.x * 2.0 - 1.0) + h.y * 6.28;
        let radius = 0.1 + h.z * 0.25;
        let pos = vec2<f32>(cos(angle), sin(angle)) * radius;
        
        let d = length(p - pos);
        let s_radius = 0.02 + sin(time * 2.0 + h.x * 10.0) * 0.01;
        let circle = smoothstep(s_radius, s_radius - 0.005, d);
        
        let c_col = mix(vec3<f32>(1.0, 0.2, 0.5), vec3<f32>(0.2, 0.5, 1.0), h.y);
        color += c_col * circle;
        alpha = max(alpha, circle);
    }
    
    // Ambient glow
    let glow = exp(-length(p) * 4.0) * 0.1;
    color += vec3<f32>(0.4, 0.3, 0.8) * glow;
    alpha = max(alpha, glow);
    
    return vec4<f32>(color, alpha);
}
