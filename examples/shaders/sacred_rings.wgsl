// Sacred Geometry — Concentric rotating rings on alpha
// Standalone element: centered, plenty of margin, transparent bg

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let dist = length(p);
    let angle = atan2(p.y, p.x);

    var alpha = 0.0;
    var color = vec3<f32>(0.0);

    // Ring 1 — outer, slow rotation
    let r1_center = 0.28;
    let r1_width = 0.012;
    let r1_dash = smoothstep(0.0, 0.5, abs(sin(angle * 8.0 + time * 0.5)));
    let r1 = smoothstep(r1_width, 0.0, abs(dist - r1_center)) * r1_dash;
    color += vec3<f32>(0.3, 0.7, 1.0) * r1;
    alpha = max(alpha, r1 * 0.9);

    // Ring 2 — middle, opposite rotation
    let r2_center = 0.20;
    let r2_width = 0.010;
    let r2_dash = smoothstep(0.0, 0.5, abs(sin(angle * 12.0 - time * 0.8)));
    let r2 = smoothstep(r2_width, 0.0, abs(dist - r2_center)) * r2_dash;
    color += vec3<f32>(0.8, 0.3, 1.0) * r2;
    alpha = max(alpha, r2 * 0.85);

    // Ring 3 — inner, fast
    let r3_center = 0.12;
    let r3_width = 0.008;
    let r3_dash = smoothstep(0.0, 0.5, abs(sin(angle * 6.0 + time * 1.2)));
    let r3 = smoothstep(r3_width, 0.0, abs(dist - r3_center)) * r3_dash;
    color += vec3<f32>(1.0, 0.5, 0.7) * r3;
    alpha = max(alpha, r3 * 0.8);

    // Center dot — pulsing
    let pulse = 0.02 + 0.005 * sin(time * 3.0);
    let center_dot = smoothstep(pulse, pulse - 0.005, dist);
    color += vec3<f32>(1.0, 1.0, 1.0) * center_dot;
    alpha = max(alpha, center_dot * 0.95);

    // Faint glow around everything
    let glow = exp(-dist * 6.0) * 0.15 * (sin(time * 2.0) * 0.3 + 0.7);
    color += vec3<f32>(0.5, 0.4, 1.0) * glow;
    alpha = max(alpha, glow);

    alpha = clamp(alpha, 0.0, 1.0);
    if (alpha < 0.01) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(color, alpha);
}
