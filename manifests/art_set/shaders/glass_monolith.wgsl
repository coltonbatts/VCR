// Glass Brutalist Monolith
// Rotating slab with refraction and Fresnel glow on alpha

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Rotation
    let angle = time * 0.4;
    let s = sin(angle);
    let c = cos(angle);
    let p_rot = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
    
    // 3D-ish slab projection
    // Simple raymarching for a box
    var ro = vec3<f32>(0.0, 0.0, -2.0);
    var rd = normalize(vec3<f32>(p, 1.5));
    
    // Rotate camera/box
    let ry = time * 0.5;
    let rx = time * 0.2;
    // ... basic cube SDF projection ...
    
    // Geometry: Box 0.3 x 0.5 x 0.1
    let size = vec3<f32>(0.2, 0.4, 0.05);
    
    var d = 100.0;
    var t = 0.0;
    for(var i = 0; i < 40; i++) {
        let pos = ro + rd * t;
        // Rotate pos
        var rp = pos;
        let s1 = sin(ry); let c1 = cos(ry);
        rp = vec3<f32>(rp.x * c1 - rp.z * s1, rp.y, rp.x * s1 + rp.z * c1);
        let s2 = sin(rx); let c2 = cos(rx);
        rp = vec3<f32>(rp.x, rp.y * c2 - rp.z * s2, rp.y * s2 + rp.z * c2);
        
        let q = abs(rp) - size;
        let dist = length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
        
        if (dist < 0.001) { break; }
        t += dist;
        if (t > 5.0) { break; }
    }
    
    if (t < 5.0) {
        let pos = ro + rd * t;
        // Normal estimate
        var n = vec3<f32>(0.0);
        // ... simplified normals for speed ...
        let fresnel = 1.0 - max(0.0, dot(-rd, vec3<f32>(0.0, 0.0, -1.0))); // very fake fresnel
        
        let col = mix(vec3<f32>(0.1, 0.1, 0.12), vec3<f32>(0.8, 0.9, 1.0), pow(fresnel, 4.0));
        let alpha = smoothstep(5.0, 4.9, t);
        return vec4<f32>(col, alpha);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
