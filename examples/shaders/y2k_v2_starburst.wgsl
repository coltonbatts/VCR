// Y2K Starburst — modern palette, triangle-based star geometry
// Palette: warm gold, chrome white, dusty violet, slate blue
// u.time = SECONDS. p = (uv-0.5)*vec2(aspect,1.0).

fn ease_out_expo(x: f32) -> f32 {
    if (x >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * x);
}
fn ease_out_back(x: f32) -> f32 {
    let c = 1.70158; let t1 = x - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

// Distance from point p to line segment (a→b)
fn sdSeg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a; let ap = p - a;
    let h = clamp(dot(ap, ab) / dot(ab, ab), 0.0, 1.0);
    return length(ap - ab * h);
}

// Signed area / point-in-triangle test
fn triSDF(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let e0 = b - a; let e1 = c - b; let e2 = a - c;
    let v0 = p - a; let v1 = p - b; let v2 = p - c;
    let d0 = dot(v0, vec2<f32>(e0.y, -e0.x));
    let d1 = dot(v1, vec2<f32>(e1.y, -e1.x));
    let d2 = dot(v2, vec2<f32>(e2.y, -e2.x));
    let inside = min(min(d0, d1), d2);
    // SDF: negative inside, positive outside
    let d_ab = sdSeg(p, a, b);
    let d_bc = sdSeg(p, b, c);
    let d_ca = sdSeg(p, c, a);
    return select(min(min(d_ab, d_bc), d_ca), -min(min(d_ab, d_bc), d_ca), inside >= 0.0);
}

// Draw one spike: a triangle from center (0,0) to two base points on a ring at angle a±hw
// outer tip at radius outer_r, base at inner_r
fn spikeVal(p: vec2<f32>, ang: f32, outer_r: f32, inner_r: f32, half_w: f32) -> f32 {
    let tip = vec2<f32>(cos(ang), sin(ang)) * outer_r;
    let bl  = vec2<f32>(cos(ang - half_w), sin(ang - half_w)) * inner_r;
    let br  = vec2<f32>(cos(ang + half_w), sin(ang + half_w)) * inner_r;
    let sd = triSDF(p, tip, bl, br);
    return smoothstep(0.006, -0.006, sd);
}

