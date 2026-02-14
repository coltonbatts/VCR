// Material: Glass Effect
// Inspired by drei's MeshTransmissionMaterial
// Creates frosted glass with chromatic aberration
//
// Uses built-in VCR uniforms only (no custom parameters needed)

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    // Fixed parameters (can be adjusted by editing shader)
    let blur_amount = 0.02;      // blur amount
    let aberration = 0.01;       // chromatic aberration strength
    
    // Create glass shape (rounded rectangle)
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Rounded rectangle SDF
    let size = vec2<f32>(0.6, 0.4);
    let radius = 0.1;
    let d = abs(p) - size + radius;
    let dist = length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
    
    if (dist < 0.0) {
        // Multi-sample blur with chromatic aberration
        let samples = 8;
        var color_r = 0.0;
        var color_g = 0.0;
        var color_b = 0.0;
        
        for (var i = 0; i < samples; i++) {
            let angle = f32(i) * 6.28318 / f32(samples);
            let offset = vec2<f32>(cos(angle), sin(angle)) * blur_amount;
            
            // Sample with chromatic aberration
            let uv_r = uv + offset + vec2<f32>(aberration, 0.0);
            let uv_g = uv + offset;
            let uv_b = uv + offset - vec2<f32>(aberration, 0.0);
            
            // Create a gradient background for glass to refract
            color_r += mix(0.2, 0.8, uv_r.y);
            color_g += mix(0.3, 0.9, uv_g.y);
            color_b += mix(0.4, 1.0, uv_b.y);
        }
        
        color_r /= f32(samples);
        color_g /= f32(samples);
        color_b /= f32(samples);
        
        // Add frosted glass tint
        let glass_tint = vec3<f32>(0.95, 0.98, 1.0);
        let final_color = vec3<f32>(color_r, color_g, color_b) * glass_tint;
        
        // Edge highlight for glass effect
        let edge_dist = abs(dist) / 0.02;
        let edge_highlight = smoothstep(1.0, 0.0, edge_dist) * 0.3;
        
        // Fresnel-like effect
        let center_dist = length(p);
        let fresnel = pow(center_dist / 0.7, 2.0) * 0.2;
        
        return vec4<f32>(final_color + edge_highlight + fresnel, 0.85);
    }
    
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
