const DEBUG_FRAMING: bool = false;
const SAFE_ZONE: f32 = 0.85; // 85% safe zone for extra clearance

fn rotate_y(p: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a); let c = cos(a);
    return vec3<f32>(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

fn rotate_x(p: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a); let c = cos(a);
    return vec3<f32>(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn sdPyramid(p: vec3<f32>, h: f32) -> f32 {
    let d2 = max(abs(p.x), abs(p.z));
    return max(-p.y, d2 + p.y - h);
}

fn map(p: vec3<f32>, time: f32) -> f32 {
    let scale = 0.22; // Small enough to fit in portrait width (0.56)
    // Center it vertically. Apex is at y=1, base at y=0.
    // So target center is (0, 0.5, 0) in local space.
    let p_offset = p - vec3<f32>(0.0, 0.0, 0.0); 
    let p_rot = rotate_x(rotate_y(p_offset / scale, time), 0.3);
    // Move slightly down so apex isn't dead center
    return sdPyramid(p_rot - vec3<f32>(0.0, -0.5, 0.0), 1.0) * scale;
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
    
    // Safety framing check (before raymarching)
    var frame_color = vec3<f32>(0.0);
    var is_debug = false;
    if (DEBUG_FRAMING) {
        let edge = 0.002;
        // Outer border (Red)
        if (uv.x < edge || uv.x > 1.0 - edge || uv.y < edge || uv.y > 1.0 - edge) {
            frame_color = vec3<f32>(1.0, 0.0, 0.0);
            is_debug = true;
        }
        // Safe zone lines (Cyan)
        let sz = (1.0 - SAFE_ZONE) * 0.5;
        if (abs(uv.x - sz) < edge*0.5 || abs(uv.x - (1.0 - sz)) < edge*0.5 || 
            abs(uv.y - sz) < edge*0.5 || abs(uv.y - (1.0 - sz)) < edge*0.5) {
            frame_color = vec3<f32>(0.0, 1.0, 1.0);
            is_debug = true;
        }
    }

    let ro = vec3<f32>(0.0, 0.0, -4.5); // Move back
    let rd = normalize(vec3<f32>(p, 2.0)); // Narrow FOV
    
    var t_dist = 0.0;
    var hit = false;
    for (var i = 0; i < 100; i++) {
        let d = map(ro + rd * t_dist, time);
        if (d < 0.001) {
            hit = true;
            break;
        }
        t_dist += d;
        if (t_dist > 10.0) { break; }
    }
    
    var out_color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (hit) {
        let pos = ro + rd * t_dist;
        let n = getNormal(pos, time);
        let light = normalize(vec3<f32>(1.0, 1.0, -1.0));
        let diff = max(dot(n, light), 0.0);
        let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
        
        let purple = vec3<f32>(0.6, 0.1, 0.9);
        let teal = vec3<f32>(0.1, 0.9, 0.7);
        
        let p_local = rotate_x(rotate_y((pos - vec3<f32>(0.0, 0.0, 0.0)) / 0.22, time), 0.3) - vec3<f32>(0.0, -0.5, 0.0);
        let pat = sin(p_local.x * 12.0) * sin(p_local.z * 12.0) + 0.5;
        
        var color = purple * (diff + 0.2);
        color = mix(color, teal, rim * 0.8);
        color += teal * pat * 0.3;
        
        let glow = 0.05 / (length(pos - vec3<f32>(0.0, 0.1, 0.0)) + 0.01);
        color += teal * glow;
        
        out_color = vec4<f32>(color, 1.0);
    } else {
        out_color = vec4<f32>(0.01, 0.0, 0.02, 0.0);
    }
    
    if (is_debug) {
        return vec4<f32>(mix(out_color.rgb, frame_color, 0.8), 1.0);
    }
    return out_color;
}
