// Dreamcore Spinning Pyramid (Alpha Transparent) V3
// Optimized for 9:16 Aspect Ratio

fn rotate(p: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
    var p_out = p;
    let cy = cos(r.y); let sy = sin(r.y);
    p_out = vec3<f32>(p_out.x * cy - p_out.z * sy, p_out.y, p_out.x * sy + p_out.z * cy);
    let cx = cos(r.x); let sx = sin(r.x);
    p_out = vec3<f32>(p_out.x, p_out.y * cx - p_out.z * sx, p_out.y * sx + p_out.z * cx);
    let cz = cos(r.z); let sz = sin(r.z);
    p_out = vec3<f32>(p_out.x * cz - p_out.y * sz, p_out.x * sz + p_out.y * cz, p_out.z);
    return p_out;
}

fn sdPyramid(p_input: vec3<f32>, h: f32) -> f32 {
    var p = p_input;
    let m2 = h*h + 0.25;
    
    p.x = abs(p.x);
    p.z = abs(p.z);
    if (p.z > p.x) {
        let tmp = p.x; p.x = p.z; p.z = tmp;
    }
    p.x -= 0.5;
    p.z -= 0.5;

    let q = vec3<f32>(p.z, h*p.y - 0.5*p.x, h*p.x + 0.5*p.y);
    
    let s = max(-q.x, 0.0);
    let t = clamp((q.y - 0.5*p.z) / (m2 + 0.25), 0.0, 1.0);
    
    let a = q.x + s;
    let b = q.y - t*0.5;
    let c = q.z - t*h;
    
    let d2 = min(q.y*q.y + q.z*q.z, a*a + b*b + c*c);
    
    return sqrt(d2) * sign(max(q.y, -q.x));
}

fn map(p: vec3<f32>, time: f32) -> f32 {
    let scale = 0.9;
    let p_rot = rotate(p, vec3<f32>(0.2, time * 0.8, -0.1)) / scale;
    // Divide the result by scale to maintain correct distance field
    return sdPyramid(p_rot, 1.3) * scale;
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
    let p_ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Better framing: lowered and slightly back
    let ro = vec3<f32>(0.0, 0.2, -3.0);
    let rd = normalize(vec3<f32>(p_ndc, 2.0));
    
    var t = 0.0;
    var hit = false;
    var dist = 0.0;
    
    for (var i = 0; i < 90; i++) {
        dist = map(ro + rd * t, time);
        if (abs(dist) < 0.0005) {
            hit = true;
            break;
        }
        t += dist;
        if (t > 10.0) { break; }
    }
    
    if (hit) {
        let pos = ro + rd * t;
        let normal = getNormal(pos, time);
        let light_dir = normalize(vec3<f32>(1.0, 2.0, -1.5));
        
        let diff = max(dot(normal, light_dir), 0.0);
        let rim = pow(1.0 - max(dot(normal, -rd), 0.0), 4.0);
        
        // Vibrant Dreamcore Gradient
        let color_top = vec3<f32>(1.0, 0.2, 0.9); // Hot Pink
        let color_bottom = vec3<f32>(0.2, 0.9, 1.0); // Cyan
        let base_color = mix(color_bottom, color_top, normal.y * 0.5 + 0.5);
        
        var final_color = base_color * (diff * 0.7 + 0.3);
        final_color += vec3<f32>(1.0, 1.0, 1.0) * rim * 0.8;
        
        // Glossy highlights
        let spec = pow(max(dot(reflect(-light_dir, normal), -rd), 0.0), 64.0);
        final_color += vec3<f32>(1.0, 1.0, 1.0) * spec * 0.4;
        
        return vec4<f32>(final_color, 0.98);
    }
    
    // Background shadow/ground pulse
    let d_dist = length(p_ndc - vec2<f32>(0.0, 0.4));
    let pulse_ring = smoothstep(0.5, 0.48, d_dist) * smoothstep(0.4, 0.42, d_dist);
    if (pulse_ring > 0.0) {
        let pulse_alpha = pulse_ring * 0.05 * (sin(time * 2.0) * 0.5 + 0.5);
        return vec4<f32>(0.5, 0.8, 1.0, pulse_alpha);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
