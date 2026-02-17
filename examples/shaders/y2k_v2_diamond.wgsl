// Y2K Diamond — modern palette: warm gold, chrome, slate, rose
// Triangle-based spike geometry, contained geometry.
// u.time = SECONDS. p = (uv-0.5)*vec2(aspect,1.0).

fn ease_out_expo(x: f32) -> f32 {
    if (x >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * x);
}
fn ease_out_back(x: f32) -> f32 {
    let c = 1.70158; let t1 = x - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn sdSeg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a; let ap = p - a;
    let h = clamp(dot(ap, ab) / dot(ab, ab), 0.0, 1.0);
    return length(ap - ab * h);
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

fn sdPolygon(p: vec2<f32>, n: f32, r: f32) -> f32 {
    let angle = 6.2832 / n;
    let a = atan2(p.y, p.x) + 3.1416 / n;
    let ai = floor(a / angle) * angle;
    let c = cos(ai - 3.1416 / n); let s = sin(ai - 3.1416 / n);
    let q = vec2<f32>(c * p.x + s * p.y, -s * p.x + c * p.y);
    return q.x - r;
}

fn diamond_col(uv: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
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

    let global_rev = ease_out_back(clamp(t / 0.55, 0.0, 1.0));

    // ── Outer tick ring ───────────────────────────────────────────────
    let orb_rev = ease_out_expo(clamp((t - 0.35) / 0.35, 0.0, 1.0));
    let orb_R = 0.41 * orb_rev;
    let orb_rot = t * 0.22;
    let orb_tick = step(0.82, sin((angle + orb_rot) * 16.0));
    let orb = smoothstep(0.004, 0.0, abs(dp - orb_R)) * orb_tick * orb_rev;
    col += slate * orb;
    alpha = max(alpha, orb);

    // ── Outer diamond outline (|x|+|y| SDF, slow rotation) ───────────
    // Diamond = L1-ball: |rotated.x| + |rotated.y| = S
    let rot1 = t * 0.15 + 0.7854;
    let cs1 = cos(rot1); let ss1 = sin(rot1);
    let rp1 = mat2x2<f32>(cs1, -ss1, ss1, cs1) * p;
    let scale1 = 0.32 * global_rev;
    let d1_sdf = (abs(rp1.x) + abs(rp1.y)) - scale1;
    // Stroke outline
    let d1_stroke = smoothstep(0.010, 0.001, abs(d1_sdf));
    let d1_grad = clamp(rp1.y / max(scale1, 0.001) * 0.5 + 0.5, 0.0, 1.0);
    col += mix(chrome, gold, d1_grad) * d1_stroke;
    alpha = max(alpha, d1_stroke);
    // 4 spike tips (narrow needle at each corner)
    for (var i = 0; i < 4; i++) {
        let tip_ang = f32(i) * (pi * 0.5) + rot1;
        let sv = spikeVal(p, tip_ang, scale1 * 1.15, scale1 * 0.90, 0.12);
        col += gold * sv;
        alpha = max(alpha, sv);
    }
    // Facet fill
    let fill_rev = ease_out_expo(clamp((t - 0.15) / 0.4, 0.0, 1.0));
    let d1_inside = step(d1_sdf, 0.0) * fill_rev;
    let facet_a = angle + t * 0.07;
    let facet_mask = smoothstep(0.010, 0.001, abs(fract(facet_a * 8.0 / 6.2832) - 0.5) - 0.44);
    col += slate * facet_mask * d1_inside * 0.30;
    alpha = max(alpha, facet_mask * d1_inside * 0.30);

    // ── Middle diamond outline (counter-rotate, rose) ─────────────────
    let d2_rot = -t * 0.38 + 0.0;  // axis-aligned initially, then rotate
    let cs2 = cos(d2_rot); let ss2 = sin(d2_rot);
    let rp2 = mat2x2<f32>(cs2, -ss2, ss2, cs2) * p;
    let scale2 = 0.22 * ease_out_expo(clamp((t - 0.18) / 0.38, 0.0, 1.0));
    let d2_sdf = (abs(rp2.x) + abs(rp2.y)) - scale2;
    let d2_stroke = smoothstep(0.008, 0.001, abs(d2_sdf));
    col += rose * d2_stroke;
    alpha = max(alpha, d2_stroke);

    // ── Inner diamond (fast spin, gold stroke) ────────────────────────
    let d3_rot = t * 0.90 + 0.7854;
    let cs3 = cos(d3_rot); let ss3 = sin(d3_rot);
    let rp3 = mat2x2<f32>(cs3, -ss3, ss3, cs3) * p;
    let scale3 = 0.12 * ease_out_back(clamp((t - 0.28) / 0.35, 0.0, 1.0));
    let d3_sdf = (abs(rp3.x) + abs(rp3.y)) - scale3;
    let d3_stroke = smoothstep(0.006, 0.001, abs(d3_sdf));
    col += gold * d3_stroke;
    alpha = max(alpha, d3_stroke);
    // Faint fill
    let d3_fill = smoothstep(0.003, -0.003, d3_sdf) * ease_out_expo(clamp((t - 0.28) / 0.35, 0.0, 1.0));
    col += gold * d3_fill * 0.20;
    alpha = max(alpha, d3_fill * 0.20);

    // ── 4 orbiting accent diamonds (contained at r=0.30) ─────────────
    let acc_rev = ease_out_back(clamp((t - 0.44) / 0.32, 0.0, 1.0));
    for (var i = 0; i < 4; i++) {
        let fi = f32(i);
        let base_ang = fi * 1.5708 + 0.7854 + t * 0.26;
        let acc_pos = vec2<f32>(cos(base_ang), sin(base_ang)) * 0.295 * acc_rev;
        let acc_p = p - acc_pos;
        let acc_r = 0.028 * acc_rev;
        let acc_rot_a = t * 1.6 + fi * 0.785;
        let caa = cos(acc_rot_a); let saa = sin(acc_rot_a);
        let acc_pr = mat2x2<f32>(caa, -saa, saa, caa) * acc_p;
        let acc_d = (abs(acc_pr.x) + abs(acc_pr.y)) - acc_r;
        let acc_fill = smoothstep(0.004, -0.004, acc_d);
        let acc_c = select(rose, gold, i % 2 == 0);
        col += acc_c * acc_fill;
        alpha = max(alpha, acc_fill);
        col += acc_c * exp(-max(acc_d, 0.0) * 70.0) * 0.35 * acc_rev;
        alpha = max(alpha, exp(-max(acc_d, 0.0) * 70.0) * 0.35 * acc_rev * 0.5);
    }

    // ── Core ──────────────────────────────────────────────────────────
    let core_rev = ease_out_back(clamp((t - 0.42) / 0.28, 0.0, 1.0));
    let core_r = (0.028 + 0.007 * sin(t * 6.5)) * core_rev;
    let core = smoothstep(core_r, 0.0, dp);
    col = mix(col, white, core);
    alpha = max(alpha, core);
    col += white * exp(-dp * 15.0) * 0.4 * core_rev;
    alpha = max(alpha, exp(-dp * 15.0) * 0.35 * core_rev);

    // ── Ambient glow ──────────────────────────────────────────────────
    let glow = exp(-dp * 6.5) * 0.15 * ease_out_expo(clamp(t / 0.6, 0.0, 1.0));
    col += mix(slate, gold, clamp(p.y * 2.0 + 0.5, 0.0, 1.0)) * glow;
    alpha = max(alpha, glow * 0.45);

    alpha = clamp(alpha, 0.0, 1.0);
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let t = u.time;
    let aspect = u.resolution.x / u.resolution.y;
    let ca = 0.0018 + 0.0008 * sin(t * 4.7);
    let off = vec2<f32>(ca / aspect, 0.0);
    let sr = diamond_col(uv + off, aspect, t);
    let sg = diamond_col(uv,       aspect, t);
    let sb = diamond_col(uv - off, aspect, t);
    return vec4<f32>(sr.r, sg.g, sb.b, sg.a);
}
