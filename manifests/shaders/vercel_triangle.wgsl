// Spinning Vercel Triangle
// Equilateral triangle on alpha transparency

fn sdEquilateralTriangle(p: vec2<f32>, r: f32) -> f32 {
    let k = 1.73205081; // sqrt(3)
    var p_mod = p;
    p_mod.x = abs(p_mod.x) - r;
    p_mod.y = p_mod.y + r/k;
    if (p_mod.x + k*p_mod.y > 0.0) {
        p_mod = vec2<f32>(p_mod.x - k*p_mod.y, -k*p_mod.x - p_mod.y) / 2.0;
    }
    p_mod.x -= clamp(p_mod.x, -2.0*r, 0.0);
    return -length(p_mod) * sign(p_mod.y);
}

fn rotate(p: vec2<f32>, angle: f32) -> vec2<f32> {
    let s = sin(angle);
    let c = cos(angle);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

fn shade(uv: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    let aspect = resolution.x / resolution.y;
    // Center and scale
    var p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    // Slow spin: 0.2 radians per second
    let angle = time * 0.2;
    p = rotate(p, angle);

    // Triangle size
    let size = 0.25;
    let dist = sdEquilateralTriangle(p, size);

    // Anti-aliased edges
    let smoothing = 2.0 / resolution.y;
    let mask = smoothstep(smoothing, 0.0, dist);

    if (mask < 0.001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Pure white triangle
    return vec4<f32>(1.0, 1.0, 1.0, mask);
}
