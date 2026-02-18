// High-fidelity occult composition: floating eye above a stone pyramid.
// Tuned for centered 9:16 framing with stable alpha and deterministic motion.

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

fn sd_ellipsoid(p: vec3<f32>, r: vec3<f32>) -> f32 {
    let k0 = length(p / r);
    let k1 = length(p / (r * r));
    return k0 * (k0 - 1.0) / max(k1, 1e-5);
}

fn sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}

fn sd_pyramid(p: vec3<f32>, h: f32, base_half: f32) -> f32 {
    // Pyramid local Y runs from 0 (base) to h (apex).
    let side_extent = (1.0 - p.y / h) * base_half;
    let side = max(abs(p.x), abs(p.z)) - side_extent;
    let bottom = -p.y;
    let top = p.y - h;
    return max(max(side, bottom), top);
}

fn eye_center() -> vec3<f32> {
    return vec3<f32>(0.0, 0.72, 0.0);
}

fn pyramid_space(p: vec3<f32>, time: f32) -> vec3<f32> {
    // Main 3D motion stays on pyramid.
    return rotate_y(p, time * 0.22);
}

fn eye_space(p: vec3<f32>, time: f32) -> vec3<f32> {
    // Keep eye/rays mostly front-facing for readability.
    return rotate_y(p, time * 0.05);
}

fn map_scene(p_world: vec3<f32>, time: f32) -> MapRes {
    let p_pyr_space = pyramid_space(p_world, time);
    let p_eye_space = eye_space(p_world, time);

    // Pyramid base form with ultra-subtle displacement (no visible cross-hatch shimmer).
    let p_pyr = p_pyr_space - vec3<f32>(0.0, -0.78, 0.0);
    var d_pyr = sd_pyramid(p_pyr, 1.18, 0.54);
    let stone = sin(p_pyr.x * 10.0 + sin(p_pyr.y * 2.2)) * sin(p_pyr.z * 8.5) * sin(p_pyr.y * 5.5);
    d_pyr += stone * 0.0009;

    // Flattened occult eye: almond via ellipsoid intersection + carved iris/pupil depth.
    let e = p_eye_space - eye_center();
    let eye_r = vec3<f32>(0.23, 0.095, 0.088);
    let d_eye_l = sd_ellipsoid(e - vec3<f32>(0.102, 0.0, 0.0), eye_r);
    let d_eye_r = sd_ellipsoid(e + vec3<f32>(0.102, 0.0, 0.0), eye_r);
    var d_eye = max(d_eye_l, d_eye_r);

    let drift = vec2<f32>(0.010 * sin(time * 0.35), 0.007 * cos(time * 0.29));
    let iris_center = vec3<f32>(drift.x, drift.y, 0.057);
    let pupil_center = vec3<f32>(drift.x * 0.8, drift.y * 0.8, 0.041);
    let d_iris_cavity = sd_sphere(e - iris_center, 0.060);
    let d_pupil_cavity = sd_sphere(e - pupil_center, 0.028);
    d_eye = max(d_eye, -d_iris_cavity);
    d_eye = max(d_eye, -d_pupil_cavity);

    // Occult sunburst rays behind the eye with high segment density for stability.
    let ray_count = 132;
    var d_rays = 1e6;
    for (var i = 0; i < ray_count; i++) {
        let t = f32(i) / f32(ray_count);
        let ang = t * TAU;
        let bend = 0.038 * sin(ang * 6.0 + time * 0.18);
        let dir = normalize(vec3<f32>(cos(ang), sin(ang) * 0.78, 0.0));
        let tangent = normalize(vec3<f32>(-dir.y, dir.x, 0.0));

        let a = eye_center() + dir * 0.30 + vec3<f32>(0.0, 0.0, -0.035);
        let b = eye_center() + dir * (0.52 + bend)
            + tangent * (0.026 * sin(ang * 3.0 + time * 0.22))
            + vec3<f32>(0.0, 0.0, -0.050);

        d_rays = min(d_rays, sd_capsule(p_eye_space, a, b, 0.0048));
    }

    var res = MapRes(d_pyr, 1.0);
    if (d_eye < res.d) {
        res = MapRes(d_eye, 2.0);
    }
    if (d_rays < res.d) {
        res = MapRes(d_rays, 3.0);
    }
    return res;
}

