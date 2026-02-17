// Y2K Bold — Orbiting Dots Element
// Bold modern Y2K: saturated hue-shifted particles, VHS RGB trail separation, clean alpha
// 60fps :: time = u.time / 60.0

fn ease_out_elastic(t: f32) -> f32 {
    if (t <= 0.0) { return 0.0; }
    if (t >= 1.0) { return 1.0; }
    return pow(2.0, -10.0 * t) * sin((t - 0.075) * 6.283185 / 0.3) + 1.0;
}

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if (t < 0.5) { return 4.0 * t * t * t; }
    let f = 2.0 * t - 2.0;
    return 0.5 * f * f * f + 1.0;
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

struct DotState {
    pos: vec2<f32>,
    col: vec3<f32>,
    reveal: f32,
    size: f32,
}

fn get_dot(i: i32, time: f32) -> DotState {
    let fi = f32(i);
    let reveal = ease_out_elastic(clamp((time - fi * 0.08) / 0.7, 0.0, 1.0));

    let orbit_base = 0.11 + fi * 0.021;
    let orbit_breath = 1.0 + 0.10 * sin(time * 1.3 + fi * 0.75);
    let orbit_r = orbit_base * reveal * orbit_breath;

    let speed_base = 1.6 - fi * 0.06;
    let dir = select(1.0, -1.0, i % 2 == 0);
    let angle = time * speed_base * dir + fi * 0.5236;

    let pos = vec2<f32>(cos(angle), sin(angle)) * orbit_r;

    // Y2K: highly saturated hue cycling — bold, no pastels
    let hue = fract(fi * 0.083 + time * 0.08);
    // Force towards saturated: cyan, magenta, yellow band
    let h6 = hue * 6.0;
    var col = vec3<f32>(0.0);
    if (h6 < 1.0) { col = vec3<f32>(1.0, h6, 0.0); }
    else if (h6 < 2.0) { col = vec3<f32>(2.0 - h6, 1.0, 0.0); }
    else if (h6 < 3.0) { col = vec3<f32>(0.0, 1.0, h6 - 2.0); }
    else if (h6 < 4.0) { col = vec3<f32>(0.0, 4.0 - h6, 1.0); }
    else if (h6 < 5.0) { col = vec3<f32>(h6 - 4.0, 0.0, 1.0); }
    else { col = vec3<f32>(1.0, 0.0, 6.0 - h6); }
    // Clamp to keep saturated only — min channel = 0 (no desaturation)
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));

    let dot_size = (0.009 + 0.0035 * sin(time * 2.8 + fi)) * reveal;

    var s: DotState;
    s.pos = pos;
    s.col = col;
    s.reveal = reveal;
    s.size = dot_size;
    return s;
}

fn dots_layer(uv_c: vec2<f32>, aspect: f32, time: f32) -> vec4<f32> {
    let breath = 1.0 + 0.025 * sin(time * 0.75);
    let p = uv_c * vec2<f32>(aspect, 1.0) / breath;

    var color = vec3<f32>(0.0);
    var alpha = 0.0;

    // Pre-compute dot positions
    var positions = array<vec2<f32>, 12>();
    var reveals   = array<f32, 12>();

    for (var i = 0; i < 12; i++) {
        let s = get_dot(i, time);
        positions[i] = s.pos;
        reveals[i]   = s.reveal;
    }

    // Render dots + constellation lines
    for (var i = 0; i < 12; i++) {
        let s = get_dot(i, time);
        if (s.reveal < 0.01) { continue; }

        let d_dot = length(p - s.pos);
        let core = smoothstep(s.size, 0.0, d_dot);
        // Bold hard-edged glow ring (Y2K: no soft gradients, use halos)
        let halo_r = s.size * 2.5;
        let halo = smoothstep(halo_r, s.size, d_dot) * 0.35 * s.reveal;

        color += s.col * (core * 1.8 + halo);
        alpha = max(alpha, core + halo * 0.5);

        // Connection lines — only nearby dots
        let prev_i = (i + 11) % 12;
        let prev_reveal = reveals[prev_i];
        if (prev_reveal > 0.01) {
            let prev_pos = positions[prev_i];
            let pa = p - s.pos;
            let ba = prev_pos - s.pos;
            let h = clamp(dot(pa, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
            let d_line = length(pa - ba * h);

            let dist_dots = length(s.pos - prev_pos);
            let conn = smoothstep(0.28, 0.10, dist_dots) * s.reveal * prev_reveal;
            let line_w = 0.0012;
            let lv = smoothstep(line_w, 0.0, d_line) * conn * 0.5;
            // White lines for bold contrast
            color += vec3<f32>(1.0, 1.0, 1.0) * lv;
            alpha = max(alpha, lv);
        }
    }

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time / 60.0;
    let aspect = u.resolution.x / u.resolution.y;
    let uv_c = uv - 0.5;

    // VHS separation: increases on elastic reveals, pulses at beat
    // Dot 0 reveals at t=0, last dot at ~0.88s
    let reveal_drive = ease_out_expo(clamp(time / 1.0, 0.0, 1.0))
        * (1.0 - ease_out_expo(clamp((time - 1.0) / 1.5, 0.0, 1.0)));
    // Beat pulse every 1.5s
    let beat_t = fract(time / 1.5);
    let beat = smoothstep(0.0, 0.06, beat_t) * smoothstep(0.18, 0.06, beat_t);

    let event = max(reveal_drive * 0.7, beat);
    let ca = 0.0025 + event * 0.012 + 0.001 * sin(time * 5.1);

    // Angular VHS offset — diagonal for kinetic feel
    let off_r = vec2<f32>( ca,  ca * 0.5);
    let off_b = vec2<f32>(-ca, -ca * 0.5);

    let s_r = dots_layer(uv_c + off_r, aspect, time);
    let s_g = dots_layer(uv_c, aspect, time);
    let s_b = dots_layer(uv_c + off_b, aspect, time);

    let out_col = vec3<f32>(s_r.r, s_g.g, s_b.b);
    let out_a   = s_g.a;

    return vec4<f32>(out_col, out_a);
}
