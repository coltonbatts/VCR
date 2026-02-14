// VCR Standard Library Alpha Demo
// Demonstrates: SDF raymarching, matcap shading, noise distortion, clean alpha
//
// Uses Layer::Shader (shade function pattern) with ShaderUniforms.
// Custom uniforms: speed (rotation speed), distort (noise amplitude)

#include "vcr:common"
#include "vcr:noise"
#include "vcr:sdf"
#include "vcr:alpha"
#include "vcr:matcap"
#include "vcr:raymarch"

// Scene SDF: smooth union of sphere + box + torus, with noise distortion
fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    let time = u.time;
    let speed = u.custom[0].x;     // rotation speed
    let distort = u.custom[0].y;   // noise distortion amount

    // Rotate the whole scene
    let angle = time * speed;
    let rp = rotate3d(p, vec3<f32>(angle * 0.7, angle, angle * 0.3));

    // Sphere with noise displacement
    let noise_val = fbm2d(rp.xy * 3.0 + vec2<f32>(time * 0.5), 3) * distort;
    let sphere = sdSphere(rp, 0.8) + noise_val;

    // Box orbiting the sphere
    let box_offset = vec3<f32>(cos(time * speed * 1.5) * 1.6, sin(time * speed * 1.2) * 0.5, sin(time * speed * 1.5) * 1.6);
    let box_d = sdBox(rp - box_offset, vec3<f32>(0.25, 0.25, 0.25));

    // Torus at the base
    let torus_p = rp - vec3<f32>(0.0, -0.3, 0.0);
    let torus_d = sdTorus(torus_p, vec2<f32>(1.2, 0.15));

    // Smooth union of all three
    var d = opSmoothUnion(sphere, box_d, 0.4);
    d = opSmoothUnion(d, torus_d, 0.3);
    return d;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let cam_pos = vec3<f32>(0.0, 1.0, 4.0);
    let look_at = vec3<f32>(0.0, 0.0, 0.0);
    let rd = camera_ray(uv, u.resolution, cam_pos, look_at, 2.0);

    let mr = raymarch(cam_pos, rd, u);

    if (!mr.hit) {
        return miss();
    }

    let n = calcNormal(mr.pos, u);
    let alpha = silhouette_alpha(mr, u.resolution);

    // Matcap shading with warm terracotta base
    let base_color = vec3<f32>(0.85, 0.45, 0.25);
    let color = matcap_shade(n, rd, base_color);

    // Add subtle noise-based color variation
    let noise_tint = fbm2d(mr.pos.xy * 2.0 + vec2<f32>(u.time * 0.2), 2) * 0.15;
    let final_color = color + vec3<f32>(noise_tint * 0.5, -noise_tint * 0.3, noise_tint);

    return out_rgba(final_color, alpha);
}
