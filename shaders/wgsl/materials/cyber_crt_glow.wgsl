// CRT Glow & Glitch Shader
// Masked to the panel area
// Driven by VCR built-in uniforms

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    // Custom uniform indices
    // custom[0].x = flicker intensity (driven by glitch(t * 12.0) in manifest)
    // custom[0].y = panel aspect ratio
    // custom[0].z = corner radius normalized
    
    let flicker = u.custom[0].x;
    
    // Panel Mask (Rounded Rect SDF in UV space)
    let size = vec2<f32>(0.35, 0.08); // Half-size of the panel in UV space
    let center = vec2<f32>(0.4, 0.85); // Center of the panel in UV space
    let radius = 0.02;
    
    let p = uv - center;
    let d = abs(p) - size + radius;
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
    
    // Soft mask
    let mask = 1.0 - smoothstep(-0.002, 0.002, dist);
    
    if (mask <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    
    // Scanlines (bright, not darkening)
    let scanline = sin(uv.y * u.resolution.y * 0.8) * 0.05 + 0.95;
    
    // RGB Subpixel-ish pop
    let subpixel = sin(uv.x * u.resolution.x * 2.0) * 0.02 + 0.98;
    
    // Combine effects into a bright "Cyber" color
    // We use high RGB values to avoid "taking on black"
    let flicker_val = max(0.8, flicker);
    let color = vec3<f32>(0.5, 0.8, 1.0) * scanline * subpixel * flicker_val;
    
    return vec4<f32>(color, mask * 0.2); // Subtle but bright glow
}

