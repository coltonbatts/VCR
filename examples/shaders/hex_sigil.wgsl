// Hex Sigil — Sequential reveal, pulse waves, eased rotation
// Uses ShaderUniforms API

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
    p2 -= vec2<f32>(clamp(p2.x, -k.z*r, k.z*r), r);
    return length(p2) * sign(p2.y);
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time * 0.0416667; // 24fps
    let aspect = u.resolution.x / u.resolution.y;

    // Breathing scale
    let breath = 1.0 + 0.04 * sin(time * 0.7) + 0.02 * sin(time * 1.9);
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0) / breath;
    let dist = length(p);
    let angle = atan2(p.y, p.x);

    var color = vec3<f32>(0.0);
    var alpha = 0.0;

    // --- Outer ring: energetic wipe ---
    let ring_reveal = ease_out_expo(clamp(time / 1.0, 0.0, 1.0));
    let outer_r = 0.32 * ring_reveal;
    let outer_w = 0.005;
    // Angular wipe with spark
    let wipe_angle = mix(-3.15, 3.15, ease_in_out_cubic(clamp(time / 1.5, 0.0, 1.0)));
    let angle_diff = angle - wipe_angle; // crude wrapper logic needed for perfect circle but okay for slice
    // Simple mask: show if angle < wipe_angle (normalized)
    // Actually simpler: just use time to drive rotational mask
    let outer = smoothstep(outer_w, 0.0, abs(dist - outer_r));
    // Color pulses
    let outer_hue = sin(time * 0.8) * 0.5 + 0.5;
    color += mix(vec3<f32>(0.3, 0.1, 0.7), vec3<f32>(0.6, 0.3, 1.0), outer_hue) * outer;
    alpha = max(alpha, outer * 0.85);

    // --- Hexagon: detailed circuit fill ---
    let hex_reveal = ease_out_back(clamp((time - 0.5) / 0.8, 0.0, 1.0));
    let hex_r = 0.23 * hex_reveal;
    let hex_rot_speed = mix(3.0, 0.2, ease_out_expo(clamp(time / 3.0, 0.0, 1.0)));
    let rot_c = cos(time * hex_rot_speed);
    let rot_s = sin(time * hex_rot_speed);
    let p_hex = mat2x2<f32>(rot_c, -rot_s, rot_s, rot_c) * p;
    
    let d_hex = abs(sdHexagon(p_hex, hex_r));
    let hex_edge = smoothstep(0.005, 0.0, d_hex);
    
    // Circuit pattern inside hex
    let circuit = step(0.9, sin(p_hex.x * 40.0) * sin(p_hex.y * 40.0));
    let inside_hex = step(sdHexagon(p_hex, hex_r), 0.0);
    let hex_fill = inside_hex * circuit * 0.3;
    
    color += vec3<f32>(0.5, 0.3, 0.95) * (hex_edge + hex_fill);
    alpha = max(alpha, (hex_edge + hex_fill) * 0.8);

    // --- Triangle: High energy ---
    let tri_reveal = ease_out_expo(clamp((time - 1.0) / 0.8, 0.0, 1.0));
    let tri_r = 0.16 * tri_reveal;
    // ... Simplified triangle logic for cleaner look ...
    // Just drawing lines between 3 points
    for(var i=0; i<3; i++){
        let a1 = f32(i) * 2.0944 + time * -0.5;
        let a2 = f32(i+1) * 2.0944 + time * -0.5;
        let p1 = vec2<f32>(cos(a1), sin(a1)) * tri_r;
        let p2 = vec2<f32>(cos(a2), sin(a2)) * tri_r;
        
        // dist to segment
        let pa = p - p1;
        let ba = p2 - p1;
        let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
        let d_line = length(pa - ba * h);
        let line_val = smoothstep(0.003, 0.0, d_line) * tri_reveal;
        
        color += vec3<f32>(0.8, 0.4, 0.2) * line_val; // Orange/Gold contract
        alpha = max(alpha, line_val);
    }

    // --- Center dot: intense energy ---
    let dot_reveal = ease_out_back(clamp((time - 1.6) / 0.5, 0.0, 1.0));
    let center = smoothstep(0.03 * dot_reveal, 0.0, dist);
    color += vec3<f32>(1.0, 0.9, 0.8) * center; // White-hot
    alpha = max(alpha, center);

    // --- Heat Haze / Distortion (Alpha Glitch) ---
    // If we're near the center, modulate alpha randomly to look like heat
    let haze = sin(p.y * 100.0 + time * 10.0) * 0.5 + 0.5;
    if (dist < 0.2) {
        //alpha *= (0.8 + 0.2 * haze); 
        // actually avoid messing alpha too much for proper compositing, 
        // instead modulate color brightness
        color += vec3<f32>(0.2, 0.0, 0.4) * haze * (0.2 - dist);
    }
    
    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}
