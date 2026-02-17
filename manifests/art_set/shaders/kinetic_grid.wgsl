// Kinetic Wireframe Grid
// Deforming grid waves on alpha

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Wave deformation
    let p_def = p + vec2<f32>(
        sin(p.y * 10.0 + time) * 0.02,
        cos(p.x * 10.0 + time) * 0.02
    );
    
    let grid_size = 20.0;
    let g = fract(p_def * grid_size);
    let grid_line = smoothstep(0.02, 0.0, min(g.x, 1.0 - g.x)) + 
                    smoothstep(0.02, 0.0, min(g.y, 1.0 - g.y));
    
    let mask = smoothstep(0.4, 0.35, length(p));
    let color = vec3<f32>(0.0, 1.0, 0.6) * grid_line;
    let alpha = grid_line * mask;
    
    return vec4<f32>(color, alpha);
}
