const TAU: f32 = 6.283185307179586;

struct MapRes {
    d: f32,
    mat: f32,
};

fn rotate_y(p: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec3<f32>(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

fn rotate_x(p: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec3<f32>(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn sd_sphere(p: vec3<f32>, r: f32) -> f32 {
    return length(p) - r;
}

fn sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

fn sd_torus(p: vec3<f32>, t: vec2<f32>) -> f32 {
    let q = vec2<f32>(length(p.xz) - t.x, p.y);
    return length(q) - t.y;
}

fn sd_cyl(p: vec3<f32>, h: f32, r: f32) -> f32 {
    let d = abs(vec2<f32>(length(p.xz), p.y)) - vec2<f32>(r, h);
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0)));
}

fn map_scene(p_world: vec3<f32>, time: f32) -> MapRes {
    let p = rotate_y(p_world, time * 0.20);
    let c = vec3<f32>(0.0, 0.12, 0.0);
    let e = p - c;

    let d_shell = abs(sd_sphere(e, 0.44)) - 0.012;

    let core_offset = vec3<f32>(0.03 * sin(time * 0.34), 0.02 * cos(time * 0.27), 0.0);
    let d_core = sd_sphere(e - core_offset, 0.13);

    let ring_a = sd_torus(rotate_x(e, 0.45), vec2<f32>(0.54, 0.008));
    let ring_b = sd_torus(rotate_x(rotate_y(e, 1.2), -0.38), vec2<f32>(0.54, 0.008));
    var d_rings = min(ring_a, ring_b);

    let seg_count = 144;
    var d_runes = 1e6;
    for (var i = 0; i < seg_count; i++) {
        let t = f32(i) / f32(seg_count);
        let a = t * TAU;
        let jitter = 0.02 * sin(a * 6.0 + time * 0.4);
        let p0 = c + vec3<f32>(cos(a) * (0.33 + jitter), sin(a * 3.0) * 0.02, sin(a) * (0.33 + jitter));
        let p1 = c + vec3<f32>(cos(a) * (0.37 + jitter), sin(a * 3.0) * 0.02, sin(a) * (0.37 + jitter));
        d_runes = min(d_runes, sd_capsule(p, p0, p1, 0.0028));
    }

    let d_base = sd_cyl(p - vec3<f32>(0.0, -0.62, 0.0), 0.20, 0.22);

    var res = MapRes(d_shell, 1.0);
    if (d_core < res.d) {
        res = MapRes(d_core, 2.0);
    }
    if (d_rings < res.d) {
        res = MapRes(d_rings, 3.0);
    }
    if (d_runes < res.d) {
        res = MapRes(d_runes, 3.0);
    }
    if (d_base < res.d) {
        res = MapRes(d_base, 4.0);
    }
    return res;
}

fn normal_at(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = 0.0009;
    let ex = vec3<f32>(e, 0.0, 0.0);
    let ey = vec3<f32>(0.0, e, 0.0);
    let ez = vec3<f32>(0.0, 0.0, e);
    return normalize(vec3<f32>(
        map_scene(p + ex, time).d - map_scene(p - ex, time).d,
        map_scene(p + ey, time).d - map_scene(p - ey, time).d,
        map_scene(p + ez, time).d - map_scene(p - ez, time).d
    ));
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let ndc = (uv - 0.5) * vec2<f32>(aspect, -1.0);

    let ro = vec3<f32>(0.0, 0.02, -6.0);
    let rd = normalize(vec3<f32>(ndc, 2.4));

    var t = 0.0;
    var hit = false;
    var mat = 0.0;
    for (var i = 0; i < 180; i++) {
        let p = ro + rd * t;
        let mr = map_scene(p, time);
        if (mr.d < 0.0006) {
            hit = true;
            mat = mr.mat;
            break;
        }
        t += mr.d * 0.92;
        if (t > 30.0) {
            break;
        }
    }

    if (!hit) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let p = ro + rd * t;
    let n = normal_at(p, time);
    let v = normalize(-rd);

    let key_dir = normalize(vec3<f32>(0.94, 1.00, -0.72));
    let rim_dir = normalize(vec3<f32>(-1.00, 0.30, -0.54));
    let key_col = vec3<f32>(0.42, 0.07, 0.09);
    let rim_col = vec3<f32>(0.09, 0.13, 0.15);

    let ndl_key = max(dot(n, key_dir), 0.0);
    let ndl_rim = max(dot(n, rim_dir), 0.0);
    let h_key = normalize(key_dir + v);
    let h_rim = normalize(rim_dir + v);

    var col = vec3<f32>(0.0);

    if (mat < 1.5) {
        let spec = pow(max(dot(n, h_key), 0.0), 210.0);
        let spec2 = pow(max(dot(n, h_rim), 0.0), 130.0);
        let fres = pow(1.0 - max(dot(n, v), 0.0), 5.0);
        col = vec3<f32>(0.028, 0.031, 0.036) * (0.18 + 0.28 * ndl_key);
        col += key_col * (0.18 * spec);
        col += rim_col * (0.20 * spec2 + 0.28 * fres);
    } else if (mat < 2.5) {
        let pulse = 0.5 + 0.5 * sin(time * 0.85);
        let spec = pow(max(dot(n, h_key), 0.0), 40.0);
        col = vec3<f32>(0.020, 0.026, 0.032) * (0.30 + 0.44 * ndl_key);
        col += rim_col * (0.16 * pulse + 0.14 * spec);
        col += key_col * (0.10 * pulse);
    } else if (mat < 3.5) {
        let spec = pow(max(dot(n, h_key), 0.0), 120.0);
        let fres = pow(1.0 - max(dot(n, v), 0.0), 3.2);
        let pulse = 0.5 + 0.5 * sin(time * 0.6);
        col = vec3<f32>(0.025, 0.030, 0.036);
        col += key_col * (0.22 * ndl_key + 0.38 * spec + 0.04 * pulse);
        col += rim_col * (0.30 * ndl_rim + 0.18 * fres);
    } else {
        let spec = pow(max(dot(n, h_key), 0.0), 60.0);
        let spec2 = pow(max(dot(n, h_rim), 0.0), 42.0);
        col = vec3<f32>(0.030, 0.026, 0.028) * (0.20 + 0.42 * ndl_key + 0.15 * ndl_rim);
        col += key_col * (0.24 * spec);
        col += rim_col * (0.14 * spec2);
    }

    let scan = 0.97 + 0.03 * sin((uv.y * resolution.y) * 0.36 + time * 0.10);
    col *= scan * 1.45;
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
