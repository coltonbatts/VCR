fn hash(p: vec3<f32>) -> f32 {
    let p3 = fract(p * 0.1031);
    let p3_2 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3_2.x + p3_2.y) * p3_2.z);
}

fn rotation_matrix(axis: vec3<f32>, angle: f32) -> mat3x3<f32> {
    let s = sin(angle);
    let c = cos(angle);
    let oc = 1.0 - c;
    return mat3x3<f32>(
        oc * axis.x * axis.x + c,           oc * axis.x * axis.y - axis.z * s,  oc * axis.z * axis.x + axis.y * s,
        oc * axis.x * axis.y + axis.z * s,  oc * axis.y * axis.y + c,           oc * axis.y * axis.z - axis.x * s,
        oc * axis.z * axis.x - axis.y * s,  oc * axis.y * axis.z + axis.x * s,  oc * axis.z * axis.z + c
    );
}

fn sd_sphere(p: vec3<f32>, r: f32) -> f32 {
    return length(p) - r;
}

fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    let rot = rotation_matrix(vec3<f32>(0.0, 1.0, 0.0), u.time * 0.01);
    let p_rot = rot * p;
    
    var d = sd_sphere(p_rot, 1.0);
    
    // Add some "neural" surface complexity
    let d2 = sin(p_rot.x * 10.0 + u.time * 0.05) * sin(p_rot.y * 10.0) * sin(p_rot.z * 10.0) * 0.05;
    return d + d2;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let aspect = u.resolution.x / u.resolution.y;
    let p = (uv * 2.0 - 1.0) * vec2<f32>(aspect, 1.0);
    
    let ro = vec3<f32>(0.0, 0.0, -3.0);
    let rd = normalize(vec3<f32>(p, 2.0));
    
    var t = 0.0;
    var d = 0.0;
    for (var i = 0; i < 64; i++) {
        let pos = ro + rd * t;
        d = map(pos, u);
        if (d < 0.001 || t > 10.0) { break; }
        t += d;
    }
    
    var col = vec3<f32>(0.0);
    
    if (t < 10.0) {
        let pos = ro + rd * t;
        let rot = rotation_matrix(vec3<f32>(0.0, 1.0, 0.0), u.time * 0.01);
        let p_rot = rot * pos;
        
        // Calculate normals
        let eps = 0.001;
        let n = normalize(vec3<f32>(
            map(pos + vec3<f32>(eps, 0.0, 0.0), u) - map(pos - vec3<f32>(eps, 0.0, 0.0), u),
            map(pos + vec3<f32>(0.0, eps, 0.0), u) - map(pos - vec3<f32>(0.0, eps, 0.0), u),
            map(pos + vec3<f32>(0.0, 0.0, eps), u) - map(pos - vec3<f32>(0.0, 0.0, eps), u)
        ));
        
        // Fresnell-ish glow
        let fresnel = pow(1.0 - max(dot(-rd, n), 0.0), 3.0);
        
        // Neural web pattern on surface
        let web = sin(p_rot.x * 20.0) * sin(p_rot.y * 20.0) * sin(p_rot.z * 20.0);
        let web_mask = smoothstep(0.8, 0.9, web);
        
        let accent_col = vec3<f32>(0.2, 0.6, 1.0); // Cyan/Blue AI vibe
        col = accent_col * fresnel * 2.0;
        col += accent_col * web_mask * 1.5;
        
        // Add flickering nodes
        let node_id = floor(p_rot * 5.0);
        let flicker = hash(node_id + vec3<f32>(floor(u.time * 0.1))) * step(0.95, hash(node_id));
        col += vec3<f32>(1.0) * flicker * 0.5;
        
        // Alpha based on depth/edge
        let alpha = smoothstep(0.0, 0.2, fresnel) + web_mask;
        return vec4<f32>(col, clamp(alpha, 0.0, 1.0));
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
