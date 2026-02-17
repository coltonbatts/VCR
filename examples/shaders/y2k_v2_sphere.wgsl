// Y2K Bold — Chrome Sphere
// Spinning chrome globe with neon lat/lon grid, magenta equator,
// dual tilted orbital rings, VHS RGB channel split.
// Space: p = (uv-0.5)*vec2(aspect,1.0). R=0.38 fills 76% of frame height.
// u.time = SECONDS (frame/fps), NOT frame index. DO NOT divide by fps again.

fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}
fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158; let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn sphere_col(uv: vec2<f32>, aspect: f32, t: f32) -> vec4<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dp = length(p);

    let reveal = ease_out_back(clamp(t / 0.65, 0.0, 1.0));
    let R = 0.38 * reveal;

    var col = vec3<f32>(0.0);
    var alpha = 0.0;

    // ── Sphere body ─────────────────────────────────────────────────
    let sphere_mask = smoothstep(R + 0.003, R - 0.003, dp);
    if (sphere_mask > 0.0) {
        let nx = p.x / max(R, 0.001);
        let ny = p.y / max(R, 0.001);
        let nz = sqrt(max(0.0, 1.0 - nx*nx - ny*ny));
        let N = normalize(vec3<f32>(nx, ny, nz));

        // Y-axis spin
        let spin = t * 0.28;
        let cs = cos(spin); let ss = sin(spin);
        let Nr = vec3<f32>(N.x*cs + N.z*ss, N.y, -N.x*ss + N.z*cs);

        // Spherical UV for texture
        let su = atan2(Nr.z, Nr.x) * 0.15915 + 0.5;
        let sv = Nr.y * 0.5 + 0.5;

        // Phong lighting
        let L = normalize(vec3<f32>(0.55, 0.70, 0.85));
        let V = vec3<f32>(0.0, 0.0, 1.0);
        let H = normalize(L + V);
        let diff = max(dot(N, L), 0.0);
        let spec = pow(max(dot(N, H), 0.0), 160.0);
        let rim  = pow(1.0 - max(dot(N, V), 0.0), 4.0);

        // Lat/lon grid
        let g_lon = smoothstep(0.018, 0.002, abs(fract(su * 12.0) - 0.5));
        let g_lat = smoothstep(0.022, 0.002, abs(fract(sv *  8.0) - 0.5));
        let grid = max(g_lon, g_lat);

        // Equatorial magenta band
        let eq = smoothstep(0.06, 0.006, abs(Nr.y));
        // Pole glows
        let pole_n = smoothstep(0.10, 0.0, abs(Nr.y - 1.0));
        let pole_s = smoothstep(0.10, 0.0, abs(Nr.y + 1.0));

        // Modern palette: chrome base, gold grid, rose equator, slate rim
        var sc = vec3<f32>(0.55, 0.58, 0.65) * (0.06 + diff * 0.75);
        sc += vec3<f32>(1.0,  1.0,  0.98) * spec * 0.95;
        sc += vec3<f32>(0.35, 0.52, 0.75) * rim  * 0.70; // slate rim
        sc += vec3<f32>(0.95, 0.78, 0.35) * grid * 0.80; // gold grid
        sc += vec3<f32>(0.85, 0.40, 0.55) * eq   * 1.0;  // rose equator
        sc += vec3<f32>(0.88, 0.90, 0.95) * pole_n * 0.85; // chrome north
        sc += vec3<f32>(0.85, 0.40, 0.55) * pole_s * 0.55; // rose south

        col += sc * sphere_mask;
        alpha = max(alpha, sphere_mask);
    }

    // ── Orbital ring 1 (squish Y → tilted back ~50°) ─────────────────
    let r1_rev = ease_out_expo(clamp((t - 0.5) / 0.45, 0.0, 1.0));
    let r1_R = 0.455 * r1_rev;
    let p1 = vec2<f32>(p.x, p.y * 0.55);
    let d1 = abs(length(p1) - r1_R);
    let ang1 = atan2(p1.y, p1.x) + t * 0.42;
    let tick1 = step(0.86, sin(ang1 * 40.0));
    let r1_val = smoothstep(0.007, 0.0, d1) * (0.2 + 0.8 * tick1) * r1_rev;
    col += vec3<f32>(0.35, 0.52, 0.75) * r1_val; // slate ring
    alpha = max(alpha, r1_val);

    // ── Orbital ring 2 (squish X → tilted side ~50°) ─────────────────
    let r2_rev = ease_out_expo(clamp((t - 0.7) / 0.45, 0.0, 1.0));
    let r2_R = 0.43 * r2_rev;
    let p2 = vec2<f32>(p.x * 0.55, p.y);
    let d2 = abs(length(p2) - r2_R);
    let ang2 = atan2(p2.y, p2.x) - t * 0.58;
    let seg2 = step(0.18, abs(sin(ang2 * 3.0)));
    let r2_val = smoothstep(0.009, 0.0, d2) * seg2 * r2_rev;
    col += vec3<f32>(0.95, 0.78, 0.35) * r2_val; // gold ring
    alpha = max(alpha, r2_val);

    // ── Glow halo ────────────────────────────────────────────────────
    let glow_rev = ease_out_expo(clamp(t / 0.7, 0.0, 1.0));
    let glow = exp(-max(dp - R - 0.01, 0.0) * 9.0) * 0.28 * glow_rev;
    let g_col = mix(vec3<f32>(0.85, 0.40, 0.55), vec3<f32>(0.35, 0.52, 0.75),
                    clamp(p.y / 0.38 * 0.5 + 0.5, 0.0, 1.0));
    col += g_col * glow;
    alpha = max(alpha, glow * 0.75);

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(col, alpha);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let t = u.time; // already in seconds
    let aspect = u.resolution.x / u.resolution.y;
    let ca = 0.0022 + 0.0012 * sin(t * 4.3);
    let off = vec2<f32>(ca / aspect, 0.0);
    let sr = sphere_col(uv + off, aspect, t);
    let sg = sphere_col(uv,       aspect, t);
    let sb = sphere_col(uv - off, aspect, t);
    return vec4<f32>(sr.r, sg.g, sb.b, sg.a);
}
