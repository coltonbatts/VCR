// Raymarching helpers
fn sdSphere(p: vec3<f32>, s: f32) -> f32 {
    return length(p) - s;
}

fn sdBox(p: vec3<f32>, b: vec3<f32>) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
}

fn sdCylinder(p: vec3<f32>, h: f32, r: f32) -> f32 {
    let d = abs(vec2<f32>(length(p.xz), p.y)) - vec2<f32>(r, h);
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0)));
}

fn opSmoothUnion(d1: f32, d2: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (d2 - d1) / k, 0.0, 1.0);
    return mix(d2, d1, h) - k * h * (1.0 - h);
}

fn rotateY(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

fn map(p: vec3<f32>, time: f32) -> f32 {
    let p_rot = rotateY(p, time * 0.8);
    
    // Statue assembly
    let head = sdSphere(p_rot - vec3<f32>(0.0, 0.4, 0.0), 0.3);
    let neck = sdCylinder(p_rot - vec3<f32>(0.0, 0.05, 0.0), 0.1, 0.08);
    let shoulders = sdBox(p_rot - vec3<f32>(0.0, -0.15, 0.0), vec3<f32>(0.4, 0.1, 0.2));
    let base = sdBox(p_rot - vec3<f32>(0.0, -0.6, 0.0), vec3<f32>(0.3, 0.1, 0.3));
    
    var d = opSmoothUnion(head, neck, 0.1);
    d = opSmoothUnion(d, shoulders, 0.15);
    d = opSmoothUnion(d, base, 0.05);
    
    return d;
}

fn getNormal(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = vec2<f32>(0.001, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy, time) - map(p - e.xyy, time),
        map(p + e.yxy, time) - map(p - e.yxy, time),
        map(p + e.yyx, time) - map(p - e.yyx, time)
    ));
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Camera — statue in upper 65% of frame
    let ro = vec3<f32>(0.0, 0.0, -5.0);
    let rd = normalize(vec3<f32>(p + vec2<f32>(0.0, 0.20), 1.5));
    // Hard cutoff: nothing renders below 65% of frame
    if (uv.y > 0.65) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }
    
    // Raymarching
    var t = 0.0;
    var hit = false;
    for (var i = 0; i < 80; i++) {
        let h = map(ro + rd * t, time);
        if (h < 0.001) {
            hit = true;
            break;
        }
        t += h;
        if (t > 10.0) { break; }
    }
    
    if (hit) {
        let pos = ro + rd * t;
        let n = getNormal(pos, time);
        let light_dir = normalize(vec3<f32>(1.0, 2.0, -2.0));
        let diff = max(dot(n, light_dir), 0.0);
        
        // Dreamcore palette
        let color_base = vec3<f32>(0.9, 0.85, 1.0); // Pale marble
        let color_light = vec3<f32>(0.0, 1.0, 1.0); // Cyan rim
        let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
        
        var final_color = color_base * (diff + 0.1);
        final_color += color_light * rim * 0.5;
        
        // Scanlines/Dither
        let scanline = 0.9 + 0.1 * sin(uv.y * resolution.y * 2.0);
        final_color *= scanline;
        
        // Alpha falloff / Fog
        let alpha = smoothstep(1.2, 0.5, length(p));
        
        return vec4<f32>(final_color, alpha);
    }
    
    // Background void
    let bg_alpha = 0.0;
    return vec4<f32>(0.0, 0.0, 0.0, bg_alpha);
}
