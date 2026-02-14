// Material: Distort Effect
// Inspired by drei's MeshDistortMaterial
// Uses simplified noise for organic vertex displacement
//
// Uses built-in VCR uniforms only (no custom parameters needed)
// Note: Uses simplified noise due to WGSL array indexing limitations

// Simplified hash-based noise (no lookup tables)
fn hash(p: vec3<f32>) -> f32 {
    var p3 = fract(p * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn noise_3d(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    
    // Cubic interpolation
    let u = f * f * (3.0 - 2.0 * f);
    
    // Sample 8 corners of cube
    let n000 = hash(i + vec3<f32>(0.0, 0.0, 0.0));
    let n100 = hash(i + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash(i + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash(i + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash(i + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash(i + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash(i + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash(i + vec3<f32>(1.0, 1.0, 1.0));
    
    // Trilinear interpolation
    let nx00 = mix(n000, n100, u.x);
    let nx10 = mix(n010, n110, u.x);
    let nx01 = mix(n001, n101, u.x);
    let nx11 = mix(n011, n111, u.x);
    
    let nxy0 = mix(nx00, nx10, u.y);
    let nxy1 = mix(nx01, nx11, u.y);
    
    return mix(nxy0, nxy1, u.z);
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    // Fixed parameters (can be adjusted by editing shader)
    let distort = 0.3;   // distortion strength
    let radius = 2.0;    // noise scale
    
    // Create 3D noise coordinates
    let p3d = vec3<f32>(uv * radius, time * 0.3);
    let noise = noise_3d(p3d) * 2.0 - 1.0; // Remap to [-1, 1]
    
    // Apply noise-based distortion to UV
    let distorted_uv = uv + vec2<f32>(noise * distort * 0.1);
    
    // Create a distorted sphere
    let aspect = resolution.x / resolution.y;
    let p = (distorted_uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dist = length(p);
    
    // Distorted sphere with organic movement
    let noise_offset = noise * distort * 0.1;
    if (dist < 0.4 + noise_offset) {
        // Calculate normal with noise influence
        let normal = normalize(vec3<f32>(p.x, p.y, sqrt(max(0.0, 0.16 - dist * dist))));
        let light_dir = normalize(vec3<f32>(0.5, 0.5, 1.0));
        
        let diffuse = max(dot(normal, light_dir), 0.0);
        let rim = pow(1.0 - abs(normal.z), 2.0);
        
        // Color gradient influenced by noise
        let color_a = vec3<f32>(0.8, 0.3, 1.0); // Purple
        let color_b = vec3<f32>(0.3, 1.0, 0.8); // Teal
        let base_color = mix(color_a, color_b, noise * 0.5 + 0.5);
        
        var final_color = base_color * (diffuse * 0.7 + 0.3);
        final_color += vec3<f32>(1.0, 1.0, 1.0) * rim * 0.5;
        
        return vec4<f32>(final_color, 1.0);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
