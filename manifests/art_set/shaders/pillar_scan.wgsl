// Brutalist Pillar Scan
// Column with a vertical scanning laser line on alpha

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let col_w = 0.15;
    let col_h = 0.4;
    
    var alpha = 0.0;
    var color = vec3<f32>(0.0);
    
    if (abs(p.x) < col_w && abs(p.y) < col_h) {
        alpha = 0.9;
        color = vec3<f32>(0.15, 0.15, 0.18);
        
        // Vertical scan line
        let scan_y = sin(time * 2.0) * col_h;
        let scan_line = smoothstep(0.01, 0.0, abs(p.y - scan_y));
        color += vec3<f32>(1.0, 0.1, 0.2) * scan_line * 2.0;
        alpha += scan_line * 0.1;
        
        // Edge highlights
        let edge = smoothstep(col_w - 0.005, col_w, abs(p.x)) + smoothstep(col_h - 0.005, col_h, abs(p.y));
        color += vec3<f32>(0.5, 0.5, 0.6) * edge;
    }
    
    return vec4<f32>(color, alpha);
}
