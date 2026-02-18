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

fn sd_sphere(p: vec3<f32>, r: f32) -> f32 {
    return length(p) - r;
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

fn hand_center() -> vec3<f32> {
    return vec3<f32>(0.0, -0.30, 0.0);
}

fn map_scene(p_world: vec3<f32>, time: f32) -> MapRes {
    let p = rotate_y(p_world, time * 0.24);
    let hc = hand_center();

    var d_hand = sd_round_box(p - hc, vec3<f32>(0.23, 0.10, 0.08), 0.045);
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(-0.14, 0.08, 0.03), hc + vec3<f32>(-0.15, 0.30, 0.06), 0.040));
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(-0.07, 0.10, 0.05), hc + vec3<f32>(-0.07, 0.36, 0.08), 0.034));
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(0.00, 0.10, 0.05), hc + vec3<f32>(0.00, 0.40, 0.09), 0.033));
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(0.07, 0.10, 0.05), hc + vec3<f32>(0.08, 0.35, 0.08), 0.032));
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(0.14, 0.09, 0.04), hc + vec3<f32>(0.16, 0.29, 0.06), 0.030));
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(-0.22, -0.04, -0.01), hc + vec3<f32>(-0.05, 0.11, 0.05), 0.041));
    d_hand = min(d_hand, sd_capsule(p, hc + vec3<f32>(0.00, -0.17, -0.02), hc + vec3<f32>(0.00, -0.42, -0.04), 0.070));

    let palm_local = p - (hc + vec3<f32>(0.0, 0.03, 0.09));
    let ring = abs(length(palm_local.xy) - 0.070) - 0.006;
    let ring_extrude = max(ring, abs(palm_local.z) - 0.008);
    let sig_cross = max(abs(palm_local.x) - 0.005, max(abs(palm_local.y) - 0.040, abs(palm_local.z) - 0.007));
    let sigil_carve = min(ring_extrude, sig_cross);
    d_hand = max(d_hand, -sigil_carve);

    let orb_center = vec3<f32>(0.0, 0.42, 0.0);
    var d_orb = sd_sphere(p - orb_center, 0.118);
    let drift = vec3<f32>(0.016 * sin(time * 0.42), 0.012 * cos(time * 0.33), 0.020);
    let d_core = sd_sphere(p - (orb_center + drift), 0.046);
    d_orb = max(d_orb, -d_core);

    let seg_count = 128;
    var d_halo = 1e6;
    var a = hc + vec3<f32>(0.18, 0.10, 0.13);
    for (var i = 1; i <= seg_count; i++) {
        let t = f32(i) / f32(seg_count);
        let ang = t * TAU;
        let r = 0.18 + 0.01 * sin(ang * 4.0 + time * 0.18);
        let b = hc + vec3<f32>(cos(ang) * r, 0.10 + 0.01 * sin(ang * 3.0), 0.13 + sin(ang) * r * 0.7);
        d_halo = min(d_halo, sd_capsule(p, a, b, 0.0055));
        a = b;
    }

    var res = MapRes(d_hand, 1.0);
    if (d_orb < res.d) {
        res = MapRes(d_orb, 2.0);
    }
    if (d_halo < res.d) {
        res = MapRes(d_halo, 3.0);
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

    let ro = vec3<f32>(0.0, -0.02, -6.8);
    let rd = normalize(vec3<f32>(ndc, 2.36));

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
        if (t > 28.0) {
            break;
        }
    }

    if (!hit) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let p = ro + rd * t;
    let n = normal_at(p, time);
    let v = normalize(-rd);

    let key_dir = normalize(vec3<f32>(0.92, 1.08, -0.74));
    let rim_dir = normalize(vec3<f32>(-1.00, 0.35, -0.56));
    let key_col = vec3<f32>(0.44, 0.08, 0.10);
    let rim_col = vec3<f32>(0.10, 0.14, 0.16);

    let ndl_key = max(dot(n, key_dir), 0.0);
    let ndl_rim = max(dot(n, rim_dir), 0.0);

    let h_key = normalize(key_dir + v);
    let h_rim = normalize(rim_dir + v);

    var col = vec3<f32>(0.0);

    if (mat < 1.5) {
        let spec = pow(max(dot(n, h_key), 0.0), 52.0);
        let spec2 = pow(max(dot(n, h_rim), 0.0), 38.0);
        let fres = pow(1.0 - max(dot(n, v), 0.0), 3.6);
        let stone = 0.5 + 0.5 * sin((p.x + p.z) * 8.0 + p.y * 6.0);
        let base = vec3<f32>(0.035, 0.030, 0.030) + stone * vec3<f32>(0.016, 0.012, 0.010);
        col = base * (0.18 + 0.46 * ndl_key + 0.16 * ndl_rim);
        col += key_col * (0.45 * spec);
        col += rim_col * (0.30 * spec2 + 0.10 * fres);
    } else if (mat < 2.5) {
        let spec = pow(max(dot(n, h_key), 0.0), 180.0);
        let spec2 = pow(max(dot(n, h_rim), 0.0), 110.0);
        let fres = pow(1.0 - max(dot(n, v), 0.0), 4.2);

        let orb_center = rotate_y(vec3<f32>(0.0, 0.42, 0.0), time * 0.24);
        let e = p - orb_center;
        let lat = smoothstep(0.012, 0.0, abs(sin(atan2(e.z, e.x) * 6.0) * 0.03 + e.y * 0.25));
        let core = smoothstep(0.080, 0.0, length(e - vec3<f32>(0.010 * sin(time * 0.42), 0.010 * cos(time * 0.33), 0.02)));

        col = vec3<f32>(0.050, 0.052, 0.058) * (0.24 + 0.44 * ndl_key);
        col += key_col * (0.26 * spec + 0.18 * core);
        col += rim_col * (0.24 * spec2 + 0.20 * fres);
        col += key_col * (0.08 * lat);
    } else {
        let spec = pow(max(dot(n, h_key), 0.0), 110.0);
        let fres = pow(1.0 - max(dot(n, v), 0.0), 3.8);
        let pulse = 0.5 + 0.5 * sin(time * 0.8);
        col = vec3<f32>(0.022, 0.024, 0.029);
        col += key_col * (0.22 * ndl_key + 0.42 * spec + 0.05 * pulse);
        col += rim_col * (0.36 * ndl_rim + 0.16 * fres);
    }

    // Subtle CRT rolloff for a soft retro tube response.
    let scan = 0.97 + 0.03 * sin((uv.y * resolution.y) * 0.38 + time * 0.12);
    col *= scan * 1.45;
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
