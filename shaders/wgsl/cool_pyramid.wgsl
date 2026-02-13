// Cool Pyramid Shader
// A glowing, wireframe-style pyramid with pulsing internal light.

fn sdPyramid(p: vec3<f32>, h: f32) -> f32 {
    let m2 = h*h + 0.25;
    
    var p_mut = p;
    p_mut.x = abs(p_mut.x);
    p_mut.z = abs(p_mut.z);
    if (p_mut.z > p_mut.x) {
        let tmp = p_mut.x;
        p_mut.x = p_mut.z;
        p_mut.z = tmp;
    }
    p_mut.x -= 0.5;
    p_mut.z -= 0.5;

    let s = vec3<f32>(m2, h, m2);
    let q = p_mut - s * max(dot(p_mut, s) / dot(s, s), 0.0);
    
    let d1 = length(q) * sign(max(q.x, q.y));
    let d2 = p.y;
    return max(d1, d2);
}

fn rotateY(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

fn rotateX(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn map(p: vec3<f32>, time: f32) -> f32 {
    let p_rot = rotateY(rotateX(p, 0.2), time * 1.2);
    return sdPyramid(p_rot + vec3<f32>(0.0, 0.4, 0.0), 1.0);
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
    
    // Camera
    let ro = vec3<f32>(0.0, 0.2, -2.5);
    let rd = normalize(vec3<f32>(p, 1.5));
    
    // Raymarching
    var t = 0.0;
    var hit = false;
    for (var i = 0; i < 60; i++) {
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
        
        // Edge glowing
        let edge = 1.0 - max(dot(n, -rd), 0.0);
        let glow = pow(edge, 4.0) * 2.5;
        
        let pulse = 0.5 + 0.5 * sin(time * 4.0);
        let color = vec3<f32>(0.0, 1.0, 0.8) * (glow + pulse * 0.2); // Cyan-ish
        
        let alpha = smoothstep(1.5, 0.5, t) * 0.9;
        return vec4<f32>(color, alpha);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
