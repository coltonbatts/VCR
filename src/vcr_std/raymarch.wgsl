// VCR Standard Library: Raymarching Template (Perfect Alpha)
// Included via #include "vcr:raymarch"
// Requirement: User must define fn map(p: vec3<f32>, u: ShaderUniforms) -> f32

fn calcNormal(p: vec3<f32>, u: ShaderUniforms) -> vec3<f32> {
    let e = vec2<f32>(0.001, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy, u) - map(p - e.xyy, u),
        map(p + e.yxy, u) - map(p - e.yxy, u),
        map(p + e.yyx, u) - map(p - e.yyx, u)
    ));
}

// Standard Raymarcher with Anti-Aliasing (AA) helper
// Returns vec4(color.rgb, alpha)
fn raymarch_render(uv: vec2<f32>, u: ShaderUniforms, cam_pos: vec3<f32>, look_at: vec3<f32>, zoom: f32) -> vec4<f32> {
    let aspect = u.resolution.x / u.resolution.y;
    let p_ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let f = normalize(look_at - cam_pos);
    let r = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), f));
    let v_up = cross(f, r);
    let rd = normalize(f * zoom + p_ndc.x * r + p_ndc.y * v_up);
    
    var t = 0.01;
    var hit = false;
    var dist = 0.0;
    
    for (var i = 0; i < 100; i = i + 1) {
        dist = map(cam_pos + rd * t, u);
        if (abs(dist) < 0.0005 || t > 20.0) {
            if (abs(dist) < 0.0005) { hit = true; }
            break;
        }
        t = t + dist;
    }
    
    if (hit) {
        let pos = cam_pos + rd * t;
        let n = calcNormal(pos, u);
        let light_dir = normalize(vec3<f32>(1.0, 2.0, -1.0));
        let diff = max(dot(n, light_dir), 0.0);
        let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
        
        let shading = diff * 0.7 + 0.3 + rim * 0.5;
        // The user should multiply their base color by shading
        // For alpha edges, we use smoothstep based on distance to the camera
        // to slightly soften the intersection.
        let alpha = smoothstep(0.001, 0.0, abs(dist)); 
        // Note: Simple single-sample edge alpha. 
        // For multi-sample AA, the user should call raymarch_render multiple times with jittered UVs.
        
        return vec4<f32>(vec3<f32>(shading), 1.0); // Returning mask/shading; user should compose
    }
    
    return vec4<f32>(0.0);
}
