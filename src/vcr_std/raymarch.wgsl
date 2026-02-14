// VCR Standard Library: Raymarching Template (Alpha-Correct)
// Included via #include "vcr:raymarch"
// Requirement: User must define fn map(p: vec3<f32>, u: ShaderUniforms) -> f32

const MARCH_MAX_STEPS: i32 = 128;
const MARCH_MAX_DIST: f32 = 40.0;
const MARCH_EPSILON: f32 = 0.0003;
const NORMAL_EPSILON: f32 = 0.0005;

fn calcNormal(p: vec3<f32>, u: ShaderUniforms) -> vec3<f32> {
    let e = vec2<f32>(NORMAL_EPSILON, 0.0);
    return normalize(vec3<f32>(
        map(p + e.xyy, u) - map(p - e.xyy, u),
        map(p + e.yxy, u) - map(p - e.yxy, u),
        map(p + e.yyx, u) - map(p - e.yyx, u)
    ));
}

// Build a camera ray from UV, camera position, look-at target, and zoom/focal length.
fn camera_ray(uv: vec2<f32>, resolution: vec2<f32>, cam_pos: vec3<f32>, look_at: vec3<f32>, zoom: f32) -> vec3<f32> {
    let aspect = resolution.x / resolution.y;
    let p_ndc = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let f = normalize(look_at - cam_pos);
    let r = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), f));
    let v_up = cross(f, r);
    return normalize(f * zoom + p_ndc.x * r + p_ndc.y * v_up);
}

// Result of a raymarch: hit position, distance traveled, closest SDF value, step count
struct MarchResult {
    hit: bool,
    pos: vec3<f32>,
    t: f32,
    dist: f32,
    steps: i32,
}

// Core raymarch loop. Returns structured result for flexible shading.
fn raymarch(ro: vec3<f32>, rd: vec3<f32>, u: ShaderUniforms) -> MarchResult {
    var result: MarchResult;
    result.hit = false;
    result.t = 0.01;
    result.steps = 0;
    result.dist = MARCH_MAX_DIST;

    for (var i = 0; i < MARCH_MAX_STEPS; i = i + 1) {
        let p = ro + rd * result.t;
        result.dist = map(p, u);
        result.steps = i + 1;
        if (abs(result.dist) < MARCH_EPSILON) {
            result.hit = true;
            result.pos = p;
            break;
        }
        if (result.t > MARCH_MAX_DIST) {
            break;
        }
        result.t = result.t + result.dist;
    }

    if (result.hit) {
        result.pos = ro + rd * result.t;
    }
    return result;
}

// Compute silhouette alpha from march result using SDF distance.
// pixel_size should be approx 1.0 / resolution.y scaled by distance.
fn silhouette_alpha(mr: MarchResult, resolution: vec2<f32>) -> f32 {
    if (!mr.hit) {
        return 0.0;
    }
    // Approximate pixel footprint in world space at hit distance
    let pixel_world = mr.t / resolution.y;
    let half_band = max(pixel_world * 1.5, MARCH_EPSILON * 4.0);
    return clamp(0.5 - mr.dist / half_band, 0.0, 1.0);
}

// Full convenience render: march, shade with basic diffuse+rim, return straight alpha.
// User can call raymarch() + silhouette_alpha() separately for custom shading.
fn raymarch_render(uv: vec2<f32>, u: ShaderUniforms, cam_pos: vec3<f32>, look_at: vec3<f32>, zoom: f32) -> vec4<f32> {
    let rd = camera_ray(uv, u.resolution, cam_pos, look_at, zoom);
    let mr = raymarch(cam_pos, rd, u);

    if (!mr.hit) {
        return vec4<f32>(0.0);
    }

    let n = calcNormal(mr.pos, u);
    let light_dir = normalize(vec3<f32>(1.0, 2.0, -1.0));
    let diff = max(dot(n, light_dir), 0.0);
    let rim = pow(1.0 - max(dot(n, -rd), 0.0), 3.0);

    let shading = diff * 0.7 + 0.3 + rim * 0.5;
    let alpha = silhouette_alpha(mr, u.resolution);
    let rgb = vec3<f32>(shading);

    // Clean straight-alpha output: zero RGB where alpha is negligible
    let mask = step(1.0 / 512.0, alpha);
    return vec4<f32>(rgb * mask, alpha * mask);
}
