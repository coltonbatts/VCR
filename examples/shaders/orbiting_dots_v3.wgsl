// Orbiting Dots — Elastic easing, motion trails, staggered reveals
// Uses ShaderUniforms API

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

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let time = u.time * 0.0416667; // 24fps
    let aspect = u.resolution.x / u.resolution.y;

    // Global breathing
    let breath = 1.0 + 0.03 * sin(time * 0.8);
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0) / breath;

    var color = vec3<f32>(0.0);
    var alpha = 0.0;
    
    // Store particle positions for connections (limited loop)
    var pos_cache = array<vec2<f32>, 12>();
    var active_cache = array<f32, 12>();

    // 12 particles with staggered reveals and motion trails
    // We run the loop first to calculate positions
    for (var i = 0; i < 12; i++) {
        let fi = f32(i);
        let reveal = ease_out_elastic(clamp((time - fi * 0.1) / 0.8, 0.0, 1.0));
        active_cache[i] = reveal;
        
        if (reveal < 0.01) { 
            pos_cache[i] = vec2<f32>(100.0, 100.0); // off-screen
            continue; 
        }

        let orbit_base = 0.12 + fi * 0.02; // Wider spread
        let orbit_breath = 1.0 + 0.12 * sin(time * 1.5 + fi * 0.8);
        let orbit_r = orbit_base * reveal * orbit_breath;
        
        // Pseudo-random speed
        let speed_base = 1.5 - fi * 0.05;
        let direction = select(1.0, -1.0, i % 2 == 0);
        let angle = time * speed_base * direction + fi * 0.5236;
        
        pos_cache[i] = vec2<f32>(cos(angle), sin(angle)) * orbit_r;
    }

    // Render pass
    for (var i = 0; i < 12; i++) {
        let reveal = active_cache[i];
        if (reveal < 0.01) { continue; }
        
        let dot_pos = pos_cache[i];
        let dot_dist = length(p - dot_pos);
        
        // Dot size pulses
        let dot_size = (0.008 + 0.004 * sin(time * 3.0 + f32(i))) * reveal;
        
        // Hot Core Glow
        let core = smoothstep(dot_size, 0.0, dot_dist);
        let glow = exp(-dot_dist * 20.0) * 0.5 * reveal;
        
        let hue = fract(f32(i) * 0.1 + time * 0.1);
        let dot_col = vec3<f32>(
            0.5 + 0.5 * cos(6.283 * hue),
            0.5 + 0.5 * cos(6.283 * (hue + 0.33)), 
            0.5 + 0.5 * cos(6.283 * (hue + 0.67))
        );
        
        color += dot_col * (core * 2.0 + glow);
        alpha = max(alpha, (core + glow * 0.5));
        
        // Connections (Constellations)
        // Check distance to previous particle
        let prev_idx = (i + 11) % 12; // wrap
        let prev_pos = pos_cache[prev_idx];
        let prev_reveal = active_cache[prev_idx];
        
        if (prev_reveal > 0.01) {
            // Line segment SDF
            let pa = p - dot_pos;
            let ba = prev_pos - dot_pos;
            let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
            let d_line = length(pa - ba * h);
            
            // Modulation based on distance between dots (if too far, break connection)
            let dist_dots = length(dot_pos - prev_pos);
            let conn_strength = smoothstep(0.3, 0.1, dist_dots) * reveal * prev_reveal;
            
            let line_w = 0.0015;
            let line_val = smoothstep(line_w, 0.0, d_line) * conn_strength * 0.4;
            
            color += vec3<f32>(0.4, 0.6, 1.0) * line_val;
            alpha = max(alpha, line_val);
        }
    }
    
    // Ambient connection fog
    let fog = fbm(p * 3.0 + time * 0.2) * 0.1;

    alpha = clamp(alpha, 0.0, 1.0);
    return vec4<f32>(color, alpha);
}

// Helper noise for effects
fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}
fn fbm(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var shift = vec2<f32>(100.0);
    var p_curr = p;
    for (var i = 0; i < 3; i++) {
        v += a * hash(p_curr);
        p_curr = p_curr * 2.0 + shift;
        a *= 0.5;
    }
    return v;
}
