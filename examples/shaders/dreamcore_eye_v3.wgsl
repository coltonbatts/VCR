// Dreamcore Eye — Dramatic open/close with organic motion
// Uses ShaderUniforms API

// Easing functions
fn ease_out_elastic(t: f32) -> f32 {
    if (t <= 0.0) { return 0.0; }
    if (t >= 1.0) { return 1.0; }
    let p = 0.3;
    return pow(2.0, -10.0 * t) * sin((t - p / 4.0) * 6.283185 / p) + 1.0;
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if (t < 0.5) { return 4.0 * t * t * t; }
    let f = 2.0 * t - 2.0;
    return 0.5 * f * f * f + 1.0;
}

fn ease_out_back(t: f32) -> f32 {
    let c = 1.70158;
    let t1 = t - 1.0;
    return t1 * t1 * ((c + 1.0) * t1 + c) + 1.0;
}

fn sdVesica(p_in: vec2<f32>, r: f32, d: f32) -> f32 {
    let p = abs(p_in);
    let b = sqrt(r * r - d * d);
    if ((p.y - d) * b > p.x * d) {
        return length(p - vec2<f32>(0.0, d));
    } else {
        return length(p - vec2<f32>(-b, 0.0)) - r;
    }
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    var p2 = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(p2) * 43758.5453);
}

