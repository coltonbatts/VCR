fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    // Center UVs to (-1, 1) range
    let p = (uv - 0.5) * 2.0;
    let aspect = resolution.x / resolution.y;
    let p_adj = vec2<f32>(p.x * aspect, p.y);

    // Polar coordinates for tunnel effect
    let radius = length(p_adj);
    let angle = atan2(p_adj.y, p_adj.x);

    // perspective warping
    let z = 1.0 / (radius + 0.01);
    let tunnel_uv = vec2<f32>(angle / 3.14159, z + time * 0.5);

    // Warp the grid
    let warped_uv = tunnel_uv + 0.05 * vec2<f32>(sin(tunnel_uv.y * 5.0 + time), cos(tunnel_uv.x * 5.0 + time));

    // Grid pattern
    let grid_size = 8.0;
    let grid = abs(fract(warped_uv * grid_size - 0.5) - 0.5);
    let lines = smoothstep(0.04, 0.0, min(grid.x, grid.y));

    // Base color (Lavender/Dreamcore blue)
    let color_top = vec3<f32>(0.6, 0.5, 0.9);
    let color_bottom = vec3<f32>(0.2, 0.1, 0.4);
    var final_color = mix(color_bottom, color_top, uv.y);

    // Add grid lines with highlight
    final_color += lines * vec3<f32>(0.8, 0.9, 1.0);

    // Fog effect: alpha falloff based on radius
    // Fog is thicker at the edges and in the distance (center of tunnel)
    let fog_dist = radius;
    let fog = smoothstep(0.0, 0.7, fog_dist) * smoothstep(1.2, 0.5, fog_dist);

    // Scanlines
    let scanline = 0.5 + 0.5 * sin(uv.y * resolution.y * 1.5);
    final_color *= mix(1.0, scanline, 0.1);

    // Dither effect (simple noise)
    let noise = fract(sin(dot(uv * time, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    final_color += (noise - 0.5) * 0.02;

    // Alpha varies across frame
    let alpha = clamp(fog, 0.0, 1.0);

    // Output STRAIGHT alpha (do NOT premultiply RGB)
    return vec4<f32>(final_color, alpha);
}
