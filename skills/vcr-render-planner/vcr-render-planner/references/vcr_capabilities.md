# VCR Capabilities Reference

## CLI Commands

```bash
vcr check <file>                    # Validate manifest (no GPU init, fast)
vcr build <file> -o out.mov         # Full render to ProRes video
vcr render-frame <file> --frame N -o frame.png  # Single frame to PNG
vcr render-frames <file> --start-frame 0 --frames 30 -o renders/seq/
vcr preview <file> -o preview.mov --scale 0.5   # Quick half-res preview
vcr watch <file> -o preview.mov --scale 0.5     # Live reload on changes
vcr lint <file>                     # Deep lint (unreachable layers)
vcr dump <file> --frame 30          # Layer states at frame
vcr params <file> --json            # List parameters
vcr explain <file> --set speed=2.0  # Resolved manifest state
vcr determinism-report <file> --frame 0 --json  # Frame hash
vcr ascii library                   # List curated ASCII animations
vcr doctor                          # Verify dependencies
```

### Global Flags

```
--quiet              Suppress non-essential logs
--backend auto       auto (default) | software | gpu
--set NAME=VALUE     Parameter override (repeatable)
--start-frame N      Start frame
--frames N           Frame count
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Usage/argument error |
| 3 | Manifest validation error |
| 4 | Missing dependency (FFmpeg) |
| 5 | I/O error |

## Manifest Top-Level Structure

```yaml
version: 1                    # Must be 1
environment:
  resolution: { width: 1920, height: 1080 }  # 1-8192 per dimension
  fps: 30                     # > 0
  duration: 3.0               # seconds (float) or { frames: 90 }
  color_space: rec709         # rec709 | rec2020 | display_p3
seed: 0                       # Deterministic randomness
params: {}                    # Typed parameters
modulators: {}                # Named expressions
groups: []                    # Hierarchical transforms
layers: []                    # At least one required
post: []                      # Post-processing (GPU only)
```

## Layer Types

### Common Properties (all layers)

id, name, z_index, anchor (top_left|center), position, pos_x, pos_y, scale,
rotation_degrees, opacity, start_time, end_time, time_offset, time_scale,
group, modulators.

### Procedural Layer

Kinds: `solid_color`, `gradient`, `circle`, `rounded_rect`, `ring`, `line`,
`triangle`, `polygon`. All support AnimatableColor (per-channel expressions).

### Text Layer

```yaml
text:
  content: "TEXT"
  font_family: "GeistPixel-Line"  # Line|Square|Grid|Circle|Triangle
  font_size: 48
  letter_spacing: 0
  color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
```

### Image Layer

```yaml
image:
  path: "assets/logo.png"    # Relative paths only
```

### Shader Layer (GPU only)

```yaml
shader:
  fragment: |
    fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> { ... }
  uniforms:
    key: value               # Up to 8, packed into custom[0..1] vec4s
```

### ASCII Layer

```yaml
ascii:
  grid: { rows: 5, columns: 20 }
  cell: { width: 16, height: 24 }
  font_variant: geist_pixel_regular
  foreground: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
  background: { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
  inline: ["line1", "line2"]  # or path: "art/file.txt"
  reveal: { kind: row_major, start_frame: 0, frames_per_cell: 1 }
```

### Animation Engine Layer

```yaml
animation_engine:
  clip_name: "library/category/name"
  fps: 12
  colors:
    foreground: [255, 255, 255, 255]
    background: [0, 0, 0, 0]
```

## ScalarProperty Formats

```yaml
opacity: 0.75                          # Static
opacity: "smoothstep(0.0, 1.0, t/30)" # Expression
opacity:                               # Keyframe
  start_frame: 0
  end_frame: 30
  from: 0.0
  to: 1.0
  easing: ease_in_out                  # linear|ease_in|ease_out|ease_in_out
```

## Expression Functions

sin, cos, abs, floor, ceil, round, fract, clamp, lerp, smoothstep, step,
easeinout, saw, tri, random, noise1d, glitch, env.

Variable `t` = frame number (float). Parameters accessible by name.

## Post-Processing (GPU only)

| Shader | Parameters |
|--------|-----------|
| passthrough | (none) |
| levels | gamma, lift, gain |
| sobel | strength |

## Parameter Types

float, int, bool, vec2, color. CLI: `--set name=value`.

## Determinism

Same manifest + params + seed + backend = identical frames.
Software: bitwise identical same machine. GPU: not guaranteed across hardware.

## Dependencies

FFmpeg in PATH, Rust toolchain (building from source), GPU (Metal on macOS) for
gpu backend / shaders / post-processing.
