// Y2K Bold — Eye Element
// Bold modern Y2K aesthetic: high-contrast, VHS RGB channel separation, clean alpha
// 60fps :: time = u.time / 60.0

fn ease_out_elastic(t: f32) -> f32 {
    if (t <= 0.0) { return 0.0; }
    if (t >= 1.0) { return 1.0; }
    let p = 0.3;
    return pow(2.0, -10.0 * t) * sin((t - p / 4.0) * 6.283185 / p) + 1.0;
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if (t < 0.5) { return 4.0 * t * t * t; }
    let f = 2.0 * t - 2.0;
    return 0.5 * f * f * f + 1.0;
}

fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158;
    let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn sdVesica(p_in: vec2<f32>, r: f32, d: f32) -> f32 {
    let p = abs(p_in);
    let b = sqrt(max(r * r - d * d, 0.0001));
    if ((p.y - d) * b > p.x * d) {
        return length(p - vec2<f32>(0.0, d));
    } else {
        return length(p - vec2<f32>(-b, 0.0)) - r;
    }
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i + vec2<f32>(0.0,0.0)), hash21(i + vec2<f32>(1.0,0.0)), u.x),
        mix(hash21(i + vec2<f32>(0.0,1.0)), hash21(i + vec2<f32>(1.0,1.0)), u.x),
        u.y
    );
}

// Sample eye SDF and return iris/pupil/sclera value at a given offset uv
fn sample_eye_alpha(uv_off: vec2<f32>, aspect: f32, time: f32, open_t: f32) -> f32 {
    let breath = 1.0 + 0.045 * sin(time * 1.1) + 0.02 * sin(time * 2.6);
    let p = uv_off * vec2<f32>(aspect, 1.0) / breath;

    let r = 0.80;
    let d = mix(0.79, 0.44, open_t);
    let eye_dist = sdVesica(p.yx, r, d);
    return smoothstep(0.006, -0.006, eye_dist);
}

fn sample_eye_color(uv_off: vec2<f32>, aspect: f32, time: f32, open_t: f32) -> vec4<f32> {
    let breath = 1.0 + 0.045 * sin(time * 1.1) + 0.02 * sin(time * 2.6);
    let p = uv_off * vec2<f32>(aspect, 1.0) / breath;

    let r = 0.80;
    let d = mix(0.79, 0.44, open_t);
    let eye_dist = sdVesica(p.yx, r, d);
    let eye_mask = smoothstep(0.006, -0.006, eye_dist);

    if (eye_mask <= 0.001) {
        return vec4<f32>(0.0);
    }

    // Iris tracking
    let look_x = sin(time * 0.42) * 0.06;
    let look_y = cos(time * 0.31) * 0.03;
    let iris_center = vec2<f32>(look_x, look_y);

    let dist_iris = length(p - iris_center);
    let iris_r = 0.22;

    // -- Y2K COLOR PALETTE --
    // Electric cyan + hot magenta + chrome white
    let col_iris_inner = vec3<f32>(0.0, 0.95, 1.0);   // electric cyan
    let col_iris_outer = vec3<f32>(0.9, 0.0, 0.6);    // hot magenta
    let col_sclera     = vec3<f32>(0.95, 0.97, 1.0);  // near-white chrome

    // Sclera base with vein hint
    var final_color = col_sclera;
    let vein = smoothstep(0.25, 0.0, abs(p.x * 2.5 + sin(p.y * 30.0) * 0.03)) * 0.08;
    final_color -= vec3<f32>(vein * 0.5, vein * 0.0, vein * 0.0);

    // Iris
    if (dist_iris < iris_r + 0.025) {
        let iris_ang = atan2((p - iris_center).y, (p - iris_center).x);
        let iris_ring = sin(iris_ang * 24.0 + time * 0.3) * 0.5 + 0.5;
        let iris_t = smoothstep(0.04, iris_r, dist_iris);
        var iris_col = mix(col_iris_inner, col_iris_outer, iris_t);

        // Limbal ring (bold dark border)
        let limbal = smoothstep(iris_r - 0.035, iris_r, dist_iris);
        iris_col = mix(iris_col, vec3<f32>(0.02, 0.0, 0.05), limbal * 0.9);

        let iris_mask = smoothstep(iris_r, iris_r - 0.012, dist_iris);
        final_color = mix(final_color, iris_col, iris_mask);
    }

    // Pupil — bold pure black
    let dilate = ease_in_out_cubic(sin(time * 0.75) * 0.5 + 0.5);
    let pupil_r = mix(0.055, 0.10, dilate);
    let pupil_mask = smoothstep(pupil_r, pupil_r - 0.01, dist_iris);
    final_color = mix(final_color, vec3<f32>(0.0, 0.0, 0.0), pupil_mask);

    // Specular (chrome Y2K sheen — big, bold)
    let spec1 = smoothstep(0.055, 0.01, length(p - (iris_center + vec2<f32>(-0.075, 0.085))));
    let spec2 = smoothstep(0.025, 0.0, length(p - (iris_center + vec2<f32>(0.05, -0.055)))) * 0.5;
    final_color = min(vec3<f32>(1.0), final_color + spec1 + spec2);

    return vec4<f32>(final_color, eye_mask);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time / 60.0;
    let aspect = u.resolution.x / u.resolution.y;
    let uv_c = uv - 0.5;

    // --- Blink sequence (6-second loop) ---
    let lt = fract(time / 6.0) * 6.0;

    // Open from closed at t=0 over 0.8s
    var open_t = ease_out_elastic(clamp(lt / 0.8, 0.0, 1.0));

    // Fast blink at 2.5s
    let b1 = smoothstep(2.4, 2.5, lt) * smoothstep(2.75, 2.6, lt);
    // Slow dramatic blink at 4.5s
    let b2_down = ease_in_out_cubic(smoothstep(4.3, 4.6, lt));
    let b2_up   = ease_out_back(smoothstep(4.6, 5.1, lt));
    let b2 = b2_down * (1.0 - b2_up);

    open_t = clamp(open_t - b1 * 0.98 - b2, 0.0, 1.0);

    // --- VHS RGB Channel Separation ---
    // Amount pulses on blinks and has a slow ambient drift
    let blink_event = max(b1, b2_down * (1.0 - b2_up * 0.5));
    let ca_pulse = blink_event * 0.018;
    let ca_drift = 0.004 * sin(time * 2.7) + 0.002 * sin(time * 7.3);
    let ca = ca_pulse + abs(ca_drift);

    // Horizontal VHS offset (R right, B left, G center)
    let off_r = vec2<f32>( ca * aspect * 0.5, ca * 0.25);
    let off_b = vec2<f32>(-ca * aspect * 0.5, -ca * 0.25);

    // Sample each channel at offset
    let s_r = sample_eye_color(uv_c + off_r / vec2<f32>(aspect, 1.0), aspect, time, open_t);
    let s_g = sample_eye_color(uv_c, aspect, time, open_t);
    let s_b = sample_eye_color(uv_c + off_b / vec2<f32>(aspect, 1.0), aspect, time, open_t);

    // Composite — RGB channels from different samples, alpha from center
    let out_col = vec3<f32>(s_r.r, s_g.g, s_b.b);
    let out_a   = s_g.a;

    // Subtle scanline shimmer (Y2K TV feel) — very light, not distracting
    let scan = 1.0 - 0.025 * step(0.5, fract(uv.y * u.resolution.y * 0.5));

    return vec4<f32>(out_col * scan, out_a);
}
