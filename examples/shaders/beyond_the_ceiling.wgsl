fn hash(p: vec3<f32>) -> f32 {
    let p3 = fract(p * 0.1031);
    let p3_2 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3_2.x + p3_2.y) * p3_2.z);
}

fn rot2(a: f32) -> mat2x2<f32> {
    let s = sin(a);
    let c = cos(a);
    return mat2x2<f32>(c, -s, s, c);
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

fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    var q = p;
    let time = u.time * 0.05;
    
    // Higher-detail "Neural Core" (Single Object)
    let p_rot = rotation_matrix(vec3<f32>(0.2, 1.0, 0.3), u.time * 0.05) * p;
    
    // Base sphere
    var d = length(p_rot) - 1.0;
    
    // High-frequency "Neural net" displacement
    let noise = sin(p_rot.x * 8.0 + u.time * 0.1) * sin(p_rot.y * 10.0) * sin(p_rot.z * 12.0) * 0.1;
    let detail = sin(p_rot.x * 40.0) * sin(p_rot.y * 45.0) * sin(p_rot.z * 38.0) * 0.02;
    
    return d + noise + detail;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let aspect = u.resolution.x / u.resolution.y;
    var p = (uv * 2.0 - 1.0) * vec2<f32>(aspect, 1.0);
    
    // Spectral Glitch
    let glitch = hash(vec3<f32>(floor(u.time * 20.0))) * 0.002 * step(0.99, hash(vec3<f32>(floor(u.time * 5.0))));
    p.x += glitch;
    
    let ro = vec3<f32>(0.0, 0.0, -3.8);
    let rd = normalize(vec3<f32>(p, 2.5));
    
    var t = 0.0;
    var d = 0.0;
    var glow = 0.0;
    
    // Volumetric glow calculation
    for (var i = 0; i < 48; i++) {
        let pos = ro + rd * t;
        d = map(pos, u);
        glow += exp(-d * 6.0) * 0.02;
        if (d < 0.001 || t > 10.0) { break; }
        t += d;
    }
    
    var col = vec3<f32>(0.0);
    
    // Deep Purple Flare
    let purple_flare = vec3<f32>(0.4, 0.1, 0.8) * glow;
    col += purple_flare;
    
    if (t < 10.0) {
        let pos = ro + rd * t;
        let eps = 0.001;
        let n = normalize(vec3<f32>(
            map(pos + vec3<f32>(eps, 0.0, 0.0), u) - map(pos - vec3<f32>(eps, 0.0, 0.0), u),
            map(pos + vec3<f32>(0.0, eps, 0.0), u) - map(pos - vec3<f32>(0.0, eps, 0.0), u),
            map(pos + vec3<f32>(0.0, 0.0, eps), u) - map(pos - vec3<f32>(0.0, 0.0, eps), u)
        ));
        
        let fresnel = pow(1.0 - max(dot(-rd, n), 0.0), 4.0);
        let diffuse = max(dot(n, normalize(vec3<f32>(1.0, 1.0, -1.0))), 0.0);
        
        // Crystalline material
        let crystal_base = vec3<f32>(0.7, 0.8, 1.0) * (diffuse * 0.6 + 0.1);
        col += crystal_base;
        col += vec3<f32>(0.5, 0.2, 1.0) * fresnel * 2.0;
        
        return vec4<f32>(col, 1.0);
    }
    
    return vec4<f32>(col, clamp(length(col), 0.0, 1.0));
}