// FBM for deeper organic texture
fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var shift = vec2<f32>(100.0);
    // Rotate to reduce axial bias
    let rot = mat2x2<f32>(cos(0.5), sin(0.5), -sin(0.5), cos(0.5));
    var p_curr = p;
    for (var i = 0; i < 5; i++) {
        v += a * hash2(p_curr).x;
        p_curr = rot * p_curr * 2.0 + shift;
        a *= 0.5;
    }
    return v;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time * 0.0416667; // frame to seconds at 24fps
    let aspect = u.resolution.x / u.resolution.y;

    // Dramatic scale breathing — the whole eye pulses
    let breath = 1.0 + 0.06 * sin(time * 1.2) + 0.03 * sin(time * 2.7);
    let p_raw = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let p = p_raw / breath;
    
    // --- Chromatic Aberration Setup ---
    // We'll sample the shape slightly offset for R and B channels later
    let ca_amount = 0.003 * (1.0 + 0.5 * sin(time * 5.0));
    
    // --- Blink cycle with anticipation ---
    // 5-second loop
    let loop_t = fract(time / 5.0) * 5.0;

    var open_amount = 1.0;
    // First blink at ~1.2s — fast, snappy
    let blink1 = smoothstep(1.0, 1.15, loop_t) * smoothstep(1.4, 1.25, loop_t);
    // Second blink at ~3.0s — slower, more dramatic with anticipation
    let blink2_down = ease_in_out_cubic(smoothstep(2.8, 3.1, loop_t));
    let blink2_up = ease_out_back(smoothstep(3.1, 3.6, loop_t));
    let blink2 = blink2_down * (1.0 - blink2_up);
    // Micro-flutter at ~4.2s
    let flutter = smoothstep(4.1, 4.15, loop_t) * smoothstep(4.25, 4.2, loop_t) * 0.4;

    open_amount = 1.0 - blink1 - blink2 - flutter;
    open_amount = clamp(open_amount, 0.0, 1.0);

    let r = 0.85;
    let d = mix(0.84, 0.5, open_amount);
    
    // Calculate distance for the main shape (Green channel / brightness)
    let eye_dist = sdVesica(p.yx, r, d);
    let eye_mask = smoothstep(0.005, -0.005, eye_dist);

    if (eye_mask <= 0.001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // --- Eye Look Direction ---
    let look_cycle = time * 0.4;
    let look_x = sin(look_cycle) * 0.08 * ease_in_out_cubic(fract(look_cycle / 6.283));
    let look_y = cos(look_cycle * 0.7) * 0.04;
    // Quick saccade
    let saccade_t = fract(time / 3.0);
    let saccade = smoothstep(0.0, 0.05, saccade_t) * smoothstep(0.1, 0.05, saccade_t);
    let saccade_offset = vec2<f32>(sin(floor(time / 3.0) * 7.13) * 0.06, 0.0) * saccade;

    let iris_center = vec2<f32>(look_x, look_y) + saccade_offset;
    
    // --- Cornea Bulge / Refraction ---
    // Fake 3D bulge by distorting UVs based on distance from iris center
    let p_iris_raw = p - iris_center;
    let dist_from_center = length(p_iris_raw);
    let bulge = smoothstep(0.4, 0.0, dist_from_center) * 0.15;
    let p_iris = p_iris_raw * (1.0 - bulge); // Distort inward
    
    let dist_iris = length(p_iris);

    // --- Sclera (White) ---
    // Veins and wetness
    let noisy_sclera = fbm(p * 8.0) * 0.05;
    let vein = smoothstep(0.3, 0.0, abs(p.x * 3.0 + sin(p.y * 40.0) * 0.02)) * 0.06;
    let sclera_base = vec3<f32>(0.92, 0.90, 0.95);
    // Shadow from eyelids
    let lid_shadow = smoothstep(-0.1, 0.3, eye_dist) * 0.6;
    var final_color = sclera_base - noisy_sclera - vec3<f32>(vein * 0.8, vein * 0.2, vein * 0.2);
    final_color *= (1.0 - lid_shadow);

    // --- Iris ---
    let iris_radius = 0.24;
    if (dist_iris < iris_radius + 0.02) {
        let iris_angle = atan2(p_iris.y, p_iris.x);
        let iris_rot = iris_angle + time * 0.1 + fbm(p_iris * 2.0) * 0.5;
        
        let radial = sin(iris_rot * 20.0 + fbm(vec2<f32>(iris_angle, time * 0.1)) * 5.0);
        let noisy_iris = fbm(p_iris * 25.0 + time * 0.05);
        
        // Color palette - deeper, more alien
        let iris_inner_col = vec3<f32>(0.8, 0.5, 0.1); // Amber
        let iris_outer_col = vec3<f32>(0.1, 0.4, 0.8); // Deep Blue
        let iris_mix = smoothstep(0.05, 0.24, dist_iris);
        
        var iris_col = mix(iris_inner_col, iris_outer_col, iris_mix);
        iris_col += radial * 0.05;
        iris_col += noisy_iris * 0.1;
        
        // Dark ring at the edge (limbal ring)
        let limbal = smoothstep(iris_radius - 0.04, iris_radius, dist_iris);
        iris_col = mix(iris_col, vec3<f32>(0.02, 0.02, 0.05), limbal * 0.8);
        
        let iris_alpha = smoothstep(iris_radius, iris_radius - 0.01, dist_iris);
        final_color = mix(final_color, iris_col, iris_alpha);
    }

    // --- Pupil ---
    let dilate = ease_in_out_cubic(sin(time * 0.8) * 0.5 + 0.5);
    let pupil_size = mix(0.06, 0.11, dilate);
    if (dist_iris < pupil_size + 0.01) {
        let pupil_mask = smoothstep(pupil_size, pupil_size - 0.01, dist_iris);
        final_color = mix(final_color, vec3<f32>(0.0, 0.0, 0.01), pupil_mask);
    }

    // --- Specular Highlights (Wetness) ---
    // Main highlight
    let spec_pos1 = iris_center + vec2<f32>(-0.08, 0.08);
    let spec1 = smoothstep(0.04, 0.01, length(p - spec_pos1));
    // Secondary soft highlight
    let spec_pos2 = iris_center + vec2<f32>(0.06, -0.06);
    let spec2 = smoothstep(0.06, 0.02, length(p - spec_pos2)) * 0.4;
    
    // Cornea reflection (environment map fake)
    let env_ref = smoothstep(0.5, 0.8, sin(p.x * 20.0 + p.y * 10.0)) * 0.05;
    
    final_color += vec3<f32>(1.0) * (spec1 + spec2) + env_ref;

    // --- Chromatic Aberration on Edges ---
    // Simply shift the red channel slightly outward
    // We approximate this by darkening the red channel where the green channel (main shape) is fading out
    let edge_r_dist = sdVesica(p.yx, r * 1.005, d); // Slightly larger
    let edge_r_mask = smoothstep(0.005, -0.005, edge_r_dist);
    let ca_r = max(0.0, edge_r_mask - eye_mask);
    
    final_color.r += ca_r * 2.0; // Red fringe
    final_color.b += ca_r * 0.5; // Slight purple tint to fringe

    // --- Atmospheric Glow ---
    let glow_intensity = 0.15 * (sin(time * 1.5) * 0.5 + 0.5);
    let glow_col = vec3<f32>(0.5, 0.2, 0.8);
    final_color += glow_col * glow_intensity * open_amount;

    return vec4<f32>(final_color, eye_mask);
}
