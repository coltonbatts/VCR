// Fixed Penrose Triangle Illusion
// Uses precise mathematical alignment for the "impossible" connection

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

fn sdBox(p: vec3<f32>, b: vec3<f32>) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);
}

fn map(p: vec3<f32>, time: f32) -> f32 {
    // The "Magic" Angles for isometric-style Penrose Triangle
    // 35.26 degrees (0.615 rad) and 45 degrees (0.785 rad)
    // We oscillation slightly to show it's 3D, then snap to alignment
    let snap = smoothstep(0.3, 0.7, abs(sin(time * 0.5) * 2.0 - 1.0));
    let drift = (sin(time * 0.5) * 0.4) * (1.0 - snap); 
    
    let p_rot = rotate(p, vec3<f32>(0.6154 + drift, 0.7853, 0.0));
    
    let L = 1.2; // Bar length
    let T = 0.2; // Thickness
    
    // Bar 1 (Bottom)
    let d1 = sdBox(p_rot - vec3<f32>(0.0, -L, 0.0), vec3<f32>(L + T, T, T));
    
    // Bar 2 (Left)
    let d2 = sdBox(p_rot - vec3<f32>(-L, 0.0, 0.0), vec3<f32>(T, L + T, T));
    
    // Bar 3 (The gap-closer)
    // This bar is rotated and shifted to connect the open ends in 2D projection
    let p3 = p_rot - vec3<f32>(0.0, 0.0, -L);
    let d3 = sdBox(p3, vec3<f32>(T, T, L + T));
    
    return min(d1, min(d2, d3));
}

fn getNormal(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = vec2<f32>(0.001, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy, time) - map(p - e.xyy, time),
        map(p + e.yxy, time) - map(p - e.yxy, time),
        map(p + e.yyx, time) - map(p - e.yyx, time)
    ));
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, clamp(p - k.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p_ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Use an extremely high focal length to simulate orthographic projection
    // This is critical for the Penrose illusion to work
    let ro = vec3<f32>(0.0, 0.0, -20.0);
    let rd = normalize(vec3<f32>(p_ndc, 15.0)); 
    
    var t = 0.0;
    var hit = false;
    var dist = 0.0;
    
    for (var i = 0; i < 100; i++) {
        dist = map(ro + rd * t, time);
        if (abs(dist) < 0.001) {
            hit = true;
            break;
        }
        t += dist;
        if (t > 30.0) { break; }
    }
    
    if (hit) {
        let pos = ro + rd * t;
        let normal = getNormal(pos, time);
        let light_dir = normalize(vec3<f32>(1.0, 3.0, -2.0));
        
        let diff = max(dot(normal, light_dir), 0.0);
        let rim = pow(1.0 - max(dot(normal, -rd), 0.0), 3.0);
        
        let hue = fract(time * 0.05 + pos.y * 0.1);
        let base_color = hsv2rgb(vec3<f32>(hue, 0.7, 0.9));
        
        var final_color = base_color * (diff * 0.6 + 0.4);
        final_color += rim * 0.4;
        
        // Scanlines for high-tech feel
        let scan = 0.95 + 0.05 * sin(uv.y * 1000.0);
        final_color *= scan;
        
        return vec4<f32>(final_color, 1.0);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
