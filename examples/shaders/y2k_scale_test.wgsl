// Scale calibration test - draws a circle of radius 0.40 in the standard VCR coord space
fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let aspect = u.resolution.x / u.resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let d = length(p);
    // Circle at R=0.40 — should fill 80% of frame height
    let circle = smoothstep(0.402, 0.398, d);
    // Crosshair
    let hline = smoothstep(0.003, 0.0, abs(p.y));
    let vline = smoothstep(0.003, 0.0, abs(p.x));
    // Half-height marker at 0.5
    let half = smoothstep(0.405, 0.395, abs(d - 0.5));
    let col = vec3<f32>(circle) + vec3<f32>(1.0, 0.0, 0.0) * (hline + vline) + vec3<f32>(0.0, 1.0, 0.0) * half;
    let alpha = max(circle, max(hline + vline, half)) * 0.8;
    return vec4<f32>(col, alpha);
}
