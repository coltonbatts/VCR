// Y2K Bold — DNA Helix / Twisted Infinity
// Two intertwined helical strands with glowing nodes, connecting cross-bars,
// orbiting particles. Tall vertical element filling the frame.
// u.time = SECONDS.

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}
fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158; let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn helix_col(uv: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);

    var col = vec3<f32>(0.0);
    var alpha = 0.0;

    // Modern palette
    let cyan    = vec3<f32>(0.35, 0.52, 0.75);  // slate
    let magenta = vec3<f32>(0.85, 0.40, 0.55);  // rose
    let yellow  = vec3<f32>(0.95, 0.78, 0.35);  // gold
    let white   = vec3<f32>(1.0,  1.0,  1.0);

    let global_rev = ease_out_expo(clamp(t / 0.8, 0.0, 1.0));

    // Helix parameters: vertical extent [-0.44, 0.44], helix radius 0.22
    let helix_h = 0.44; // half-height
    let helix_r = 0.20; // strand radius
    let freq = 3.5;      // coil frequency
    let spin = t * 0.5;  // rotation speed

    // Sample helix strands as tubes: for each Y position, find closest strand point
    // Strand A: phase 0, Strand B: phase PI
    let N_seg = 32; // segments to check

    for (var i = 0; i < N_seg; i++) {
        let fi = f32(i);
        let y_seg = mix(-helix_h, helix_h, fi / f32(N_seg - 1));
        let seg_rev = ease_out_back(clamp((t - fi / f32(N_seg - 1) * 0.6) / 0.35, 0.0, 1.0));

        // Strand A
        let ang_a = y_seg * freq * 6.2832 / (helix_h * 2.0) + spin;
        let xa = helix_r * cos(ang_a) * global_rev * seg_rev;
        let node_a = vec2<f32>(xa, y_seg);
        let dist_a = length(p - node_a);
        let node_size_a = 0.012 * seg_rev * global_rev;
        let node_a_val = smoothstep(node_size_a, 0.0, dist_a);

        // Strand B (opposite phase)
        let ang_b = ang_a + 3.1416;
        let xb = helix_r * cos(ang_b) * global_rev * seg_rev;
        let node_b = vec2<f32>(xb, y_seg);
        let dist_b = length(p - node_b);
        let node_b_val = smoothstep(node_size_a, 0.0, dist_b);

        // Strand colors alternate
        let hue_a = fi / f32(N_seg);
        let strand_col_a = mix(cyan, magenta, hue_a);
        let strand_col_b = mix(magenta, yellow, hue_a);

        col += strand_col_a * node_a_val;
        alpha = max(alpha, node_a_val);
        col += strand_col_b * node_b_val;
        alpha = max(alpha, node_b_val);

        // Cross-bars connecting strands every ~4 segments
        if (i % 4 == 0 && seg_rev > 0.01) {
            let bar_start = node_a;
            let bar_end   = node_b;
            let ba = bar_end - bar_start;
            let pa2 = p - bar_start;
            let h = clamp(dot(pa2, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
            let d_bar = length(pa2 - ba * h);
            let bar_val = smoothstep(0.006, 0.0, d_bar) * seg_rev * global_rev;
            col += yellow * bar_val * 0.7;
            alpha = max(alpha, bar_val * 0.7);
        }
    }

    // ── Backbone glow tubes (approximate as thick smear) ─────────────
    // Check distance from p to the sine-wave path of each strand
    let y_clamped = clamp(p.y, -helix_h, helix_h);
    let ang_a_near = y_clamped * freq * 6.2832 / (helix_h * 2.0) + spin;
    let xa_near = helix_r * cos(ang_a_near) * global_rev;
    let ang_b_near = ang_a_near + 3.1416;
    let xb_near = helix_r * cos(ang_b_near) * global_rev;

    let d_strand_a = length(p - vec2<f32>(xa_near, y_clamped));
    let d_strand_b = length(p - vec2<f32>(xb_near, y_clamped));

    let glow_a = exp(-d_strand_a * 35.0) * 0.25 * global_rev * smoothstep(helix_h + 0.05, helix_h - 0.05, abs(p.y));
    let glow_b = exp(-d_strand_b * 35.0) * 0.25 * global_rev * smoothstep(helix_h + 0.05, helix_h - 0.05, abs(p.y));
    col += cyan    * glow_a;
    col += magenta * glow_b;
    alpha = max(alpha, (glow_a + glow_b) * 0.8);

    // ── Outer ring at equator ─────────────────────────────────────────
    let ring_rev = ease_out_expo(clamp((t - 0.5) / 0.4, 0.0, 1.0));
    let ring_R = 0.26 * ring_rev;
    let ring = smoothstep(0.007, 0.0, abs(length(p) - ring_R)) * ring_rev;
    col += cyan * ring * 0.6;
    alpha = max(alpha, ring * 0.6);

    // ── Top and bottom cap glows ──────────────────────────────────────
    let cap_rev = ease_out_expo(clamp((t - 0.6) / 0.35, 0.0, 1.0));
    let cap_t = exp(-length(p - vec2<f32>(0.0,  helix_h)) * 18.0) * 0.5 * cap_rev;
    let cap_b = exp(-length(p - vec2<f32>(0.0, -helix_h)) * 18.0) * 0.5 * cap_rev;
    col += white * cap_t;
    col += white * cap_b;
    alpha = max(alpha, (cap_t + cap_b) * 0.9);

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(col, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let t = u.time;
    let aspect = u.resolution.x / u.resolution.y;
    let ca = 0.0018 + 0.001 * sin(t * 5.1);
    let off = vec2<f32>(ca / aspect, 0.0);
    let sr = helix_col(uv + off, aspect, t);
    let sg = helix_col(uv,       aspect, t);
    let sb = helix_col(uv - off, aspect, t);
    return vec4<f32>(sr.r, sg.g, sb.b, sg.a);
}
