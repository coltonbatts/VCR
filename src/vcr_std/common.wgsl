// VCR Standard Library: Common Utilities
// Included via #include "vcr:common"

const PI: f32 = 3.14159265359;
const TWO_PI: f32 = 6.28318530718;

fn rotate2d(p: vec2<f32>, a: f32) -> vec2<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

fn rotate3d(p: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
    var p_out = p;
    // Y-axis
    let cy = cos(r.y); let sy = sin(r.y);
    p_out = vec3<f32>(p_out.x * cy - p_out.z * sy, p_out.y, p_out.x * sy + p_out.z * cy);
    // X-axis
    let cx = cos(r.x); let sx = sin(r.x);
    p_out = vec3<f32>(p_out.x, p_out.y * cx - p_out.z * sx, p_out.y * sx + p_out.z * cx);
    // Z-axis
    let cz = cos(r.z); let sz = sin(r.z);
    p_out = vec3<f32>(p_out.x * cz - p_out.y * sz, p_out.x * sz + p_out.y * cz, p_out.z);
    return p_out;
}

fn hsv2rgb(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, clamp(p - k.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}