fn normal_at(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = 0.0009;
    let ex = vec3<f32>(e, 0.0, 0.0);
    let ey = vec3<f32>(0.0, e, 0.0);
    let ez = vec3<f32>(0.0, 0.0, e);
    let nx = map_scene(p + ex, time).d - map_scene(p - ex, time).d;
    let ny = map_scene(p + ey, time).d - map_scene(p - ey, time).d;
    let nz = map_scene(p + ez, time).d - map_scene(p - ez, time).d;
    return normalize(vec3<f32>(nx, ny, nz));
}

fn shade_pyramid(local: vec3<f32>, n: vec3<f32>, v: vec3<f32>, key_dir: vec3<f32>, rim_dir: vec3<f32>) -> vec3<f32> {
    let key_col = vec3<f32>(1.00, 0.74, 0.40);
    let rim_col = vec3<f32>(0.18, 0.88, 0.90);

    let ndl_key = max(dot(n, key_dir), 0.0);
    let ndl_rim = max(dot(n, rim_dir), 0.0);

    let h_key = normalize(key_dir + v);
    let h_rim = normalize(rim_dir + v);
    let spec_key = pow(max(dot(n, h_key), 0.0), 72.0);
    let spec_rim = pow(max(dot(n, h_rim), 0.0), 52.0);
    let fresnel = pow(1.0 - max(dot(n, v), 0.0), 4.0);

    let y_norm = clamp(local.y / 1.18, 0.0, 1.0);
    let row = floor((1.0 - y_norm) * 10.0);
    var stagger = 0.0;
    if (fract(row * 0.5) > 0.25) {
        stagger = 0.5;
    }

    let u = abs(local.x) + abs(local.z) * 0.75;
    let brick_u = fract(u * 8.0 + stagger);
    let brick_v = fract((1.0 - y_norm) * 10.0);
    let edge_u = min(brick_u, 1.0 - brick_u);
    let edge_v = min(brick_v, 1.0 - brick_v);
    let mortar = 1.0 - smoothstep(0.02, 0.05, min(edge_u, edge_v));

    let grain = 0.5 + 0.5 * sin(local.x * 6.2 + local.z * 7.1 + local.y * 4.0);
    var albedo = vec3<f32>(0.034, 0.036, 0.041) + vec3<f32>(0.048, 0.040, 0.032) * (0.35 + 0.65 * grain);
    albedo -= vec3<f32>(0.015, 0.013, 0.010) * mortar;

    var col = albedo * (0.24 + 0.62 * ndl_key + 0.24 * ndl_rim);
    col += key_col * (0.58 * spec_key);
    col += rim_col * (0.46 * spec_rim);
    col += mix(key_col, rim_col, 0.35) * fresnel * 0.20;

    return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn shade_eye(p_obj: vec3<f32>, n: vec3<f32>, v: vec3<f32>, time: f32, key_dir: vec3<f32>, rim_dir: vec3<f32>) -> vec3<f32> {
    let key_col = vec3<f32>(1.00, 0.76, 0.42);
    let rim_col = vec3<f32>(0.20, 0.90, 0.92);

    let ndl_key = max(dot(n, key_dir), 0.0);
    let ndl_rim = max(dot(n, rim_dir), 0.0);

    let h_key = normalize(key_dir + v);
    let h_rim = normalize(rim_dir + v);
    let spec_key = pow(max(dot(n, h_key), 0.0), 220.0);
    let spec_rim = pow(max(dot(n, h_rim), 0.0), 130.0);
    let fresnel = pow(1.0 - max(dot(n, v), 0.0), 3.6);

    let e = p_obj - eye_center();
    let eye_r = vec3<f32>(0.23, 0.095, 0.088);
    let d_eye_l = sd_ellipsoid(e - vec3<f32>(0.102, 0.0, 0.0), eye_r);
    let d_eye_r = sd_ellipsoid(e + vec3<f32>(0.102, 0.0, 0.0), eye_r);
    let d_almond = max(d_eye_l, d_eye_r);
    let edge = smoothstep(0.016, 0.002, abs(d_almond));

    let drift = vec2<f32>(0.010 * sin(time * 0.35), 0.007 * cos(time * 0.29));
    let iris_uv = (e.xy - drift) / vec2<f32>(0.18, 0.086);
    let iris_r = length(iris_uv);
    let iris_mask = smoothstep(0.62, 0.20, iris_r);
    let pupil_mask = smoothstep(0.30, 0.10, iris_r);
    let limbal = smoothstep(0.60, 0.50, iris_r) * (1.0 - pupil_mask);

    let theta = atan2(iris_uv.y, iris_uv.x);
    let striations = 0.5 + 0.5 * sin(theta * 18.0 + iris_r * 16.0 - time * 0.2);

    let sclera = vec3<f32>(0.84, 0.83, 0.80) * (0.84 + 0.16 * ndl_key);
    var iris_col = mix(vec3<f32>(0.08, 0.20, 0.22), vec3<f32>(0.18, 0.55, 0.52), clamp(iris_r * 1.5, 0.0, 1.0));
    iris_col *= 0.75 + 0.25 * striations;
    let pupil_col = vec3<f32>(0.02, 0.024, 0.030);

    var eye_col = mix(sclera, iris_col, iris_mask);
    eye_col = mix(eye_col, pupil_col, pupil_mask);
    eye_col += vec3<f32>(0.03, 0.15, 0.16) * limbal * 0.8;

    let iris_pulse = 0.5 + 0.5 * sin(time * 0.70);
    let emissive = vec3<f32>(0.06, 0.45, 0.43) * (0.04 + 0.05 * iris_pulse) * iris_mask * (1.0 - pupil_mask);

    var col = eye_col;
    col += key_col * (0.32 * ndl_key + 0.95 * spec_key);
    col += rim_col * (0.25 * ndl_rim + 0.60 * spec_rim);
    col += rim_col * fresnel * 0.20;
    col += emissive;
    col -= vec3<f32>(0.06, 0.05, 0.04) * edge * 0.35;

    return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn shade_rays(n: vec3<f32>, v: vec3<f32>, key_dir: vec3<f32>, rim_dir: vec3<f32>, time: f32) -> vec3<f32> {
    let key_col = vec3<f32>(1.00, 0.78, 0.44);
    let rim_col = vec3<f32>(0.22, 0.90, 0.94);

    let ndl_key = max(dot(n, key_dir), 0.0);
    let ndl_rim = max(dot(n, rim_dir), 0.0);
    let h_key = normalize(key_dir + v);
    let spec = pow(max(dot(n, h_key), 0.0), 120.0);
    let fresnel = pow(1.0 - max(dot(n, v), 0.0), 4.0);

    let pulse = 0.5 + 0.5 * sin(time * 0.45);
    var col = vec3<f32>(0.05, 0.06, 0.08);
    col += key_col * (0.45 * ndl_key + 0.70 * spec);
    col += rim_col * (0.60 * ndl_rim + 0.35 * fresnel);
    col += rim_col * (0.06 + 0.06 * pulse);

    return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, -1.0);

    // Centered 9:16 framing with clean safety margin at max extent.
    let ro = vec3<f32>(0.0, 0.0, -8.8);
    let focal = 2.55;
    let rd = normalize(vec3<f32>(p, focal));

    var t = 0.0;
    var hit = false;
    var mat = 0.0;

    for (var i = 0; i < 176; i++) {
        let pos = ro + rd * t;
        let res = map_scene(pos, time);
        if (res.d < 0.0006) {
            hit = true;
            mat = res.mat;
            break;
        }
        t += res.d * 0.92;
        if (t > 32.0) {
            break;
        }
    }

    if (!hit) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let pos = ro + rd * t;
    let n = normal_at(pos, time);
    let v = normalize(-rd);
    let obj_pyr = pyramid_space(pos, time);
    let obj_eye = eye_space(pos, time);

    // Dual-light rig: warm amber key + cool teal rim/fill.
    let key_dir = normalize(vec3<f32>(0.92, 1.05, -0.74));
    let rim_dir = normalize(vec3<f32>(-1.00, 0.40, -0.56));

    var color = vec3<f32>(0.0);
    if (mat < 1.5) {
        color = shade_pyramid(obj_pyr - vec3<f32>(0.0, -0.78, 0.0), n, v, key_dir, rim_dir);
    } else if (mat < 2.5) {
        color = shade_eye(obj_eye, n, v, time, key_dir, rim_dir);
    } else {
        color = shade_rays(n, v, key_dir, rim_dir, time);
    }

    return vec4<f32>(color, 1.0);
}
