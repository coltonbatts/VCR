// Prismatic Ring
// Glass ring breaking light into spectrum on alpha

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let dist = length(p);
    let inner = 0.25;
    let outer = 0.3;
    
    let ring = smoothstep(inner, inner + 0.01, dist) * smoothstep(outer, outer - 0.01, dist);
    
    var color = vec3<f32>(0.0);
    if (ring > 0.0) {
        // Spectral shift
        let angle = atan2(p.y, p.x) + time;
        color.r = 0.5 + 0.5 * sin(angle);
        color.g = 0.5 + 0.5 * sin(angle + 2.0);
        color.b = 0.5 + 0.5 * sin(angle + 4.0);
        
        // Highlights
        let spec = pow(max(0.0, sin(angle * 3.0 - time * 2.0)), 10.0);
        color += spec;
    }
    
    return vec4<f32>(color * ring, ring);
}
