# VCR Examples

Start here and work your way down. Each example builds on the last.

## Learning Path

| # | File | What you'll learn |
|---|------|-------------------|
| 0 | [`hello_world.vcr`](hello_world.vcr) | Minimal scene — one circle, one background |
| 1 | [`skill_01_static_shapes.vcr`](skill_01_static_shapes.vcr) | Multiple shapes, z-ordering, color |
| 2 | [`skill_02_animated_circle.vcr`](skill_02_animated_circle.vcr) | Expressions, motion, easing |
| 3 | [`skill_03_lower_third.vcr`](skill_03_lower_third.vcr) | Text layers, groups, broadcast layout |
| 4 | [`skill_04_composition.vcr`](skill_04_composition.vcr) | Composing multiple animated elements |
| 5 | [`skill_05_custom_shader.vcr`](skill_05_custom_shader.vcr) | Inline WGSL shaders (GPU only) |

## Running any example

```bash
# Validate
vcr check examples/hello_world.vcr

# Render a single frame
vcr render-frame examples/hello_world.vcr --frame 0

# Render to video
vcr build examples/hello_world.vcr -o renders/hello.mov

# Live reload while editing
vcr watch examples/hello_world.vcr -o renders/preview
```

## Other examples

| File | Description |
|------|-------------|
| `demo_scene.vcr` | Kitchen-sink demo of most layer types |
| `instrument_typography.vcr` | Steerable typography with params |
| `instrument_grid.vcr` | Parametric grid layout |
| `instrument_logo_reveal.vcr` | Animated logo reveal |
| `geist_showcase.vcr` | Geist Pixel font family demo |
| `retro_pyramid.vcr` | 3D-style raymarched pyramid |
| `glass_panel.vcr` | Frosted glass effect |
| `dreamcore_statue.vcr` | Custom shader + post-processing |

For the full manifest reference, see [`SKILL.md`](../SKILL.md).
