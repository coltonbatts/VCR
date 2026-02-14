// VCR Standard Library Demo
#include "vcr:common"
#include "vcr:noise"
#include "vcr:sdf"
#include "vcr:raymarch"

fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    let time = u.time * 0.05;
    
    // Use common rotate3d
    let p_rot = rotate3d(p, vec3<f32>(time, time * 0.7, 0.0));
    
    // Use sdf primitives
    let d1 = sdTorus(p_rot, vec2<f32>(0.5, 0.2));
    
    // Use noise for displacement
    let displacement = noise2d(p.xy * 2.0 + time) * 0.1;
    
    return d1 + displacement;
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let cam_pos = vec3<f32>(0.0, 0.0, -2.5);
    let look_at = vec3<f32>(0.0, 0.0, 0.0);
    
    // Use raymarch template
    let result = raymarch_render(uv, u, cam_pos, look_at, 2.0);
    
    if (result.a > 0.0) {
        let time = u.time * 0.05;
        let color = hsv2rgb(vec3<f32>(fract(time * 0.1 + uv.x * 0.2), 0.8, result.r));
        return vec4<f32>(color, 0.95);
    }
    
    return vec4<f32>(0.0);
}
