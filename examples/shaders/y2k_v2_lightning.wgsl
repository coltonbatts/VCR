// Y2K Lightning — modern palette: gold, chrome, rose, slate
// Triangle-based spike center star. u.time = SECONDS.

fn ease_out_expo(x: f32) -> f32 {
    if (x >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * x);
}
fn ease_out_back(x: f32) -> f32 {
    let c = 1.70158; let t1 = x - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}
fn hash11(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

fn sdSeg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a; let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
    return length(pa - ba * h);
}

fn triSDF(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let e0 = b - a; let e1 = c - b; let e2 = a - c;
    let v0 = p - a; let v1 = p - b; let v2 = p - c;
    let d0 = dot(v0, vec2<f32>(e0.y, -e0.x));
    let d1 = dot(v1, vec2<f32>(e1.y, -e1.x));
    let d2 = dot(v2, vec2<f32>(e2.y, -e2.x));
    let inside = min(min(d0, d1), d2);
    let d_ab = sdSeg(p, a, b);
    let d_bc = sdSeg(p, b, c);
    let d_ca = sdSeg(p, c, a);
    return select(min(min(d_ab, d_bc), d_ca), -min(min(d_ab, d_bc), d_ca), inside >= 0.0);
}

fn spikeVal(p: vec2<f32>, ang: f32, outer_r: f32, inner_r: f32, half_w: f32) -> f32 {
    let tip = vec2<f32>(cos(ang), sin(ang)) * outer_r;
    let bl  = vec2<f32>(cos(ang - half_w), sin(ang - half_w)) * inner_r;
    let br  = vec2<f32>(cos(ang + half_w), sin(ang + half_w)) * inner_r;
    return smoothstep(0.006, -0.006, triSDF(p, tip, bl, br));
}

