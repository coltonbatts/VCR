// Orbiting Dots — particles circling a center point on alpha
// Y2K / occult orbital system

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);

    var color = vec3<f32>(0.0);
    var alpha = 0.0;

    // Draw orbiting dots at different radii and speeds
    for (var i = 0; i < 8; i++) {
        let fi = f32(i);
        let orbit_r = 0.08 + fi * 0.035;
        let speed = (1.5 - fi * 0.1) * select(1.0, -1.0, i % 2 == 0);
        let phase = fi * 0.785398; // pi/4 stagger

        let angle = time * speed + phase;
        let dot_pos = vec2<f32>(cos(angle), sin(angle)) * orbit_r;
        let dot_dist = length(p - dot_pos);

        let dot_size = 0.008 + 0.003 * sin(time * 2.0 + fi);
        let dot = smoothstep(dot_size, dot_size - 0.003, dot_dist);

        // Each dot gets a slightly different hue
        let hue = fract(fi * 0.125 + time * 0.05);
        let dot_color = vec3<f32>(
            0.5 + 0.5 * cos(6.283 * hue),
            0.5 + 0.5 * cos(6.283 * (hue + 0.333)),
            0.5 + 0.5 * cos(6.283 * (hue + 0.666))
        );

        color += dot_color * dot;
        alpha = max(alpha, dot * 0.95);

        // Faint trail / orbit ring
        let ring = smoothstep(0.002, 0.0, abs(length(p) - orbit_r)) * 0.08;
        color += vec3<f32>(0.4, 0.3, 0.6) * ring;
        alpha = max(alpha, ring);
    }

    // Center glow
    let center_glow = exp(-length(p) * 15.0) * 0.3;
    color += vec3<f32>(1.0, 0.8, 1.0) * center_glow;
    alpha = max(alpha, center_glow);

    alpha = clamp(alpha, 0.0, 1.0);
    if (alpha < 0.01) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(color, alpha);
}
