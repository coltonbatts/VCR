// Final Bulletproof Torus Knot
// PHYSICALLY constructs the knot from 128 connected segments
// Corrected winding math and massive vertical safety margin

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

fn sdCapsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

// Corrected Torus Knot Parametric Path
fn getKnotPoint(t: f32, p: f32, q: f32, R: f32, r: f32) -> vec3<f32> {
    // Both angles must complete their respective windings
    let phi = t * 6.2831853 * q;
    let theta = t * 6.2831853 * p;
    let x = (R + r * cos(phi)) * cos(theta);
    let y = (R + r * cos(phi)) * sin(theta);
    let z = r * sin(phi);
    return vec3<f32>(x, y, z);
}

fn map(p_in: vec3<f32>, time: f32) -> f32 {
    let p_rot = rotate(p_in, vec3<f32>(time * 0.4, time * 0.6, time * 0.2));
    
    // (3,2) Torus Knot
    let knot_p = 3.0;
    let knot_q = 2.0;
    let R = 1.2; 
    let r = 0.5;
    let tube_radius = 0.18;
    
    var d = 1e10;
    // Higher density for smooth geometry
    let num_segments = 120;
    
    var p1 = getKnotPoint(0.0, knot_p, knot_q, R, r);
    for (var i = 1; i <= num_segments; i++) {
        let t = f32(i) / f32(num_segments);
        let p2 = getKnotPoint(t, knot_p, knot_q, R, r);
        d = min(d, sdCapsule(p_rot, p1, p2, tube_radius));
        p1 = p2;
    }
    
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
    let p_ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // MASSIVE SAFETY BUFFER: Move camera way back
    let ro = vec3<f32>(0.0, 0.0, -45.0);
    // Adjust FOV for distance
    let rd = normalize(vec3<f32>(p_ndc, 5.0));
    
    var t = 0.0;
    var hit = false;
    var dist = 0.0;
    
    for (var i = 0; i < 96; i++) {
        dist = map(ro + rd * t, time);
        if (abs(dist) < 0.001) {
            hit = true;
            break;
        }
        t += dist;
        if (t > 60.0) { break; }
    }
    
    if (hit) {
        let pos = ro + rd * t;
        let normal = getNormal(pos, time);
        
        // Purple Primary Light
        let l1_dir = normalize(vec3<f32>(1.5, 2.0, -3.0));
        let l1_color = vec3<f32>(0.7, 0.2, 1.0); // Purple
        let diff1 = max(dot(normal, l1_dir), 0.0);
        let spec1 = pow(max(dot(reflect(-l1_dir, normal), -rd), 0.0), 32.0);
        
        // Teal Secondary Light
        let l2_dir = normalize(vec3<f32>(-2.0, -1.0, -2.0));
        let l2_color = vec3<f32>(0.0, 1.0, 0.8); // Teal
        let diff2 = max(dot(normal, l2_dir), 0.0);
        let spec2 = pow(max(dot(reflect(-l2_dir, normal), -rd), 0.0), 16.0);
        
        // Rim Light
        let rim = pow(1.0 - max(dot(normal, -rd), 0.0), 4.0);
        
        // Combine Shading
        let ambient = vec3<f32>(0.02, 0.01, 0.03);
        var final_color = ambient;
        final_color += l1_color * (diff1 * 0.6 + spec1 * 0.4);
        final_color += l2_color * (diff2 * 0.4 + spec2 * 0.3 + rim * 0.3);
        
        return vec4<f32>(final_color, 1.0);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
