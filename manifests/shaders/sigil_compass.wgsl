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

fn rotate2(v: vec2<f32>, a: f32) -> vec2<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec2<f32>(v.x * c - v.y * s, v.x * s + v.y * c);
}

fn sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

fn sd_round_box(p: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let q = abs(p) - b;
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}

fn sd_cyl(p: vec3<f32>, h: f32, r: f32) -> f32 {
    let d = abs(vec2<f32>(length(p.xy), p.z)) - vec2<f32>(r, h);
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0)));
}

fn sd_torus(p: vec3<f32>, t: vec2<f32>) -> f32 {
    let q = vec2<f32>(length(p.xy) - t.x, p.z);
    return length(q) - t.y;
}

fn map_scene(p_world: vec3<f32>, time: f32) -> MapRes {
    let p = rotate_y(p_world, 0.42 * sin(time * 0.42));

    var d_base = sd_cyl(p, 0.05, 0.46);

    for (var i = 0; i < 8; i++) {
        let ang = f32(i) * (TAU / 8.0);
        let qxy = rotate2(p.xy, -ang);
        var spike_len = 0.25;
        if ((i % 2) == 0) {
            spike_len = 0.34;
        }
        let spike = sd_round_box(vec3<f32>(qxy.x - (0.48 + spike_len * 0.5), qxy.y, p.z), vec3<f32>(spike_len * 0.5, 0.018, 0.028), 0.010);
        d_base = min(d_base, spike);
    }

    let d_outer = sd_torus(p, vec2<f32>(0.68, 0.010));

    var d_ticks = 1e6;
    let seg_count = 128;
    for (var i = 0; i < seg_count; i++) {
        let t = f32(i) / f32(seg_count);
        let a = t * TAU;
        let r0 = 0.60;
        let r1 = 0.67;
        let p0 = vec3<f32>(cos(a) * r0, sin(a) * r0, 0.0);
        let p1 = vec3<f32>(cos(a) * r1, sin(a) * r1, 0.0);
        d_ticks = min(d_ticks, sd_capsule(p, p0, p1, 0.0026));
    }

    let needle_ang = time * 0.42;
    let nxy = rotate2(p.xy, -needle_ang);
    let d_needle_a = sd_round_box(vec3<f32>(nxy.x - 0.28, nxy.y, p.z), vec3<f32>(0.26, 0.010, 0.015), 0.004);
    let d_needle_b = sd_round_box(vec3<f32>(nxy.x + 0.22, nxy.y, p.z), vec3<f32>(0.18, 0.010, 0.013), 0.004);
    let d_needle = min(d_needle_a, d_needle_b);

    let d_core = sd_cyl(p, 0.030, 0.090);

    var res = MapRes(d_base, 1.0);
    if (d_outer < res.d) {
        res = MapRes(d_outer, 2.0);
    }
    if (d_ticks < res.d) {
        res = MapRes(d_ticks, 2.0);
    }
    if (d_needle < res.d) {
        res = MapRes(d_needle, 3.0);
    }
    if (d_core < res.d) {
        res = MapRes(d_core, 3.0);
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

    let ro = vec3<f32>(0.0, 0.0, -7.1);
    let rd = normalize(vec3<f32>(ndc, 2.45));

    var t = 0.0;
    var hit = false;
    var mat = 0.0;
    for (var i = 0; i < 176; i++) {
        let p = ro + rd * t;
        let mr = map_scene(p, time);
        if (mr.d < 0.0006) {
            hit = true;
            mat = mr.mat;
            break;
        }
        t += mr.d * 0.92;
        if (t > 32.0) {
            break;
        }
    }

    if (!hit) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let p = ro + rd * t;
    let n = normal_at(p, time);
    let v = normalize(-rd);

    let key_dir = normalize(vec3<f32>(0.92, 1.06, -0.74));
    let rim_dir = normalize(vec3<f32>(-1.00, 0.34, -0.56));
    let key_col = vec3<f32>(0.40, 0.07, 0.09);
    let rim_col = vec3<f32>(0.09, 0.13, 0.15);

    let ndl_key = max(dot(n, key_dir), 0.0);
    let ndl_rim = max(dot(n, rim_dir), 0.0);
    let h_key = normalize(key_dir + v);
    let h_rim = normalize(rim_dir + v);

    var col = vec3<f32>(0.0);

    if (mat < 1.5) {
        let spec = pow(max(dot(n, h_key), 0.0), 80.0);
        let spec2 = pow(max(dot(n, h_rim), 0.0), 56.0);
        let fres = pow(1.0 - max(dot(n, v), 0.0), 4.0);
        let etched = 0.5 + 0.5 * sin((p.x + p.z) * 16.0);
        let base = vec3<f32>(0.030, 0.030, 0.034) + etched * vec3<f32>(0.014, 0.010, 0.008);
        col = base * (0.20 + 0.46 * ndl_key + 0.16 * ndl_rim);
        col += key_col * (0.38 * spec);
        col += rim_col * (0.26 * spec2 + 0.12 * fres);
    } else if (mat < 2.5) {
        let spec = pow(max(dot(n, h_key), 0.0), 120.0);
        let pulse = 0.5 + 0.5 * sin(time * 0.6);
        col = vec3<f32>(0.020, 0.023, 0.028);
        col += key_col * (0.24 * ndl_key + 0.34 * spec + 0.06 * pulse);
        col += rim_col * (0.32 * ndl_rim);
    } else {
        let spec = pow(max(dot(n, h_key), 0.0), 170.0);
        let spec2 = pow(max(dot(n, h_rim), 0.0), 120.0);
        let pulse = 0.5 + 0.5 * sin(time * 0.9);
        col = vec3<f32>(0.036, 0.036, 0.040);
        col += key_col * (0.24 * ndl_key + 0.52 * spec + 0.08 * pulse);
        col += rim_col * (0.16 * ndl_rim + 0.40 * spec2);
    }

    let scan = 0.97 + 0.03 * sin((uv.y * resolution.y) * 0.40 + time * 0.12);
    col *= scan * 1.45;
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
