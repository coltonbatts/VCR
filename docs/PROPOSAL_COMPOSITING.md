# Proposal: Track Mattes, Nested Compositions, Motion Blur

Status: proposal, not implemented. It builds on the seconds-based time model
(`docs/PARAMS.md`) and the linear-light premultiplied pipeline (`docs/COLOR_PIPELINE.md`).
All three features need both of those.

## Shared prerequisites

All three features need the same two changes, so they should land first as one refactor.

1. **Evaluate at continuous time, not at a `u32` frame index.**
   `evaluate_layer_state`, `layer_source_frame`, and both renderers take `frame_index: u32`.
   Sub-frame samples (motion blur) and child timelines at a different fps (nested
   comps) need `evaluate_at(time: KeyTime)`, with a fractional frame or seconds.
   `TimingControls::remap_frame` and `ExpressionContext` already work in `f32`, so the change
   is mostly signatures. The exceptions are ascii reveals and `ShaderUniform.frame`, which stay
   integer and use `floor`.
2. **Offscreen float layers.** Today every layer draws straight into the one frame
   accumulator. Mattes and precomps need to render a layer (or a whole scene) into its own
   premultiplied-linear float target and read it back as an input. GPU: `Rgba16Float`
   textures from a small per-frame pool. Software: an `f32` image type alongside `Texels`,
   which is 8-bit and must not be used for intermediates.

## 1. Track mattes and masks

Schema (per layer):

```yaml
- id: title
  matte: { source: title_matte, mode: alpha }   # alpha | alpha_inverted | luma | luma_inverted
- id: title_matte
  matte_only: true                               # evaluated, never composited directly
```

Shape masks are sugar for an anonymous `matte_only` procedural layer:
`mask: { kind: rounded_rect, center: ..., size: ..., invert: false }`.

Timeline:
- Build a layer dependency graph at load: the matte source must exist, it cannot be its own
  matte transitively, and it may sit at any `z_index`.
- Evaluate a matte-only layer's state (transform, opacity, group chain, timing window) every
  frame even though it is not drawn. `vcr lint`'s unreachable-layer check must treat
  "used as a matte" as reachable.

Renderer, with one canonical definition for both backends:
- Render the matte source into a frame-sized float target.
- Coverage `m` per pixel: for `alpha`, `m = matte.a`. For `luma`,
  `m = dot(unpremultiply(matte.rgb), BT709_LUMA_LINEAR) * matte.a`. The inverted modes use `1 - m`.
- The matted layer composites `src * m` instead of `src`. On the GPU that is a second
  texture binding in the blend shader, read with `textureLoad` at the fragment position, so
  the matted layer itself needs no extra pass. The software backend multiplies in
  `composite_texels`.
- Extend `tests/backend_parity.rs` with alpha and luma mattes on a rotated source.

## 2. Nested compositions

Schema:

```yaml
- id: lower_third
  composition:
    path: comps/lower_third.vcr
    params: { accent_color: "#4FE1B8" }       # applied like --set on the child
    outside: hold                              # hold | transparent | loop
```

Loading:
- Load the child with `load_and_validate_manifest_with_options`, passing its params as
  overrides. Detect cycles with a stack of canonical paths. The child's assets resolve inside
  the child's directory, and the child manifest must itself be inside the parent's directory,
  so the existing path-escape rule extends naturally.
- **Hashing:** `manifest_hash` covers only the top manifest's raw text today. It must fold in
  each child's resolved hash, or editing a child would not change the parent's hash, which
  breaks the determinism promise and any render cache keyed on it.

Timeline:
- The precomp layer's own timing controls give a parent-local time in seconds. That maps to
  child time `t_child = local_seconds`, i.e. `frame_child = t_child * child.fps`, which is
  usually fractional. This is why prerequisite 1 is required.
- The child keeps its own `version`, so a v1 child still sees frame-based `t` at its own fps.
- Past the child's duration, `outside` decides: hold the last frame, go transparent, or loop.

Renderer:
- The child is a nested `Renderer` that shares the GPU device and queue. It renders into a
  float texture at the child's resolution **without** the resolve pass (no sRGB encode or
  quantization inside the tree). The parent then treats that texture as a normal layer
  source, so transform, opacity and mattes all apply.
- Static children render once. Otherwise results are memoized per child time.
- Metadata lists each child path (relative) and resolved hash. Child warnings bubble up.

## 3. Sub-frame motion blur

Schema:

```yaml
environment:
  motion_blur: { shutter_angle: 180, shutter_phase: -90, samples: 8 }
layers:
  - id: hud
    motion_blur: false        # per-layer opt-out
```

Timeline:
- For output frame `n`, sample times are
  `t_i = (n + (shutter_phase + shutter_angle * (i + 0.5) / samples) / 360) / fps`, where
  `i = 0..samples`. That is a fixed stratified pattern, with no randomness, so renders stay
  deterministic.
- Every animated input is evaluated per sample: transforms, opacity, keyframes,
  expressions, procedural sources and ascii reveals. `frame` becomes fractional within the
  shutter. Document `random(floor(frame))` for per-frame noise.

Renderer:
- Clear a second float accumulator. For each sample, render the scene into the normal
  accumulator and add it with weight `1/samples`. Then resolve once. Averaging only works
  in linear premultiplied space; under the old gamma pipeline, blurred edges would have
  darkened.
- Optimization: split layers into "static across the shutter", which render once at weight
  1, and "moving", which are sampled. Cost is roughly `samples ×` for moving layers only.
- Software parity: same sample times, same averaging order (sum in `f32`, then scale). The
  parity tolerance should hold, because each sample is already within tolerance and
  averaging does not amplify error.
- The blur settings are in the manifest, so they are covered by the manifest hash.

## Suggested order

1. Continuous-time evaluation plus offscreen float targets (prerequisites).
2. Motion blur. This is the smallest surface area, and it exercises continuous time and
   float accumulation end to end.
3. Mattes, which need the dependency graph and a second texture binding.
4. Nested compositions, which need the recursive renderer, hash folding and the loader changes.
