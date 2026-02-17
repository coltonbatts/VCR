// Dreamcore Column — Dramatic reveal, light sweep, breathing sway
// Uses ShaderUniforms API

// --- Easing ---
fn ease_out_expo(t: f32) -> f32 {
    if (t >= 1.0) { return 1.0; }
    return 1.0 - pow(2.0, -10.0 * t);
}

fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158;
    let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if (t < 0.5) { return 4.0 * t * t * t; }
    let f = 2.0 * t - 2.0;
    return 0.5 * f * f * f + 1.0;
}

// --- Noise & FBM ---
fn hash2(p: vec2<f32>) -> vec2<f32> {
    var p2 = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(p2) * 43758.5453);
}

fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var p_curr = p;
    for (var i = 0; i < 5; i++) {
        v += a * hash2(p_curr).x;
        p_curr = p_curr * 2.1 + vec2<f32>(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

// --- SDF Primitives ---
fn sdBox(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time * 0.0416667; // 24fps
    let aspect = u.resolution.x / u.resolution.y;

    // --- Animation State ---
    // Reveal: column rises from bottom with overshoot
    let rise = ease_out_back(clamp(time / 1.5, 0.0, 1.0));
    let rise_offset = (1.0 - rise) * 0.6; // starts below frame
    
    // Sway: slight majestic lean
    let sway_ang = sin(time * 0.7) * 0.02;
    let c_sway = cos(sway_ang);
    let s_sway = sin(sway_ang);
    let rot = mat2x2<f32>(c_sway, -s_sway, s_sway, c_sway);

    // Breath: global scale pulse
    let breath = 1.0 + 0.02 * sin(time * 1.0);

    let p_raw = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    // Apply sway rotation
    var p = rot * p_raw;
    // Apply reveal and breath
    p.y += rise_offset;
    p /= breath;

    // --- Geometry Definitions ---
    // Shaft: tapered cylinder
    let y_norm = clamp(p.y * 2.0 + 0.5, 0.0, 1.0); // 0 at bottom, 1 at top approx
    let col_w_base = 0.07;
    let col_w_top = 0.055;
    let col_w = mix(col_w_base, col_w_top, y_norm);
    
    // Fluting: modify radius based on angle approx (x pos)
    // We approximate the cylinder surface normal x-component as p.x / col_w
    let x_surf = p.x / col_w;
    // Only valid within the shaft width
    var d_shaft = abs(p.x) - col_w;
    
    // Pseudo-3D Normal Calculation
    // Normal z = sqrt(1 - x^2) assuming cylinder
    // We'll modulate the radius to create flutes
    let flute_freq = 18.0;
    // Distortion of the surface
    let flute_depth = 0.003;
    let flute = sin(acos(clamp(x_surf, -1.0, 1.0)) * flute_freq); 
    // Applying flute to distance field effectively
    d_shaft += flute * flute_depth * smoothstep(0.9, 0.5, abs(x_surf)); // fade flutes at edge
    
    // Capital (Top)
    let cap_y = 0.38;
    let cap_w = 0.09;
    let cap_h = 0.05;
    let d_cap = sdBox(p - vec2<f32>(0.0, cap_y), vec2<f32>(cap_w, cap_h));
    
    // Base (Bottom)
    let base_y = -0.38;
    let base_w = 0.10;
    let base_h = 0.04;
    let d_base = sdBox(p - vec2<f32>(0.0, base_y), vec2<f32>(base_w, base_h));
    
    // Unite shapes
    // Smooth transition between shaft and ends
    let d_col = min(d_shaft, min(d_cap, d_base));
    
    // Vertical clip
    let h_limit = 0.42;
    let d_clip = abs(p.y) - h_limit;
    let d_final = max(d_col, d_clip);
    
    // --- Render Mask ---
    // Soft anti-aliased edge
    let alpha_mask = smoothstep(0.001, -0.001, d_final);
    
    if (alpha_mask <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    
    // --- Material / Lighting ---
    // Calculate normal (Pseudo-3D)
    // If we are on the shaft:
    var N = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(p.y) < cap_y - cap_h && abs(p.y) > base_y + base_h) {
        // Cylinder normal
        let nx = p.x / col_w;
        let nz = sqrt(max(0.0, 1.0 - nx*nx));
        // Add flute perturbation to normal
        let flute_n = cos(acos(clamp(nx, -1.0, 1.0)) * flute_freq) * 0.3;
        N = normalize(vec3<f32>(nx + flute_n, 0.0, nz));
    } else {
        // Block normal (flat front + slight curve for style)
        N = normalize(vec3<f32>(p.x * 2.0, p.y * 2.0, 1.0));
    }
    
    // Lighting setup
    let light_dir = normalize(vec3<f32>(sin(time), 0.2, 0.8));
    let view_dir = vec3<f32>(0.0, 0.0, 1.0);
    let half_dir = normalize(light_dir + view_dir);
    
    // Diffuse
    let diff = max(dot(N, light_dir), 0.0);
    
    // Specular (Glossy Marble)
    let spec = pow(max(dot(N, half_dir), 0.0), 32.0);
    
    // Rim light (Atmospheric)
    let rim = 1.0 - max(dot(N, view_dir), 0.0);
    let rim_light = pow(rim, 3.0) * vec3<f32>(0.5, 0.0, 1.0); // Purple rim
    
    // --- Texture: Alien Marble ---
    let noise_scale = 12.0;
    let marble_p = vec2<f32>(p.x, p.y * 0.5) * noise_scale;
    let fbm_val = fbm(marble_p + time * 0.05);
    // Veins
    let vein = smoothstep(0.4, 0.45, abs(sin(marble_p.y + fbm_val * 4.0)));
    
    // Colors
    let col_base = vec3<f32>(0.1, 0.05, 0.15); // Dark obsidian base
    let col_vein = vec3<f32>(0.8, 0.6, 0.4);   // Gold/Copper veins
    let col_surf = mix(col_base, col_vein, vein * 0.6);
    
    // Combine lighting
    var color = col_surf * (0.2 + diff * 0.8) + spec * 0.5;
    color += rim_light;
    
    // --- Magic Sweep ---
    // A beam of light scanning up/down
    let sweep_y = sin(time) * 0.5;
    let sweep_width = 0.1;
    let sweep = smoothstep(sweep_width, 0.0, abs(p.y - sweep_y));
    color += vec3<f32>(0.4, 1.0, 0.8) * sweep * 0.5; // Cyan scanner
    
    // Shadow at the base
    let contact_shadow = smoothstep(-0.42, -0.3, p.y);
    color *= (0.2 + 0.8 * contact_shadow);

    return vec4<f32>(color, alpha_mask);
}
