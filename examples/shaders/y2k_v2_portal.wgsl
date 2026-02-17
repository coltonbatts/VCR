// Y2K Bold — Portal / Eye of the Machine
// Concentric bold rings with a glowing liquid center, sweeping scan beam,
// outer data crown. Full-frame. u.time = SECONDS.

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}
fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158; let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn hash(v: f32) -> f32 {
    return fract(sin(v * 127.1) * 43758.5453);
}

fn portal_col(uv: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dp = length(p);
    let angle = atan2(p.y, p.x);

    var col = vec3<f32>(0.0);
    var alpha = 0.0;

    // Modern palette
    let cyan    = vec3<f32>(0.35, 0.52, 0.75);  // slate
    let magenta = vec3<f32>(0.85, 0.40, 0.55);  // rose
    let yellow  = vec3<f32>(0.95, 0.78, 0.35);  // gold
    let white   = vec3<f32>(1.0,  1.0,  1.0);

    // ── Ring 1: Outer crown — tick marks ─────────────────────────────
    let r1_rev = ease_out_expo(clamp(t / 0.45, 0.0, 1.0));
    let r1_R = 0.43 * r1_rev;
    let r1_rot = t * 0.15;
    let tick1 = step(0.90, sin((angle + r1_rot) * 48.0));
    let arc1  = step(0.30, sin((angle + r1_rot * 2.0) * 6.0)); // 6 major arcs
    let r1 = smoothstep(0.0025, 0.0, abs(dp - r1_R)) * (arc1 * 0.5 + tick1 * 0.8) * r1_rev;
    col += cyan * r1;
    alpha = max(alpha, r1);

    // ── Ring 2: Bold segmented (4 segments) ──────────────────────────
    let r2_rev = ease_out_back(clamp((t - 0.3) / 0.45, 0.0, 1.0));
    let r2_R = 0.36 * r2_rev;
    let r2_rot = -t * 0.38;
    let seg2 = step(0.22, abs(sin((angle + r2_rot) * 2.0)));
    let r2 = smoothstep(0.010, 0.001, abs(dp - r2_R)) * seg2 * r2_rev;
    col += magenta * r2;
    alpha = max(alpha, r2);

    // ── Ring 3: Inner fast ────────────────────────────────────────────
    let r3_rev = ease_out_expo(clamp((t - 0.5) / 0.4, 0.0, 1.0));
    let r3_R = 0.27 * r3_rev;
    let r3_rot = t * 0.9;
    let glyph3 = step(0.55, sin((angle + r3_rot) * 8.0) * sin((angle - r3_rot) * 3.0) + 0.5);
    let r3 = smoothstep(0.008, 0.0, abs(dp - r3_R)) * glyph3 * r3_rev;
    col += yellow * r3;
    alpha = max(alpha, r3);

    // ── Ring 4: Innermost pulse ───────────────────────────────────────
    let r4_rev = ease_out_back(clamp((t - 0.65) / 0.35, 0.0, 1.0));
    let r4_R = 0.18 * r4_rev * (1.0 + 0.06 * sin(t * 3.5));
    let r4 = smoothstep(0.010, 0.0, abs(dp - r4_R)) * r4_rev;
    col += cyan * r4;
    alpha = max(alpha, r4);

    // ── Liquid center fill ────────────────────────────────────────────
    let fill_rev = ease_out_expo(clamp((t - 0.55) / 0.4, 0.0, 1.0));
    let fill_R = 0.175 * fill_rev;
    let fill = smoothstep(fill_R, fill_R - 0.012, dp);
    if (fill > 0.0) {
        // Swirling plasma inside
        let swirl_ang = angle + dp * 4.0 + t * 1.8;
        let plasma = sin(swirl_ang * 3.0) * 0.5 + 0.5;
        let plasma2 = sin(swirl_ang * 5.0 - t * 2.5) * 0.5 + 0.5;
        let plasma_col = mix(vec3<f32>(0.85, 0.40, 0.55), vec3<f32>(0.88, 0.90, 0.95), plasma * plasma2);
        col += plasma_col * fill;
        alpha = max(alpha, fill);
    }

    // ── Scan beam (rotating bright line) ─────────────────────────────
    let scan_rev = ease_out_expo(clamp((t - 0.4) / 0.5, 0.0, 1.0));
    let scan_ang = t * 1.5;
    let scan_diff = abs(fract((angle - scan_ang) / 6.2832) - 0.5) * 2.0;
    let scan_w = 0.04;
    let scan_beam = smoothstep(scan_w, 0.0, scan_diff * dp) * smoothstep(0.44, 0.0, dp) * smoothstep(0.0, 0.05, dp) * scan_rev;
    col += white * scan_beam * 0.9;
    alpha = max(alpha, scan_beam * 0.85);

    // ── Anamorphic lens flare on scan ────────────────────────────────
    let flare_t = fract(t * 1.5 / 6.2832);
    let flare = smoothstep(0.96, 1.0, cos(t * 1.5)) * scan_rev;
    let streak_h = smoothstep(0.0025, 0.0, abs(p.y)) * smoothstep(0.5, 0.0, abs(p.x)) * flare;
    let streak_v = smoothstep(0.0025, 0.0, abs(p.x)) * smoothstep(0.5, 0.0, abs(p.y)) * flare * 0.4;
    col += white * (streak_h + streak_v);
    alpha = max(alpha, (streak_h + streak_v));

    // ── Glow ─────────────────────────────────────────────────────────
    let glow = exp(-max(dp - 0.44, 0.0) * 15.0) * 0.2 * ease_out_expo(clamp(t / 0.6, 0.0, 1.0));
    col += mix(magenta, cyan, clamp(angle / 6.2832 + 0.5, 0.0, 1.0)) * glow;
    alpha = max(alpha, glow * 0.6);

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(col, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let t = u.time;
    let aspect = u.resolution.x / u.resolution.y;
    let ca = 0.0025 + 0.0015 * sin(t * 3.8);
    let off = vec2<f32>(ca / aspect, 0.0);
    let sr = portal_col(uv + off, aspect, t);
    let sg = portal_col(uv,       aspect, t);
    let sb = portal_col(uv - off, aspect, t);
    return vec4<f32>(sr.r, sg.g, sb.b, sg.a);
}
