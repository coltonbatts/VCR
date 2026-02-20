// High-fidelity (3,2) torus knot built from a high-density capsule chain.

fn rotate_xyz(p: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
    let cx = cos(r.x);
    let sx = sin(r.x);
    let cy = cos(r.y);
    let sy = sin(r.y);
    let cz = cos(r.z);
    let sz = sin(r.z);

    let py = vec3<f32>(p.x, p.y * cx - p.z * sx, p.y * sx + p.z * cx);
    let px = vec3<f32>(py.x * cy + py.z * sy, py.y, -py.x * sy + py.z * cy);
    return vec3<f32>(px.x * cz - px.y * sz, px.x * sz + px.y * cz, px.z);
}

fn sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

fn torus_knot_point(t01: f32, p: f32, q: f32, major_r: f32, minor_r: f32) -> vec3<f32> {
    let a = 6.2831853 * t01 * p;
    let b = 6.2831853 * t01 * q;
    let c = major_r + minor_r * cos(b);
    return vec3<f32>(c * cos(a), c * sin(a), minor_r * sin(b));
}

fn map_scene(world_p: vec3<f32>, time: f32) -> f32 {
    // Slow multi-axis rotation for stable silhouette evolution.
    let rotated = rotate_xyz(world_p, vec3<f32>(0.33 * time, 0.47 * time, 0.21 * time));

    // (3,2) knot geometry with dense capsules to avoid gaps/flicker.
    let knot_p = 3.0;
    let knot_q = 2.0;
    let major_r = 1.15;
    let minor_r = 0.44;
    let tube_r = 0.12;
    let segments = 160;

    var d = 1e9;
    var a = torus_knot_point(0.0, knot_p, knot_q, major_r, minor_r);
    for (var i = 1; i <= segments; i++) {
        let t = f32(i) / f32(segments);
        let b = torus_knot_point(t, knot_p, knot_q, major_r, minor_r);
        d = min(d, sd_capsule(rotated, a, b, tube_r));
        a = b;
    }

    return d;
}

fn estimate_normal(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = 0.0007;
    let ex = vec3<f32>(e, 0.0, 0.0);
    let ey = vec3<f32>(0.0, e, 0.0);
    let ez = vec3<f32>(0.0, 0.0, e);
    let nx = map_scene(p + ex, time) - map_scene(p - ex, time);
    let ny = map_scene(p + ey, time) - map_scene(p - ey, time);
    let nz = map_scene(p + ez, time) - map_scene(p - ez, time);
    return normalize(vec3<f32>(nx, ny, nz));
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);

    // Camera tuned for 9:16 with explicit safety buffer controls.
    let ro = vec3<f32>(0.0, 0.0, -31.5);
    let focal = 3.6;
    let rd = normalize(vec3<f32>(ndc, focal));

    var t = 0.0;
    var hit = false;

    for (var i = 0; i < 176; i++) {
        let p = ro + rd * t;
        let d = map_scene(p, time);
        if (d < 0.0006) {
            hit = true;
            break;
        }
        t += d * 0.92;
        if (t > 88.0) {
            break;
        }
    }

    if (!hit) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let p = ro + rd * t;
    let n = estimate_normal(p, time);
    let v = normalize(-rd);

    // Dual-light rig: deep purple key + teal rim/fill.
    let key_dir = normalize(vec3<f32>(0.95, 1.15, -0.72));
    let key_col = vec3<f32>(0.41, 0.13, 0.70);
    let rim_dir = normalize(vec3<f32>(-1.05, 0.26, -0.55));
    let rim_col = vec3<f32>(0.04, 0.78, 0.76);

    let key_ndl = max(dot(n, key_dir), 0.0);
    let rim_ndl = max(dot(n, rim_dir), 0.0);

    // Metallic response with smooth specular highlights.
    let h_key = normalize(key_dir + v);
    let h_rim = normalize(rim_dir + v);
    let spec_key = pow(max(dot(n, h_key), 0.0), 90.0);
    let spec_rim = pow(max(dot(n, h_rim), 0.0), 56.0);

    let fresnel = pow(1.0 - max(dot(n, v), 0.0), 4.2);
    let ambient = vec3<f32>(0.012, 0.010, 0.020);
    let base_metal = vec3<f32>(0.13, 0.13, 0.15);

    var color = ambient + base_metal * 0.20;
    color += key_col * (0.68 * key_ndl + 1.15 * spec_key);
    color += rim_col * (0.36 * rim_ndl + 0.82 * spec_rim);
    color += mix(key_col, rim_col, 0.42) * fresnel * 0.34;

    // Keep linear-energy result in valid range for clean alpha compositing.
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(color, 1.0);
}
