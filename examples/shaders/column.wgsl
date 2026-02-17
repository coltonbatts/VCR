// Dreamcore Column — Classical column on alpha
// Standalone element: centered, floats in space

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);

    // Column shaft — vertical rectangle with slight taper
    let col_width_bottom = 0.06;
    let col_width_top = 0.05;
    let col_height = 0.35;
    let col_y_center = 0.0;

    // Normalized y position along column (-1 at bottom, 1 at top)
    let y_norm = (p.y - col_y_center) / col_height;

    // Width tapers from bottom to top (entasis)
    let col_width = mix(col_width_bottom, col_width_top, y_norm * 0.5 + 0.5);

    var alpha = 0.0;
    var color = vec3<f32>(0.0);

    // Shaft
    if (abs(p.x) < col_width && abs(p.y - col_y_center) < col_height) {
        // Fluting effect — vertical grooves
        let flute = abs(sin(p.x * 80.0)) * 0.15;
        let light = 0.7 + 0.3 * (1.0 - abs(p.x) / col_width); // center is brighter
        let marble = vec3<f32>(0.85, 0.82, 0.88) * (light - flute);

        // Subtle time-based color shift
        let shift = sin(time * 0.5) * 0.05;
        color = marble + vec3<f32>(shift, 0.0, -shift);
        alpha = 1.0;
    }

    // Capital (top piece) — wider block
    let cap_y = col_y_center + col_height;
    let cap_height = 0.035;
    let cap_width = col_width_top * 1.6;
    if (abs(p.x) < cap_width && p.y > cap_y - 0.005 && p.y < cap_y + cap_height) {
        color = vec3<f32>(0.9, 0.87, 0.92);
        alpha = 1.0;
    }

    // Base — wider block at bottom
    let base_y = col_y_center - col_height;
    let base_height = 0.03;
    let base_width = col_width_bottom * 1.5;
    if (abs(p.x) < base_width && p.y < base_y + 0.005 && p.y > base_y - base_height) {
        color = vec3<f32>(0.8, 0.78, 0.85);
        alpha = 1.0;
    }

    // Soft edge
    if (alpha > 0.0) {
        // Edge fade for anti-aliasing feel
        let edge_fade = smoothstep(0.0, 0.003, min(
            col_width - abs(p.x),
            col_height - abs(p.y - col_y_center)
        ));
        alpha *= edge_fade;
    }

    // Faint ambient glow
    let glow_dist = length(p);
    let glow = exp(-glow_dist * 4.0) * 0.06;
    if (alpha < 0.01) {
        color = vec3<f32>(0.7, 0.6, 0.9);
        alpha = glow;
    }

    alpha = clamp(alpha, 0.0, 1.0);
    if (alpha < 0.01) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(color, alpha);
}
