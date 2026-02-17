// Y2K Bold — Hex Sigil Element
// Bold modern Y2K: neon geometry, VHS glitch burst on transitions, clean alpha
// 60fps :: time = u.time / 60.0

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}

fn ease_out_back(t: f32) -> f32 {
    let c = 2.0;
    let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if (t < 0.5) { return 4.0 * t * t * t; }
    let f = 2.0 * t - 2.0;
    return 0.5 * f * f * f + 1.0;
}

fn sdHexagon(p: vec2<f32>, r: f32) -> f32 {
    let k = vec3<f32>(-0.866025404, 0.5, 0.577350269);
    var p2 = abs(p);
    p2 -= 2.0 * min(dot(k.xy, p2), 0.0) * k.xy;
    p2 -= vec2<f32>(clamp(p2.x, -k.z * r, k.z * r), r);
    return length(p2) * sign(p2.y);
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn sigil_at(uv_c: vec2<f32>, aspect: f32, time: f32) -> vec4<f32> {
    // Breathing (slow, confident)
    let breath = 1.0 + 0.035 * sin(time * 0.65) + 0.015 * sin(time * 1.85);
    let p = uv_c * vec2<f32>(aspect, 1.0) / breath;
    let dist = length(p);
    let angle = atan2(p.y, p.x);

    var color = vec3<f32>(0.0);
    var alpha = 0.0;

    // Y2K Palette
    let col_neon_green  = vec3<f32>(0.05, 1.0,  0.35); // Matrix green
    let col_electric    = vec3<f32>(0.0,  0.85, 1.0);  // Electric cyan
    let col_hot_magenta = vec3<f32>(1.0,  0.0,  0.65); // Hot magenta
    let col_white       = vec3<f32>(1.0,  1.0,  1.0);
    let col_yellow      = vec3<f32>(1.0,  0.92, 0.0);

    // --- Outer ring: wipe in, arc segments ---
    let ring_reveal = ease_out_expo(clamp(time / 0.8, 0.0, 1.0));
    let outer_r = 0.335 * ring_reveal;
    let outer_w = 0.0045;
    // 4 bold arcs with clear gaps
    let arc4 = step(0.3, sin(angle * 2.0 + time * 0.22));
    let outer = smoothstep(outer_w, 0.0, abs(dist - outer_r)) * arc4;
    color += col_electric * outer;
    alpha = max(alpha, outer * 0.9);

    // Tick marks on outer ring
    let ticks = step(0.92, sin((angle + time * 0.3) * 36.0));
    let outer_ticks = smoothstep(0.0015, 0.0, abs(dist - outer_r)) * ticks * ring_reveal;
    color += col_white * outer_ticks * 0.7;
    alpha = max(alpha, outer_ticks * 0.7);

    // --- Hexagon: bold stroke, fast spin-in then slow drift ---
    let hex_reveal = ease_out_back(clamp((time - 0.35) / 0.65, 0.0, 1.0));
    let hex_r = 0.235 * hex_reveal;
    let hex_rot_speed = mix(4.5, 0.15, ease_out_expo(clamp(time / 2.5, 0.0, 1.0)));
    let hex_angle = time * hex_rot_speed;
    let rot_c = cos(hex_angle);
    let rot_s = sin(hex_angle);
    let p_hex = mat2x2<f32>(rot_c, -rot_s, rot_s, rot_c) * p;

    let d_hex = sdHexagon(p_hex, hex_r);
    // Bold hex edge (thicker than before)
    let hex_stroke_w = 0.007;
    let hex_edge = smoothstep(hex_stroke_w, 0.0, abs(d_hex));
    color += col_hot_magenta * hex_edge;
    alpha = max(alpha, hex_edge);

    // Circuit grid fill inside hex (Y2K grid aesthetic)
    let inside_hex = step(d_hex, 0.0);
    let grid_x = step(0.85, sin(p_hex.x * 48.0));
    let grid_y = step(0.85, sin(p_hex.y * 48.0));
    let grid = max(grid_x, grid_y);
    let hex_fill = inside_hex * grid * 0.25 * hex_reveal;
    color += col_neon_green * hex_fill;
    alpha = max(alpha, hex_fill);

    // --- Triangle: neon outline, counter-rotate ---
    let tri_reveal = ease_out_expo(clamp((time - 0.7) / 0.55, 0.0, 1.0));
    let tri_r = 0.155 * tri_reveal;
    let tri_angle_off = time * -0.4;
    for (var i = 0; i < 3; i++) {
        let a1 = f32(i) * 2.094395 + tri_angle_off;
        let a2 = f32(i + 1) * 2.094395 + tri_angle_off;
        let tp1 = vec2<f32>(cos(a1), sin(a1)) * tri_r;
        let tp2 = vec2<f32>(cos(a2), sin(a2)) * tri_r;

        let pa = p - tp1;
        let ba = tp2 - tp1;
        let h = clamp(dot(pa, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
        let d_line = length(pa - ba * h);
        let line_val = smoothstep(0.004, 0.0, d_line) * tri_reveal;

        color += col_yellow * line_val;
        alpha = max(alpha, line_val);
    }

    // --- Center core: pulsing chrome dot ---
    let dot_reveal = ease_out_back(clamp((time - 1.1) / 0.4, 0.0, 1.0));
    // 4-point star shape
    let star_r = 0.028 * dot_reveal * (1.0 + 0.15 * sin(time * 5.5));
    let star_d = dist - star_r * (1.0 + 0.4 * abs(cos(angle * 4.0)));
    let center = smoothstep(0.006, 0.0, star_d);
    color += col_white * center;
    alpha = max(alpha, center);

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time / 60.0;
    let aspect = u.resolution.x / u.resolution.y;
    let uv_c = uv - 0.5;

    // VHS glitch: big burst on reveal events, subtle ambient
    // Hex spin-in is the major event (0.35–1.0s)
    let spin_event = ease_out_expo(clamp((time - 0.35) / 0.5, 0.0, 1.0))
        * (1.0 - ease_out_expo(clamp((time - 0.85) / 0.6, 0.0, 1.0)));
    // Pulse every 2s
    let pulse_t = fract(time / 2.0);
    let pulse = smoothstep(0.0, 0.06, pulse_t) * smoothstep(0.2, 0.06, pulse_t);

    let event = max(spin_event * 0.8, pulse);
    let ca = 0.003 + event * 0.016 + 0.0012 * sin(time * 4.2);

    // Horizontal primary offset + slight vertical skew for VHS tape feel
    let off_r = vec2<f32>( ca,  ca * 0.2);
    let off_b = vec2<f32>(-ca, -ca * 0.2);

    let s_r = sigil_at(uv_c + off_r, aspect, time);
    let s_g = sigil_at(uv_c, aspect, time);
    let s_b = sigil_at(uv_c + off_b, aspect, time);

    let out_col = vec3<f32>(s_r.r, s_g.g, s_b.b);
    let out_a   = s_g.a;

    return vec4<f32>(out_col, out_a);
}
