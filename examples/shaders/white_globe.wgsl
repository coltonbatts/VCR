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
    let rot = rotation_matrix(vec3<f32>(0.1, 1.0, 0.2), u.time * 0.02);
    let p_rot = rot * p;
    
    var d = sd_sphere(p_rot, 1.0);
    
    // Smooth grid lines/nodes pattern
    let d2 = sin(p_rot.x * 12.0) * sin(p_rot.y * 12.0) * sin(p_rot.z * 12.0) * 0.02;
    return d + d2;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let aspect = u.resolution.x / u.resolution.y;
    let p = (uv * 2.0 - 1.0) * vec2<f32>(aspect, 1.0);
    
    let ro = vec3<f32>(0.0, 0.0, -2.8);
    let rd = normalize(vec3<f32>(p, 2.2));
    
    var t = 0.0;
    var d = 0.0;
    for (var i = 0; i < 48; i++) {
        let pos = ro + rd * t;
        d = map(pos, u);
        if (d < 0.001 || t > 8.0) { break; }
        t += d;
    }
    
    var col = vec3<f32>(0.0);
    
    if (t < 8.0) {
        let pos = ro + rd * t;
        let rot = rotation_matrix(vec3<f32>(0.1, 1.0, 0.2), u.time * 0.02);
        let p_rot = rot * pos;
        
        let eps = 0.002;
        let n = normalize(vec3<f32>(
            map(pos + vec3<f32>(eps, 0.0, 0.0), u) - map(pos - vec3<f32>(eps, 0.0, 0.0), u),
            map(pos + vec3<f32>(0.0, eps, 0.0), u) - map(pos - vec3<f32>(0.0, eps, 0.0), u),
            map(pos + vec3<f32>(0.0, 0.0, eps), u) - map(pos - vec3<f32>(0.0, 0.0, eps), u)
        ));
        
        let fresnel = pow(1.0 - max(dot(-rd, n), 0.0), 2.5);
        let dot_pattern = smoothstep(0.85, 0.9, sin(p_rot.x * 24.0) * sin(p_rot.y * 24.0) * sin(p_rot.z * 24.0));
        
        // Solid white glowing vibe
        col = vec3<f32>(1.0) * fresnel * 1.5;
        col += vec3<f32>(1.0) * dot_pattern * 0.8;
        
        let alpha = smoothstep(0.0, 0.1, fresnel) + dot_pattern;
        return vec4<f32>(col, clamp(alpha, 0.0, 1.0));
    }
    
    return vec4<f32>(0.0);
}
