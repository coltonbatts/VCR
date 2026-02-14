// VCR Standard Library: Noise Functions
// Included via #include "vcr:noise"

fn hash1(n: f32) -> f32 {
    return fract(sin(n) * 43758.5453123);
}

fn noise1d(x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash1(i), hash1(i + 1.0), u);
}

// Simple 2D Gradient Noise
fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn noise2d(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i + vec2<f32>(0.0, 0.0)), 
                   hash2(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash2(i + vec2<f32>(0.0, 1.0)), 
                   hash2(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// Fractal Brownian Motion
fn fbm2d(p_input: vec2<f32>, octaves: i32) -> f32 {
    var p = p_input;
    var v = 0.0;
    var a = 0.5;
    let shift = vec2<f32>(100.0);
    // Rotating used for reducing directional artifacts
    let rot = mat2x2<f32>(cos(0.5), sin(0.5), -sin(0.5), cos(0.5));
    for (var i = 0; i < octaves; i = i + 1) {
        v = v + a * noise2d(p);
        p = rot * p * 2.0 + shift;
        a = a * 0.5;
    }
    return v;
}