fn lightning_col(uv: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dp = length(p);
    let angle = atan2(p.y, p.x);
    let pi = 3.14159265;

    var col = vec3<f32>(0.0);
    var alpha = 0.0;

    let gold   = vec3<f32>(0.95, 0.78, 0.35);
    let chrome = vec3<f32>(0.88, 0.90, 0.95);
    let rose   = vec3<f32>(0.85, 0.40, 0.55);
    let slate  = vec3<f32>(0.35, 0.52, 0.75);
    let white  = vec3<f32>(1.0,  1.0,  1.0);

    let global_rev = ease_out_expo(clamp(t / 0.5, 0.0, 1.0));

    // ── 8 primary lightning bolts (zigzag segments) ───────────────────
    for (var i = 0; i < 8; i++) {
        let fi = f32(i);
        let bolt_rev = ease_out_back(clamp((t - fi * 0.055) / 0.32, 0.0, 1.0));
        let base_ang = fi / 8.0 * 2.0 * pi;
        var prev = vec2<f32>(0.0, 0.0);
        let bolt_len = 0.44 * bolt_rev;
        for (var j = 0; j < 5; j++) {
            let fj = f32(j);
            let r0 = (fj / 5.0) * bolt_len;
            let r1 = ((fj + 1.0) / 5.0) * bolt_len;
            let jitter_amp = 0.042 * (1.0 - fj / 5.0);
            let jitter_seed = fi * 100.0 + fj + floor(t * 8.0);
            let jit0 = (hash11(jitter_seed) - 0.5) * jitter_amp;
            let jit1 = (hash11(jitter_seed + 1.0) - 0.5) * jitter_amp;
            let perp = base_ang + 1.5708;
            let a = vec2<f32>(cos(base_ang), sin(base_ang)) * r0 + vec2<f32>(cos(perp), sin(perp)) * jit0;
            let b = vec2<f32>(cos(base_ang), sin(base_ang)) * r1 + vec2<f32>(cos(perp), sin(perp)) * jit1;
            let d_seg = sdSeg(p, a, b);
            let bolt_c = select(chrome, gold, i % 2 == 0);
            let seg_val = smoothstep(0.004, 0.0, d_seg) * bolt_rev;
            col += bolt_c * seg_val;
            alpha = max(alpha, seg_val);
            let glow_seg = exp(-d_seg * 45.0) * 0.18 * bolt_rev;
            col += bolt_c * glow_seg;
            alpha = max(alpha, glow_seg * 0.7);
        }
    }

    // ── Outer charge ring (slate, randomly lit arcs) ───────────────────
    let ring_rev = ease_out_expo(clamp((t - 0.30) / 0.4, 0.0, 1.0));
    let ring_R = 0.42 * ring_rev;
    let arc_seed = floor(t * 5.0);
    let arc_mask = step(0.38, hash11(angle * 8.0 + arc_seed * 7.13));
    let ring = smoothstep(0.005, 0.0, abs(dp - ring_R)) * arc_mask * ring_rev;
    col += slate * ring;
    alpha = max(alpha, ring);

    // ── Inner 6-point star — TRIANGLE-BASED, sharp ────────────────────
    let star_rev = ease_out_back(clamp((t - 0.20) / 0.4, 0.0, 1.0));
    let star_rot = t * 1.5;
    let outer6 = 0.10 * star_rev;
    let inner6 = 0.018 * star_rev;
    let hw6 = 0.28; // half-width: narrow = sharp spikes
    for (var i = 0; i < 6; i++) {
        let ang = f32(i) * (pi / 3.0) + star_rot;
        let sv = spikeVal(p, ang, outer6, inner6, hw6);
        col = mix(col, white, sv);
        alpha = max(alpha, sv);
    }
    // Spike glow
    let star_glow = exp(-dp * 20.0) * 0.55 * star_rev;
    col += chrome * star_glow;
    alpha = max(alpha, star_glow * 0.6);

    // ── Charge nodes at perimeter (6, orbiting) ───────────────────────
    let node_rev = ease_out_back(clamp((t - 0.4) / 0.35, 0.0, 1.0));
    for (var i = 0; i < 6; i++) {
        let fi = f32(i);
        let node_ang = fi / 6.0 * 2.0 * pi + t * 0.33;
        let node_pos = vec2<f32>(cos(node_ang), sin(node_ang)) * 0.40 * node_rev;
        let d_node = length(p - node_pos);
        let node_pulse = 1.0 + 0.28 * sin(t * 6.0 + fi * 1.047);
        let node_r = 0.026 * node_rev * node_pulse;
        let node_val = smoothstep(node_r, 0.0, d_node);
        let n_col = select(rose, gold, i % 3 == 0);
        col += n_col * node_val;
        alpha = max(alpha, node_val);
        // Arc to center (flicker)
        let d_arc = sdSeg(p, vec2<f32>(0.0), node_pos);
        let arc_flicker = step(0.5, hash11(fi + floor(t * 10.0) * 0.41));
        let arc_val = smoothstep(0.003, 0.0, d_arc) * arc_flicker * node_rev * 0.45;
        col += n_col * arc_val;
        alpha = max(alpha, arc_val);
    }

    // ── Core ─────────────────────────────────────────────────────────
    let core_pulse = 1.0 + 0.22 * sin(t * 8.5);
    let core = smoothstep(0.040 * global_rev * core_pulse, 0.0, dp);
    col = mix(col, white, core);
    alpha = max(alpha, core);
    col += white * exp(-dp * 5.5) * 0.22 * global_rev;
    alpha = max(alpha, exp(-dp * 5.5) * 0.22 * global_rev * 0.6);

    alpha = clamp(alpha, 0.0, 1.0);
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let t = u.time;
    let aspect = u.resolution.x / u.resolution.y;
    let rev = ease_out_expo(clamp(t / 0.5, 0.0, 1.0));
    let ca = 0.0022 + rev * 0.010 * max(0.0, 1.0 - t) + 0.0009 * sin(t * 6.3);
    let off = vec2<f32>(ca / aspect, 0.0);
    let sr = lightning_col(uv + off, aspect, t);
    let sg = lightning_col(uv,       aspect, t);
    let sb = lightning_col(uv - off, aspect, t);
    return vec4<f32>(sr.r, sg.g, sb.b, sg.a);
}
