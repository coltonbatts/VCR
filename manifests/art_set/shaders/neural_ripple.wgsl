// Neural Pulse Ripple
// Liquid metal/water surface with pulsing light on alpha

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    let u = f * f * (3.0 - 2.0 * f);
    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let resolution = uniforms.resolution;
    let aspect = resolution.x / resolution.y;
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    
    let dist = length(p);
    let ripple = sin(dist * 20.0 - time * 4.0) * 0.5 + 0.5;
    let n = noise(p * 5.0 + time * 0.2);
    
    let mask = smoothstep(0.45, 0.4, dist);
    let bright = pow(ripple * n, 3.0) * 2.0;
    
    let col = mix(vec3<f32>(0.05, 0.1, 0.2), vec3<f32>(0.4, 0.8, 1.0), bright);
    let alpha = mask * (0.2 + bright * 0.8);
    
    return vec4<f32>(col * alpha, alpha);
}
