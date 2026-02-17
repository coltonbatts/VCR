// Organic Glitch Bloom
// Abstract blooming shape with pixelation on alpha

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    var p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let angle = atan2(p.y, p.x);
    let dist = length(p);
    
    // Blooming deformation
    let bloom = 0.2 + 0.1 * sin(angle * 5.0 + time * 2.0) * sin(time);
    let pixel_dist = floor(dist * 40.0) / 40.0;
    
    let mask = smoothstep(bloom + 0.01, bloom, pixel_dist);
    let glitch = step(0.95, hash(vec2<f32>(floor(time * 10.0), pixel_dist)));
    
    let color = mix(vec3<f32>(0.8, 0.2, 0.6), vec3<f32>(1.0, 0.9, 0.0), mask * (1.0 - glitch));
    let alpha = mask * (0.8 + glitch * 0.2);
    
    return vec4<f32>(color * alpha, alpha);
}
