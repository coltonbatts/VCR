// VCR Standard Library: Matcap Shading
// Included via #include "vcr:matcap"
//
// Matcap (Material Capture) maps a view-space normal to a 2D lookup
// to produce stylized, view-dependent shading without explicit lights.
// Since VCR has no texture sampling in custom shaders, we provide
// procedural matcap generators.

// Convert a world-space normal + view direction into matcap UV [0,1]^2
fn matcap_uv(normal: vec3<f32>, view_dir: vec3<f32>) -> vec2<f32> {
    let r = normalize(reflect(view_dir, normal));
    let m = 2.0 * sqrt(r.x * r.x + r.y * r.y + (r.z + 1.0) * (r.z + 1.0));
    return vec2<f32>(r.x / m + 0.5, r.y / m + 0.5);
}

// Procedural matcap: smooth clay/ceramic look
// Returns an RGB color given a matcap UV
fn matcap_clay(muv: vec2<f32>, base_color: vec3<f32>) -> vec3<f32> {
    let center = muv - 0.5;
    let d = length(center);
    // Diffuse-like falloff
    let diffuse = 1.0 - smoothstep(0.0, 0.6, d);
    // Specular highlight near top-left
    let spec_pos = muv - vec2<f32>(0.35, 0.3);
    let spec = exp(-dot(spec_pos, spec_pos) * 18.0);
    // Rim darkening
    let rim = smoothstep(0.35, 0.55, d);
    let color = base_color * (diffuse * 0.85 + 0.15) + vec3<f32>(spec * 0.6);
    return mix(color, base_color * 0.15, rim);
}

// Procedural matcap: metallic chrome look
fn matcap_chrome(muv: vec2<f32>) -> vec3<f32> {
    let center = muv - 0.5;
    let d = length(center);
    let angle = atan2(center.y, center.x);
    // Metallic bands based on angle
    let band = sin(angle * 3.0 + d * 8.0) * 0.3 + 0.5;
    // Bright highlight
    let spec_pos = muv - vec2<f32>(0.35, 0.3);
    let spec = exp(-dot(spec_pos, spec_pos) * 12.0);
    // Edge darkening
    let edge = 1.0 - smoothstep(0.3, 0.52, d);
    let base = vec3<f32>(band * edge);
    return base + vec3<f32>(spec * 0.8);
}

// Procedural matcap: warm/cool hemisphere shading
fn matcap_hemisphere(muv: vec2<f32>, warm: vec3<f32>, cool: vec3<f32>) -> vec3<f32> {
    let t = clamp(muv.y, 0.0, 1.0);
    let base = mix(warm, cool, t);
    // Soft highlight
    let center = muv - 0.5;
    let d = length(center);
    let highlight = exp(-d * d * 10.0) * 0.3;
    let rim = smoothstep(0.35, 0.52, d);
    return base * (1.0 - rim * 0.5) + vec3<f32>(highlight);
}

// Full matcap shade: takes normal, ray direction, and a base color.
// Uses the clay matcap by default. Convenience function.
fn matcap_shade(normal: vec3<f32>, rd: vec3<f32>, base_color: vec3<f32>) -> vec3<f32> {
    let muv = matcap_uv(normal, rd);
    return matcap_clay(muv, base_color);
}
