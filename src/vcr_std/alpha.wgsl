// VCR Standard Library: Alpha-Correct Rendering Utilities
// Included via #include "vcr:alpha"
//
// VCR convention: shade() returns STRAIGHT alpha vec4(rgb, a).
// The blend pipeline handles compositing. These helpers ensure
// clean output: zero RGB where alpha is zero, smooth silhouettes,
// and correct premultiply/unpremultiply for intermediate work.

// Return a transparent-black miss pixel. Use for ray misses.
fn miss() -> vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}

// Compose straight-alpha output: clamps alpha, zeros RGB when alpha ~ 0.
// Use this as the final return from shade() for clean silhouettes.
fn out_rgba(rgb: vec3<f32>, a: f32) -> vec4<f32> {
    let alpha = clamp(a, 0.0, 1.0);
    // Kill RGB below perceptual threshold to prevent ghost fringe
    let mask = step(1.0 / 512.0, alpha);
    return vec4<f32>(rgb * mask, alpha * mask);
}

// Overload: compose with a base color and separate coverage
fn out_color(color: vec3<f32>, coverage: f32, opacity: f32) -> vec4<f32> {
    return out_rgba(color, coverage * opacity);
}

// Premultiply straight alpha for intermediate compositing
fn premultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

// Unpremultiply back to straight alpha (safe divide)
fn unpremultiply(c: vec4<f32>) -> vec4<f32> {
    if (c.a < 1.0 / 512.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(c.rgb / c.a, c.a);
}

// SDF-based coverage: smooth alpha falloff at silhouette edges.
// `dist` is signed distance (negative = inside), `pixel_size` is
// the approximate size of one pixel in SDF space (use 1.0/resolution.y
// or derive from ray t-distance).
fn sdf_coverage(dist: f32, pixel_size: f32) -> f32 {
    let half_band = max(pixel_size * 0.75, 0.0001);
    return clamp(0.5 - dist / half_band, 0.0, 1.0);
}

// Over-composite two straight-alpha colors (A over B).
fn alpha_over(front: vec4<f32>, back: vec4<f32>) -> vec4<f32> {
    let fa = front.a;
    let ba = back.a * (1.0 - fa);
    let out_a = fa + ba;
    if (out_a < 1.0 / 512.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>((front.rgb * fa + back.rgb * ba) / out_a, out_a);
}
