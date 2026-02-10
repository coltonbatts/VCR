# VCR Steerability Model

This guide explains how to make broad, predictable motion changes quickly without moving to a heavyweight GUI.

## Mental Model

Think of evaluation as:

1. Global controls (`params`, `seed`, `modulators`)
2. Group transforms + timing
3. Layer transforms + timing
4. Final composition

Small changes to `params` or one shared `modulator` can drive many layers coherently.

## 1) Global Parameters

Use top-level `params` for high-level controls:

```yaml
params:
  energy: 0.7
  tension: 0.4
  phase: 1.2
```

Expressions can reference params by name directly:

```yaml
rotation_degrees: "sin(t * 0.08 + phase) * (energy * 25)"
```

Rules:

- Params are scalar `f32` values.
- Names must be identifiers (`energy`, `phase_2`, etc.).
- `t` is reserved.

## 2) Expression Helpers

Built-ins:

- `clamp(x, min, max)`
- `lerp(a, b, t)`
- `smoothstep(edge0, edge1, x)`
- `easeInOut(x)`
- `sin(x)`, `cos(x)`, `abs(x)`
- `noise1d(x[, seed_offset])` deterministic and seedable
- `env(t)` or `env(t, attack, decay)`

Determinism:

- `noise1d` uses manifest `seed`.
- Same manifest + seed + frame index => same result.

## 3) Modulators

A modulator is a reusable scalar signal:

```yaml
modulators:
  wobble:
    expression: "noise1d(t * 0.2) * energy"
```

Attach it to a layer (or group) with property weights:

```yaml
modulators:
  - source: wobble
    weights:
      x: 24
      y: 10
      rotation: 6
      opacity: 0.08
```

This is the main steerability primitive for coherent multi-property motion.

## 4) Groups and Timing

Groups let multiple layers inherit a shared transform/timeline.

```yaml
groups:
  - id: rig
    position: [120, 80]
    time_offset: 0.2
    time_scale: 1.1
```

Layers opt in via `group`:

```yaml
- id: card
  group: rig
```

Timing controls on groups or layers:

- `start_time` (seconds)
- `end_time` (seconds)
- `time_offset` (seconds)
- `time_scale` (> 0)

## 5) Fast Iteration Workflow

Use these while designing motion:

```bash
# 1. Validate
cargo run -- check scene.vcr

# 2. Fast preview frames (no ffmpeg required)
cargo run -- preview scene.vcr --image-sequence -o preview_frames

# 3. One frame deep inspection
cargo run -- render-frame scene.vcr --frame 72 -o frame_72.png

# 4. Live loop while editing manifest
cargo run -- watch scene.vcr --image-sequence -o preview_frames

# 5. Debug evaluated values
cargo run -- dump scene.vcr --frame 72

# 6. Lint for common issues
cargo run -- lint scene.vcr
```

## 6) Debugging and Stability

Layer fields for debugging:

- `id` (required stable identifier)
- `name` (human-readable)
- `stable_id` (optional external cross-ref)

Diagnostics:

- `dump` prints resolved z-order, visibility, transforms, opacity.
- `lint` flags likely-unreachable layers.
- Validation errors include field context and parse location for YAML failures.

## 7) Backward Compatibility

- Existing manifests continue to work.
- New fields are additive with defaults.
- `version` defaults to `1` and is validated for schema stability.
