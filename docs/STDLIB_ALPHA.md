# VCR Standard Library: Alpha-Correct 3D Rendering

## Overview

The VCR standard library (`vcr_std`) provides WGSL modules for alpha-correct 3D rendering via SDF raymarching. All modules are bundled into the binary and included via the `#include "vcr:<module>"` preprocessor directive.

## Modules

| Module | Include | Purpose |
|--------|---------|---------|
| `common` | `#include "vcr:common"` | Constants (PI, TWO_PI), rotation, HSV conversion |
| `noise` | `#include "vcr:noise"` | Deterministic hash, value noise, FBM |
| `sdf` | `#include "vcr:sdf"` | Signed distance primitives + boolean ops |
| `raymarch` | `#include "vcr:raymarch"` | Raymarching loop, normals, camera, silhouette AA |
| `alpha` | `#include "vcr:alpha"` | Alpha helpers: `out_rgba`, `miss`, coverage, compositing |
| `matcap` | `#include "vcr:matcap"` | Procedural matcap shading (clay, chrome, hemisphere) |

## Alpha Convention

VCR custom shaders return **straight alpha**: `vec4(rgb, a)` where RGB is NOT premultiplied by alpha. The blend pipeline and ProRes encoder handle conversion.

### Rules

1. **Miss pixels** must return `vec4(0.0)` — use `miss()` from `vcr:alpha`
2. **Hit pixels** must have RGB = 0 when alpha = 0 — use `out_rgba(color, alpha)` which enforces this
3. **Silhouette edges** should use `silhouette_alpha()` from `vcr:raymarch` for smooth falloff
4. **No ghost fringe** — `out_rgba()` kills RGB below a 1/512 alpha threshold

### Key Functions (vcr:alpha)

```wgsl
fn miss() -> vec4<f32>                          // Transparent zero
fn out_rgba(rgb: vec3<f32>, a: f32) -> vec4<f32> // Clean straight-alpha output
fn out_color(color: vec3<f32>, coverage: f32, opacity: f32) -> vec4<f32>
fn sdf_coverage(dist: f32, pixel_size: f32) -> f32  // SDF silhouette smoothing
fn alpha_over(front: vec4<f32>, back: vec4<f32>) -> vec4<f32>  // A-over-B composite
fn premultiply(c: vec4<f32>) -> vec4<f32>       // For intermediate work
fn unpremultiply(c: vec4<f32>) -> vec4<f32>     // Safe divide
```

## Raymarching (vcr:raymarch)

Requires user to define `fn map(p: vec3<f32>, u: ShaderUniforms) -> f32` (the scene SDF).

### Structured API

```wgsl
struct MarchResult { hit: bool, pos: vec3<f32>, t: f32, dist: f32, steps: i32 }

fn camera_ray(uv, resolution, cam_pos, look_at, zoom) -> vec3<f32>
fn raymarch(ro, rd, u) -> MarchResult
fn calcNormal(p, u) -> vec3<f32>
fn silhouette_alpha(mr, resolution) -> f32
```

### Convenience

```wgsl
fn raymarch_render(uv, u, cam_pos, look_at, zoom) -> vec4<f32>  // Full march+shade
```

## Matcap Shading (vcr:matcap)

Procedural matcap generators — no texture sampling required.

```wgsl
fn matcap_uv(normal, view_dir) -> vec2<f32>     // View-space normal → UV
fn matcap_clay(muv, base_color) -> vec3<f32>     // Warm ceramic look
fn matcap_chrome(muv) -> vec3<f32>               // Metallic chrome
fn matcap_hemisphere(muv, warm, cool) -> vec3<f32> // Warm/cool split
fn matcap_shade(normal, rd, base_color) -> vec3<f32> // Convenience (clay default)
```

## SDF Primitives (vcr:sdf)

```wgsl
fn sdSphere(p, radius) -> f32
fn sdBox(p, half_extents) -> f32
fn sdTorus(p, vec2(major_r, minor_r)) -> f32
fn sdCapsule(p, a, b, radius) -> f32
fn opUnion(d1, d2) -> f32
fn opSubtraction(d1, d2) -> f32
fn opIntersection(d1, d2) -> f32
fn opSmoothUnion(d1, d2, k) -> f32
```

## Example: Minimal Alpha-Correct 3D Shader

```wgsl
#include "vcr:common"
#include "vcr:sdf"
#include "vcr:alpha"
#include "vcr:matcap"
#include "vcr:raymarch"

fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    let rp = rotate3d(p, vec3<f32>(0.0, u.time * 0.5, 0.0));
    return sdBox(rp, vec3<f32>(0.8));
}

fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
    let cam = vec3<f32>(0.0, 1.0, 3.5);
    let rd = camera_ray(uv, u.resolution, cam, vec3<f32>(0.0), 2.0);
    let mr = raymarch(cam, rd, u);
    if (!mr.hit) { return miss(); }
    let n = calcNormal(mr.pos, u);
    let color = matcap_shade(n, rd, vec3<f32>(0.7, 0.3, 0.2));
    return out_rgba(color, silhouette_alpha(mr, u.resolution));
}
```

## Verification

### Visual Validation

Render the demo and verify clean alpha by compositing over contrasting backgrounds:

```bash
vcr render-frame examples/std_alpha_demo.vcr --frame 30 --backend gpu -o renders/test.png
```

Open in an image editor and place over:
- Black background — no bright fringe at edges
- White background — no dark fringe at edges
- Checkerboard — smooth silhouette, no stairstepping
- Saturated magenta — no color bleeding

### Determinism

Same machine + backend produces identical output:

```bash
vcr render-frame examples/std_alpha_demo.vcr --frame 0 --backend gpu -o renders/a.png
vcr render-frame examples/std_alpha_demo.vcr --frame 0 --backend gpu -o renders/b.png
diff renders/a.png renders/b.png  # Should be identical
```

### ProRes 4444 Export

```bash
vcr build examples/std_alpha_demo.vcr -o renders/std_alpha_demo.mov --backend gpu
```

Import into DaVinci Resolve, Final Cut Pro, or After Effects. Place over a solid color track. Verify zero edge artifacts.

## Architecture Notes

- Shaders run as `Layer::Shader` with the `shade(uv, ShaderUniforms) -> vec4<f32>` contract
- The preprocessor expands `#include "vcr:*"` directives before WGSL compilation
- Module ordering matters: include dependencies before dependents (e.g., `sdf` before `raymarch`)
- All noise functions use `sin`-based hashing — deterministic across runs on the same GPU
- Software backend renders shader layers as transparent (GPU-only feature)
