// Sacred Rings — Staggered reveals, eased rotation, breathing scale
// Uses ShaderUniforms API

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

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time * 0.0416667; // 24fps
    let aspect = u.resolution.x / u.resolution.y;

    // Global breathing scale
    let breath = 1.0 + 0.04 * sin(time * 0.9) + 0.02 * sin(time * 2.3);
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0) / breath;
    let dist = length(p);
    let angle = atan2(p.y, p.x);

    var alpha = 0.0;
    var color = vec3<f32>(0.0);

    // Common Palette: Mystic Purple/Cyan
    let col_pri = vec3<f32>(0.4, 0.9, 1.0); // Cyan
    let col_sec = vec3<f32>(0.7, 0.2, 0.9); // Purple
    let col_tri = vec3<f32>(1.0, 0.8, 0.5); // Gold

    // --- Ring 1: Outer Data Ring ---
    let r1_reveal = ease_out_expo(clamp(time / 0.8, 0.0, 1.0));
    let r1_r = 0.35 * r1_reveal;
    let r1_w = 0.002; // Very thin
    let r1_rot = time * 0.2;
    // Data ticks
    let r1_ticks = smoothstep(0.8, 0.9, sin((angle + r1_rot) * 60.0));
    let r1_base = smoothstep(r1_w, 0.0, abs(dist - r1_r));
    let r1_val = r1_base * (0.2 + 0.8 * r1_ticks);
    
    color += col_pri * r1_val * 0.8;
    alpha = max(alpha, r1_val * 0.8);

    // --- Ring 2: Middle Segmented ---
    let r2_reveal = ease_out_back(clamp((time - 0.3) / 0.8, 0.0, 1.0));
    let r2_r = 0.25 * r2_reveal;
    let r2_w = 0.008;
    let r2_rot = -time * 0.5 + sin(time * 0.8) * 0.5;
    let r2_segs = smoothstep(0.0, 0.5, abs(sin((angle + r2_rot) * 3.0)));
    let r2_val = smoothstep(r2_w, 0.0, abs(dist - r2_r)) * r2_segs;
    
    color += col_sec * r2_val;
    alpha = max(alpha, r2_val);

    // --- Ring 3: Inner Fast Ring w/ Glyphs ---
    let r3_reveal = ease_out_expo(clamp((time - 0.6) / 0.8, 0.0, 1.0));
    let r3_r = 0.15 * r3_reveal;
    let r3_w = 0.012;
    let r3_rot = time * 0.8;
    // "Glyphs" - box patterns
    let r3_glyph = step(0.5, sin((angle + r3_rot) * 12.0) * sin((angle - r3_rot * 2.0) * 4.0));
    let r3_val = smoothstep(r3_w, 0.005, abs(dist - r3_r)) * r3_glyph;
    
    color += col_tri * r3_val;
    alpha = max(alpha, r3_val);

    // --- Center: Starburst ---
    let c_reveal = ease_out_back(clamp((time - 1.0) / 0.5, 0.0, 1.0));
    let c_dist = length(p);
    let c_star = abs(cos(angle * 4.0 + time));
    let c_core = smoothstep(0.05 * c_reveal, 0.0, c_dist);
    let c_rays = smoothstep(0.01, 0.0, abs(angle * 0.0)) * smoothstep(0.2, 0.0, c_dist); // simplified ray
    
    color += col_pri * c_core;
    alpha = max(alpha, c_core);

    // --- Lens Flare / Anamorphic Streak on events ---
    // Flash every 3 seconds
    let flash_t = fract(time / 3.0);
    let flash = smoothstep(0.0, 0.1, flash_t) * smoothstep(0.3, 0.1, flash_t);
    // Vertical streak
    let streak_w = 0.002;
    let streak_h = 0.6;
    let streak = smoothstep(streak_w, 0.0, abs(p.x)) * smoothstep(streak_h, 0.0, abs(p.y)) * flash;
    
    color += vec3<f32>(1.0, 1.0, 1.0) * streak;
    alpha = max(alpha, streak);

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}
