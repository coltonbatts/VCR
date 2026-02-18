// Dynamic Moiré Sphere Illusion
// Uses overlapping rotating patterns to create liquid-like interference

fn rotate(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let s = sin(angle);
    let c = cos(angle);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

fn grid_pattern(p: vec2<f32>, size: f32) -> f32 {
    let g = abs(fract(p * size - 0.5) - 0.5);
    return smoothstep(0.01, 0.05, min(g.x, g.y));
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Create a sphere mask
    let radius = 0.4;
    let dist = length(p);
    let sphere_mask = smoothstep(radius, radius - 0.01, dist);
    
    if (sphere_mask < 0.001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    
    // Project UVs onto the sphere for more 3D feel
    let sphere_uv = p / radius;
    let z = sqrt(max(0.0, 1.0 - dot(sphere_uv, sphere_uv)));
    let p_3d = vec3<f32>(sphere_uv, z);
    
    // Two layers of rotating grids
    let p1 = rotate(sphere_uv, time * 0.2);
    let p2 = rotate(sphere_uv, -time * 0.3);
    
    let layer1 = grid_pattern(p1, 20.0);
    let layer2 = grid_pattern(p2, 22.0); // Slightly different scale for interference
    
    // Moiré interference
    let interference = layer1 * layer2;
    
    // Cyber blue/purple palette
    let color1 = vec3<f32>(0.1, 0.8, 1.0); // Cyan
    let color2 = vec3<f32>(0.8, 0.2, 1.0); // Purple
    
    var final_color = mix(color1, color2, sin(time * 0.5 + dist * 5.0) * 0.5 + 0.5);
    final_color *= interference;
    
    // Add some "sphere lighting"
    let lighting = dot(normalize(p_3d), normalize(vec3<f32>(1.0, 1.0, 1.0))) * 0.5 + 0.5;
    final_color += (1.0 - interference) * 0.1 * lighting; // Subtle glow in the gaps
    
    return vec4<f32>(final_color, sphere_mask);
}
