// Material: Wobble Effect
// Inspired by drei's MeshWobbleMaterial
// Creates sine wave-based vertex displacement for wobble animation
//
// Uses built-in VCR uniforms only (no custom parameters needed)

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    // Fixed parameters (can be adjusted by editing shader)
    let factor = 0.5;  // wobble strength
    let speed = 1.0;   // animation speed
    
    // Create wobble effect based on UV position
    let wobble_freq = 10.0;
    let theta = sin(time * speed + uv.y * wobble_freq) * factor;
    
    // Apply rotation to create wobble
    let c = cos(theta);
    let s = sin(theta);
    
    // Rotate UV coordinates
    let centered_uv = uv - 0.5;
    let rotated_uv = vec2<f32>(
        centered_uv.x * c - centered_uv.y * s,
        centered_uv.x * s + centered_uv.y * c
    ) + 0.5;
    
    // Create a simple gradient sphere for demonstration
    let aspect = resolution.x / resolution.y;
    let p = (rotated_uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dist = length(p);
    
    // Sphere with wobble
    if (dist < 0.4) {
        // Calculate normal for lighting
        let normal = normalize(vec3<f32>(p.x, p.y, sqrt(max(0.0, 0.16 - dist * dist))));
        let light_dir = normalize(vec3<f32>(0.5, 0.5, 1.0));
        
        let diffuse = max(dot(normal, light_dir), 0.0);
        let rim = pow(1.0 - abs(normal.z), 2.0);
        
        // Vibrant color gradient
        let color_a = vec3<f32>(1.0, 0.3, 0.8); // Pink
        let color_b = vec3<f32>(0.3, 0.8, 1.0); // Cyan
        let base_color = mix(color_a, color_b, rotated_uv.y);
        
        var final_color = base_color * (diffuse * 0.7 + 0.3);
        final_color += vec3<f32>(1.0, 1.0, 1.0) * rim * 0.5;
        
        return vec4<f32>(final_color, 1.0);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
