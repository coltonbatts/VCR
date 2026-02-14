// Dreamcore Opening Eye (Alpha Transparent) V3
// Optimized for 9:16 Aspect Ratio

fn sdVesica(p_in: vec2<f32>, r: f32, d: f32) -> f32 {
    let p = abs(p_in);
    let b = sqrt(r*r - d*d);
    if ((p.y - d) * b > p.x * d) {
        return length(p - vec2<f32>(0.0, d));
    } else {
        return length(p - vec2<f32>(-b, 0.0)) - r;
    }
}

fn hash2(p: vec2<f32>) -> vec2<f32> {
    var p2 = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(p2) * 43758.5453123);
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Animation cycles
    let cycle = fract(time * 0.2); // 5 second main cycle
    
    // Lid opening: Open for part of cycle, then micro-blink
    var open_amount = smoothstep(0.0, 0.3, cycle) * smoothstep(1.0, 0.7, cycle);
    
    // Micro-blink at cycle 0.5
    let micro_blink = 1.0 - smoothstep(0.03, 0.0, abs(cycle - 0.53));
    open_amount *= micro_blink;
    
    // Lid parameters: Limited 'd' to prevent "too wide" feeling
    let r = 0.85;
    let d = mix(0.84, 0.55, open_amount); // 0.84 is nearly closed, 0.55 is natural wide
    
    // Eye shape (Vesica) - rotated for horizontal opening
    let eye_dist = sdVesica(p.yx, r, d);
    let eye_mask = smoothstep(0.005, -0.005, eye_dist);
    
    if (eye_mask <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    
    // Iris and Pupil movement
    let iris_center = vec2<f32>(sin(time * 0.4) * 0.04, cos(time * 0.6) * 0.03);
    let p_iris = p - iris_center;
    let dist_iris = length(p_iris);
    
    // Layering
    var final_color = vec3<f32>(0.0);
    
    // Sclera (White part) with subtle veins/noise
    let noisy_sclera = hash2(p * 20.0).x * 0.02;
    let sclera_color = vec3<f32>(0.96, 0.94, 0.98) - noisy_sclera;
    final_color = sclera_color;
    
    // Iris
    if (dist_iris < 0.22) {
        let noisy = hash2(p_iris * 15.0 + time * 0.1).x;
        let iris_base = mix(vec3<f32>(0.9, 0.1, 0.6), vec3<f32>(0.1, 0.5, 0.9), dist_iris * 4.0);
        let iris_tex = mix(iris_base, vec3<f32>(1.0), noisy * 0.15);
        
        let iris_mask = smoothstep(0.22, 0.21, dist_iris);
        final_color = mix(final_color, iris_tex, iris_mask);
    }
    
    // Pupil
    let pupil_size = 0.07 + 0.02 * sin(time * 1.5);
    if (dist_iris < pupil_size) {
        let pupil_mask = smoothstep(pupil_size, pupil_size - 0.01, dist_iris);
        final_color = mix(final_color, vec3<f32>(0.03, 0.0, 0.08), pupil_mask);
    }
    
    // Shadowing on the edges of the eye (occlusion by lids)
    let edge_shadow = smoothstep(-0.1, 0.4, eye_dist);
    final_color *= (1.0 - edge_shadow * 0.6);
    
    // Eerie glow
    let glow = 0.08 * (sin(time * 3.0) * 0.5 + 0.5) * open_amount;
    final_color += vec3<f32>(0.5, 0.2, 1.0) * glow;
    
    return vec4<f32>(final_color, eye_mask);
}
