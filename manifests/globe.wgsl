fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let aspect = uniforms.resolution.x / uniforms.resolution.y;
    let scale = uniforms.custom[0].x; // from manifest uniforms: [0.35]
    
    // Center and correct aspect ratio
    let p = (uv * 2.0 - 1.0) * vec2<f32>(aspect, 1.0) / scale;
    
    let r2 = dot(p, p);
    
    // Outside the sphere
    if (r2 > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    
    // Ray-sphere intersection (orthographic)
    let z = sqrt(1.0 - r2);
    let normal = vec3<f32>(p.x, p.y, z);
    
    // Rotation
    let angle = uniforms.time * 2.5;
    let s = sin(angle);
    let c = cos(angle);
    let rotated_normal = vec3<f32>(
        normal.x * c + normal.z * s,
        normal.y,
        -normal.x * s + normal.z * c
    );
    
    // Longitude/Latitude lines
    let lon = atan2(rotated_normal.z, rotated_normal.x);
    let lat = asin(rotated_normal.y);
    
    let lon_line = abs(sin(lon * 10.0));
    let lat_line = abs(sin(lat * 10.0));
    
    var color = 0.3;
    if (lon_line < 0.1 || lat_line < 0.1) {
        color = 1.0;
    }
    
    // Lighting
    let light_dir = normalize(vec3<f32>(0.5, 0.5, 1.0));
    let light = max(dot(normal, light_dir), 0.0);
    let final_color = color * (light + 0.2); // Added more ambient light
    
    return vec4<f32>(vec3<f32>(final_color), 1.0);
}
