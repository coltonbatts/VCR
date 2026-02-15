// Techie Globe Shader (WgpuShaderLayer Compatible)
// Raymarched sphere with procedural longitude/latitude grid and CRT scanlines.
// Pure Monochrome White - CRT Glitch & Interior Transparency.

#include "vcr:common"
#include "vcr:sdf"
#include "vcr:alpha"
#include "vcr:noise"

fn rotateY(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec3<f32>(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
}

// Longitude/Latitude Grid Pattern
fn gridPattern(p: vec3<f32>) -> f32 {
    let r = length(p);
    let phi = atan2(p.z, p.x); // Longitude
    let theta = acos(p.y / r);  // Latitude
    
    // Scale for grid frequency
    let longScale = 12.0;
    let latScale = 8.0;
    
    let gridLong = abs(sin(phi * longScale));
    let gridLat = abs(sin(theta * latScale));
    
    // Sharpen the grid lines
    let lineThickness = 0.05;
    let longLines = smoothstep(1.0 - lineThickness, 1.0, gridLong);
    let latLines = smoothstep(1.0 - lineThickness, 1.0, gridLat);
    
    return max(longLines, latLines);
}

// SDF for the scene
fn map(p_in: vec3<f32>, time: f32) -> f32 {
    let rot_p = rotateY(p_in, time * 0.15); // Slow spin
    return sdSphere(rot_p, 1.0);
}

fn calcNormalCustom(p: vec3<f32>, time: f32) -> vec3<f32> {
    let e = vec2<f32>(0.0005, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy, time) - map(p - e.xyy, time),
        map(p + e.yxy, time) - map(p - e.yxy, time),
        map(p + e.yyx, time) - map(p - e.yyx, time)
    ));
}

fn camera_ray_custom(uv: vec2<f32>, resolution: vec2<f32>, cam_pos: vec3<f32>, look_at: vec3<f32>, zoom: f32) -> vec3<f32> {
    let aspect = resolution.x / resolution.y;
    let p_ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let f = normalize(look_at - cam_pos);
    let r = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), f));
    let v_up = cross(f, r);
    return normalize(f * zoom + p_ndc.x * r + p_ndc.y * v_up);
}

fn raymarch_custom(ro: vec3<f32>, rd: vec3<f32>, time: f32) -> f32 {
    var t = 0.01;
    for (var i = 0; i < 128; i = i + 1) {
        let p = ro + rd * t;
        let d = map(p, time);
        if (abs(d) < 0.001) {
            return t;
        }
        if (t > 10.0) {
            break;
        }
        t = t + d;
    }
    return -1.0;
}

fn shade(uv_in: vec2<f32>, time: f32, resolution: vec2<f32>) -> vec4<f32> {
    var uv = uv_in;
    
    // CRT "Breaking Apart" Glitch Effect
    // Horizontal jitter based on scanlines and noise
    let glitch_noise = noise1d(time * 12.0);
    if (glitch_noise > 0.8) {
        let block_y = floor(uv.y * 32.0);
        let jitter = (noise1d(block_y + time * 50.0) - 0.5) * 0.02 * glitch_noise;
        uv.x += jitter;
    }
    
    // Subtle continuous jitter for "alive" feel
    uv.x += (noise1d(uv.y * 500.0 + time * 20.0) - 0.5) * 0.001;

    // Camera setup (Far back to avoid clipping)
    let cam_pos = vec3<f32>(0.0, 0.0, -4.5); 
    let look_at = vec3<f32>(0.0, 0.0, 0.0);
    let zoom = 2.0; 
    
    let rd = camera_ray_custom(uv, resolution, cam_pos, look_at, zoom);
    let t = raymarch_custom(cam_pos, rd, time);
    
    if (t < 0.0) {
        return miss();
    }
    
    let p = cam_pos + rd * t;
    let n = calcNormalCustom(p, time);
    let rot_pos = rotateY(p, time * 0.15);
    
    // Grid Logic
    let grid = gridPattern(rot_pos);
    
    // Monochrome White colors
    let baseColor = vec3<f32>(0.0, 0.0, 0.0); // Transparent interior
    let glowColor = vec3<f32>(1.0, 1.0, 1.0);  // Pure white
    
    // Lighting
    let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);
    let fresnel = rim * 1.0;
    
    // CRT Scanlines (horizontal bands)
    let scanline_frequency = 300.0;
    let scanline = sin(uv.y * scanline_frequency) * 0.05 + 0.95;
    
    // Combine Grid and Fresnel
    let mix_val = max(grid, fresnel);
    let finalRgb = mix(baseColor, glowColor, mix_val) * scanline;
    
    // Alpha Logic: interior is transparent, grid/fresnel/edges are visible
    let edge_dist = abs(map(p, time));
    let pixel_world = t / resolution.y;
    let edgeAlpha = clamp(0.5 - edge_dist / (pixel_world * 3.0), 0.0, 1.0);
    
    // Total alpha is the combination of grid, fresnel, and the sphere's edge
    let alpha = edgeAlpha * clamp(mix_val * 1.5, 0.0, 1.0);
    
    return out_rgba(finalRgb, alpha);
}