fn starburst_col(uv: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dp = length(p);
    let angle = atan2(p.y, p.x);
    let pi = 3.14159265;

    var col = vec3<f32>(0.0);
    var alpha = 0.0;

    // Modern palette: warm gold, chrome, dusty rose, slate
    let gold    = vec3<f32>(0.95, 0.78, 0.35);
    let chrome  = vec3<f32>(0.88, 0.90, 0.95);
    let rose    = vec3<f32>(0.85, 0.40, 0.55);
    let slate   = vec3<f32>(0.35, 0.52, 0.75);
    let white   = vec3<f32>(1.0,  1.0,  1.0);

    let rev = ease_out_expo(clamp(t / 0.65, 0.0, 1.0));

    // ── LAYER 1: 16 outer spikes — thin gold needles ──────────────────
    let rot16 = t * 0.10;
    let outer16 = 0.44 * rev;  // pushed to frame edge
    let inner16 = 0.22 * rev;
    let hw16 = 0.09; // half-width angle (~5 deg) — narrow needle
    for (var i = 0; i < 16; i++) {
        let ang = f32(i) * (2.0 * pi / 16.0) + rot16;
        let sv = spikeVal(p, ang, outer16, inner16, hw16);
        col += gold * sv;
        alpha = max(alpha, sv);
    }
    // Outer glow
    let glow16 = exp(-max(dp - outer16 * 0.82, 0.0) * 16.0) * 0.28 * rev * smoothstep(inner16, outer16, dp);
    col += gold * glow16;
    alpha = max(alpha, glow16 * 0.4);

    // ── LAYER 2: 8-point star body — chrome/rose, bold ───────────────
    let body_rev = ease_out_back(clamp((t - 0.20) / 0.45, 0.0, 1.0));
    let rot8 = t * -0.16;
    let outer8 = 0.32 * body_rev;  // larger body
    let inner8 = 0.10 * body_rev;
    let hw8 = 0.20;
    for (var i = 0; i < 8; i++) {
        let ang = f32(i) * (2.0 * pi / 8.0) + rot8;
        let sv = spikeVal(p, ang, outer8, inner8, hw8);
        let g = clamp(dp / max(outer8, 0.01), 0.0, 1.0);
        let sc = mix(chrome, rose, g * g);
        col = mix(col, sc, sv);
        alpha = max(alpha, sv);
    }
    // Body glow
    let body_glow = exp(-max(dp - outer8 * 0.7, 0.0) * 22.0) * 0.45 * body_rev * smoothstep(0.0, outer8, dp);
    col += rose * body_glow;
    alpha = max(alpha, body_glow * 0.55);

    // ── LAYER 3: spinning dashed ring (slate) ─────────────────────────
    let ring1_rev = ease_out_expo(clamp((t - 0.30) / 0.38, 0.0, 1.0));
    let ring1_R = 0.375 * ring1_rev;  // scaled up
    let dash1 = step(0.5, sin((angle + t * 1.4) * 7.0));
    let ring1 = smoothstep(0.005, 0.0, abs(dp - ring1_R)) * dash1 * ring1_rev;
    col += slate * ring1;
    alpha = max(alpha, ring1);

    // ── LAYER 4: outer bold arc ring (gold, 3 arcs) ───────────────────
    let ring2_rev = ease_out_expo(clamp((t - 0.44) / 0.38, 0.0, 1.0));
    let ring2_R = 0.420 * ring2_rev;
    let arc2 = step(0.35, sin((angle - t * 0.38) * 3.0 * 0.5 + 1.0));
    let ring2 = smoothstep(0.007, 0.0, abs(dp - ring2_R)) * arc2 * ring2_rev;
    col += gold * ring2;
    alpha = max(alpha, ring2);

    // ── LAYER 5: inner 4-point star — fast spin, chrome ──────────────
    let in_rev = ease_out_back(clamp((t - 0.40) / 0.30, 0.0, 1.0));
    let rot4 = t * 1.1;
    let outer4 = 0.075 * in_rev;
    let inner4 = 0.010 * in_rev;
    let hw4 = 0.30;
    for (var i = 0; i < 4; i++) {
        let ang = f32(i) * (pi / 2.0) + rot4;
        let sv = spikeVal(p, ang, outer4, inner4, hw4);
        col = mix(col, white, sv);
        alpha = max(alpha, sv);
    }
    let glow4 = exp(-dp * 18.0) * 0.5 * in_rev;
    col += chrome * glow4;
    alpha = max(alpha, glow4 * 0.55);

    // ── LAYER 6: white core ───────────────────────────────────────────
    let core_rev = ease_out_back(clamp((t - 0.50) / 0.25, 0.0, 1.0));
    let core_r = (0.022 + 0.006 * sin(t * 9.0)) * core_rev;
    let core = smoothstep(core_r, 0.0, dp);
    col = mix(col, white, core);
    alpha = max(alpha, core);
    col += white * exp(-dp * 12.0) * 0.35 * core_rev;
    alpha = max(alpha, exp(-dp * 12.0) * 0.35 * core_rev);

    col *= 1.0 + 0.04 * sin(t * 3.8);
    alpha = clamp(alpha, 0.0, 1.0);
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let t = u.time;
    let aspect = u.resolution.x / u.resolution.y;
    let rev = ease_out_expo(clamp(t / 0.65, 0.0, 1.0));
    let ca = 0.0015 + rev * 0.006 * max(0.0, 1.0 - t) + 0.0008 * sin(t * 5.2);
    let off = vec2<f32>(ca / aspect, 0.0);
    let sr = starburst_col(uv + off, aspect, t);
    let sg = starburst_col(uv,       aspect, t);
    let sb = starburst_col(uv - off, aspect, t);
    return vec4<f32>(sr.r, sg.g, sb.b, sg.a);
}
