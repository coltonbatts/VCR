// Y2K Bold — Column Element
// Bold modern Y2K: chrome/obsidian column, VHS RGB edge fringing, clean alpha
// 60fps :: time = u.time / 60.0

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}

fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158;
    let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if (t < 0.5) { return 4.0 * t * t * t; }
    let f = 2.0 * t - 2.0;
    return 0.5 * f * f * f + 1.0;
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i + vec2<f32>(0.0, 0.0)), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y
    );
}

fn fbm2(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var pc = p;
    for (var i = 0; i < 4; i++) {
        v += a * noise2(pc);
        pc = pc * 2.1 + vec2<f32>(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

fn sdBox(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// Returns (alpha_mask, column_color)
fn column_sample(uv_c: vec2<f32>, aspect: f32, time: f32) -> vec4<f32> {
    // Rise with overshoot
    let rise = ease_out_back(clamp(time / 1.2, 0.0, 1.0));
    let rise_offset = (1.0 - rise) * 0.55;

    // Majestic sway
    let sway_ang = sin(time * 0.65) * 0.018;
    let c_sw = cos(sway_ang);
    let s_sw = sin(sway_ang);
    let rot = mat2x2<f32>(c_sw, -s_sw, s_sw, c_sw);

    let breath = 1.0 + 0.018 * sin(time * 0.9);

    let p_raw = uv_c * vec2<f32>(aspect, 1.0);
    var p = rot * p_raw;
    p.y += rise_offset;
    p /= breath;

    // Column geometry
    let y_norm = clamp(p.y * 2.0 + 0.5, 0.0, 1.0);
    let col_w_base = 0.068;
    let col_w_top  = 0.050;
    let col_w = mix(col_w_base, col_w_top, y_norm);

    let x_surf = p.x / max(col_w, 0.0001);
    var d_shaft = abs(p.x) - col_w;

    // Fluting — crisp, bold (Y2K: clear geometry)
    let flute_freq = 16.0;
    let flute_depth = 0.004;
    let flute = sin(acos(clamp(x_surf, -1.0, 1.0)) * flute_freq);
    d_shaft += flute * flute_depth * smoothstep(0.95, 0.5, abs(x_surf));

    // Capital
    let cap_y = 0.365;
    let d_cap  = sdBox(p - vec2<f32>(0.0, cap_y),  vec2<f32>(0.088, 0.048));
    // Sub-capital detail
    let d_cap2 = sdBox(p - vec2<f32>(0.0, cap_y - 0.055), vec2<f32>(0.075, 0.012));

    // Base
    let base_y = -0.365;
    let d_base  = sdBox(p - vec2<f32>(0.0, base_y), vec2<f32>(0.095, 0.042));
    let d_base2 = sdBox(p - vec2<f32>(0.0, base_y + 0.055), vec2<f32>(0.075, 0.012));

    let d_col   = min(d_shaft, min(d_cap, min(d_cap2, min(d_base, d_base2))));
    let h_limit = 0.42;
    let d_clip  = abs(p.y) - h_limit;
    let d_final = max(d_col, d_clip);

    let alpha_mask = smoothstep(0.0012, -0.0012, d_final);

    if (alpha_mask <= 0.0) {
        return vec4<f32>(0.0);
    }

    // --- Material: Y2K Chrome / Obsidian ---
    // Pseudo-3D cylinder normal
    var N = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(p.y) < cap_y - 0.048 && p.y > base_y + 0.042) {
        let nx = clamp(p.x / max(col_w, 0.0001), -1.0, 1.0);
        let nz = sqrt(max(0.0, 1.0 - nx * nx));
        let flute_n = cos(acos(clamp(nx, -1.0, 1.0)) * flute_freq) * 0.25;
        N = normalize(vec3<f32>(nx + flute_n, 0.0, nz));
    } else {
        N = normalize(vec3<f32>(p.x * 1.5, p.y * 1.5, 1.0));
    }

    // Light: primary from upper-right (bold directional), secondary fill
    let L1 = normalize(vec3<f32>(sin(time * 0.4) * 0.5 + 0.5, 0.3, 0.9));
    let L2 = normalize(vec3<f32>(-0.6, 0.2, 0.7));
    let V  = vec3<f32>(0.0, 0.0, 1.0);
    let H1 = normalize(L1 + V);

    let diff1 = max(dot(N, L1), 0.0);
    let diff2 = max(dot(N, L2), 0.0) * 0.35;
    let spec  = pow(max(dot(N, H1), 0.0), 64.0); // Sharp chrome highlight

    // Rim light — Y2K: electric cyan rim
    let rim = pow(1.0 - max(dot(N, V), 0.0), 2.5);
    let rim_col = vec3<f32>(0.0, 0.9, 1.0); // Cyan rim

    // Texture: chrome-veined obsidian
    let noise_p = vec2<f32>(p.x, p.y * 0.6) * 10.0;
    let fbm_val  = fbm2(noise_p + time * 0.04);
    // Vein: bright chrome streaks (Y2K: metallic, not stone)
    let vein = smoothstep(0.38, 0.42, abs(sin(noise_p.y * 0.5 + fbm_val * 3.5)));

    let col_base  = vec3<f32>(0.06, 0.04, 0.12); // Deep obsidian
    let col_vein  = vec3<f32>(0.85, 0.90, 1.0);  // Chrome vein
    let col_surf  = mix(col_base, col_vein, vein * 0.5);

    var color = col_surf * (0.15 + diff1 * 0.75 + diff2) + spec * vec3<f32>(0.9, 0.95, 1.0);
    color += rim_col * rim * 0.45;

    // Y2K Scanner sweep — cyan beam
    let sweep_y = sin(time * 0.9) * 0.48;
    let sweep   = smoothstep(0.09, 0.0, abs(p.y - sweep_y));
    color += vec3<f32>(0.0, 1.0, 0.9) * sweep * 0.45;

    // Base shadow
    let contact_shadow = smoothstep(-0.42, -0.3, p.y);
    color *= (0.25 + 0.75 * contact_shadow);

    return vec4<f32>(color, alpha_mask);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time / 60.0;
    let aspect = u.resolution.x / u.resolution.y;
    let uv_c = uv - 0.5;

    // VHS edge fringing: maximum on reveal, then settles to subtle ambient
    // Strong at start, pulses with sweep
    let reveal_pulse = ease_out_back(clamp(time / 1.2, 0.0, 1.0))
        * (1.0 - ease_out_expo(clamp((time - 1.2) / 1.0, 0.0, 1.0)));
    let sweep_pulse = sin(time * 0.9) * 0.5 + 0.5; // ties to sweep frequency

    let event = max(reveal_pulse * 0.8, sweep_pulse * 0.15);
    let ca = 0.003 + event * 0.011 + 0.0018 * sin(time * 3.8);

    // Horizontal split for VHS tape authenticity
    let off_r = vec2<f32>( ca, 0.0);
    let off_b = vec2<f32>(-ca, 0.0);

    let s_r = column_sample(uv_c + off_r, aspect, time);
    let s_g = column_sample(uv_c, aspect, time);
    let s_b = column_sample(uv_c + off_b, aspect, time);

    let out_col = vec3<f32>(s_r.r, s_g.g, s_b.b);
    let out_a   = s_g.a;

    return vec4<f32>(out_col, out_a);
}
