// Y2K Bold — Sacred Rings Element
// Bold modern Y2K: sharp HUD rings, VHS RGB offset on motion events, clean alpha
// 60fps :: time = u.time / 60.0

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}

fn ease_in_out_quart(t: f32) -> f32 {
    if (t < 0.5) { return 8.0 * t * t * t * t; }
    let f = t - 1.0;
    return 1.0 - 8.0 * f * f * f * f;
}

fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158;
    let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

// Evaluate rings at a given UV offset (for RGB channel splitting)
fn rings_at(uv_off: vec2<f32>, aspect: f32, time: f32) -> vec4<f32> {
    // Global breathing — subtle, smooth
    let breath = 1.0 + 0.03 * sin(time * 0.85) + 0.015 * sin(time * 2.2);
    let p = uv_off * vec2<f32>(aspect, 1.0) / breath;
    let dist = length(p);
    let angle = atan2(p.y, p.x);

    var alpha = 0.0;
    var color = vec3<f32>(0.0);

    // Y2K Palette: Electric Cyan | Hot Magenta | Chrome White | Neon Yellow
    let col_cyan    = vec3<f32>(0.0,  0.95, 1.0);
    let col_magenta = vec3<f32>(1.0,  0.05, 0.6);
    let col_white   = vec3<f32>(1.0,  1.0,  1.0);
    let col_yellow  = vec3<f32>(1.0,  0.95, 0.0);

    // --- Ring 1: Outer data ring — precise thin tick marks ---
    let r1_reveal = ease_out_expo(clamp(time / 0.6, 0.0, 1.0));
    let r1_r = 0.36 * r1_reveal;
    let r1_rot = time * 0.18;
    let r1_ticks = step(0.86, sin((angle + r1_rot) * 48.0));
    let r1_solid = step(0.78, sin((angle + r1_rot * 2.0) * 6.0)); // 6 major arcs
    let r1_base = smoothstep(0.0018, 0.0, abs(dist - r1_r));
    let r1_val = r1_base * (r1_solid * 0.6 + r1_ticks * 0.85);

    color += col_cyan * r1_val;
    alpha = max(alpha, r1_val);

    // --- Ring 2: Middle segmented (6-segment, bold) ---
    let r2_reveal = ease_out_back(clamp((time - 0.25) / 0.7, 0.0, 1.0));
    let r2_r = 0.265 * r2_reveal;
    let r2_w = 0.007;
    let r2_rot = -time * 0.45 + sin(time * 0.7) * 0.4;
    // 6 bold arcs, clear gaps
    let r2_segs = step(0.25, abs(sin((angle + r2_rot) * 3.0)));
    let r2_val = smoothstep(r2_w, 0.001, abs(dist - r2_r)) * r2_segs;

    color += col_magenta * r2_val;
    alpha = max(alpha, r2_val);

    // --- Ring 3: Inner fast ring — glyph details ---
    let r3_reveal = ease_out_expo(clamp((time - 0.5) / 0.6, 0.0, 1.0));
    let r3_r = 0.165 * r3_reveal;
    let r3_w = 0.010;
    let r3_rot = time * 0.75;
    // Binary glyph pattern (bold dashes)
    let r3_glyph = step(0.6, sin((angle + r3_rot) * 16.0) * sin((angle - r3_rot * 1.5) * 5.0) + 0.5);
    let r3_val = smoothstep(r3_w, 0.002, abs(dist - r3_r)) * r3_glyph;

    color += col_yellow * r3_val;
    alpha = max(alpha, r3_val);

    // --- Center: Bold + pulsing ---
    let c_reveal = ease_out_back(clamp((time - 0.85) / 0.45, 0.0, 1.0));
    // Starburst core
    let c_star_r = 0.042 * c_reveal * (1.0 + 0.12 * sin(time * 4.0));
    let c_core = smoothstep(c_star_r, 0.0, dist);
    color += col_white * c_core;
    alpha = max(alpha, c_core);

    // Inner ring pulse
    let pulse_r = 0.055 * c_reveal + 0.01 * sin(time * 6.0) * c_reveal;
    let pulse_ring = smoothstep(0.004, 0.0, abs(dist - pulse_r)) * c_reveal;
    color += col_cyan * pulse_ring * 0.8;
    alpha = max(alpha, pulse_ring * 0.8);

    // --- Anamorphic streak (Y2K lens flare) — every 3s ---
    let flash_t = fract(time / 3.0);
    let flash = smoothstep(0.0, 0.08, flash_t) * smoothstep(0.22, 0.08, flash_t);
    let streak_w = 0.0018;
    let streak = smoothstep(streak_w, 0.0, abs(p.x)) * smoothstep(0.55, 0.0, abs(p.y)) * flash;
    // Horizontal anamorphic flare
    let hstreak = smoothstep(streak_w * 3.0, 0.0, abs(p.y)) * smoothstep(0.65, 0.0, abs(p.x)) * flash * 0.5;
    color += col_white * (streak + hstreak);
    alpha = max(alpha, (streak + hstreak));

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time / 60.0;
    let aspect = u.resolution.x / u.resolution.y;
    let uv_c = uv - 0.5;

    // VHS channel separation — slow ambient + event pulses
    // Events: big reveal at 0s, flash every 3s
    let flash_t = fract(time / 3.0);
    let flash = smoothstep(0.0, 0.08, flash_t) * smoothstep(0.22, 0.08, flash_t);
    let reveal_pulse = ease_out_expo(clamp(time / 0.6, 0.0, 1.0)) * (1.0 - ease_out_expo(clamp((time - 0.6) / 0.8, 0.0, 1.0)));

    let event = max(flash, reveal_pulse);
    let ca = 0.003 + event * 0.014 + 0.0015 * sin(time * 3.5);

    // Primarily horizontal VHS split
    let off_r = vec2<f32>( ca, ca * 0.3);
    let off_b = vec2<f32>(-ca, -ca * 0.3);

    let s_r = rings_at(uv_c + off_r, aspect, time);
    let s_g = rings_at(uv_c, aspect, time);
    let s_b = rings_at(uv_c + off_b, aspect, time);

    // Compose RGB channels — center alpha for clean compositing
    let out_col = vec3<f32>(s_r.r, s_g.g, s_b.b);
    let out_a = s_g.a;

    return vec4<f32>(out_col, out_a);
}
