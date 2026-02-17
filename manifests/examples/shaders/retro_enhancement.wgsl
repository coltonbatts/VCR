// retro_enhancement.wgsl — Y2K Grid and Glow background
// Designed to complement Retro Emoji motion pack

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);

    // Dark blue background
    var color = vec3<f32>(0.02, 0.02, 0.05);

    // Animated Grid
    let grid_uv = p * 4.0;
    let scroll = time * 0.2;
    let scanline = sin((grid_uv.y + scroll) * 20.0);
    let vertical_line = sin((grid_uv.x) * 20.0);
    
    let grid = smoothstep(0.98, 1.0, max(scanline, vertical_line));
    let grid_color = vec3<f32>(0.2, 0.4, 0.8) * grid * (0.5 + 0.5 * sin(time * 2.0));
    color += grid_color;

    // Glowing center (behind where emoji will be)
    let glow = exp(-length(p) * 4.0) * 0.2;
    color += vec3<f32>(0.4, 0.1, 0.6) * glow;

    // Subtle scanline overlay
    let sl = sin(uv.y * 1000.0) * 0.05;
    color -= vec3<f32>(sl);

    return vec4<f32>(color, 1.0);
}
