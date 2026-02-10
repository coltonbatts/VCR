# VCR (Video Component Renderer)

VCR is a headless, local-first motion graphics renderer.
It compiles declarative scene manifests into video files using a GPU pipeline (`wgpu`) with an automatic CPU software fallback (`tiny-skia`) when no GPU adapter is available.

## Features

- YAML-based scene manifests (`.vcr` files)
- Layered compositing with z-order
- Asset and procedural layers
- Procedural solid and linear gradient sources
- Per-layer animation controls:
  - `position` (static vec2 or keyframed mapping)
  - `pos_x`/`position_x` and `pos_y`/`position_y` expression overrides
  - `scale`, `rotation_degrees`, `opacity`
- ProRes 4444 output through FFmpeg
- Automatic CPU fallback with warning when no GPU is present

## Requirements

- Rust (stable)
- FFmpeg available on `PATH`

## Build

```bash
cargo build
```

## CLI

```bash
cargo run -- --help
```

Commands:

- `check <manifest>`: validate and print manifest summary
- `build <manifest> -o <output.mov>`: render video

Examples:

```bash
cargo run -- check sanity_check.vcr
cargo run -- build sanity_check.vcr -o test.mov
```

If no GPU is available, VCR falls back automatically:

```text
[VCR] Warning: No GPU found. Falling back to CPU rendering (Slow).
```

## Manifest Shape (high-level)

```yaml
environment:
  resolution:
    width: 1920
    height: 1080
  fps: 24
  duration:
    frames: 48

layers:
  - id: background
    z_index: 0
    procedural:
      kind: solid_color
      color: { r: 0.1, g: 0.1, b: 0.1, a: 1.0 }

  - id: moving_gradient
    z_index: 1
    pos_x: "t * 40"
    procedural:
      kind: gradient
      start_color: { r: 0.15, g: 0.45, b: 0.95, a: 0.85 }
      end_color: { r: 0.95, g: 0.35, b: 0.15, a: 0.85 }
      direction: horizontal
```

## Included Example Manifests

- `sanity_check.vcr`
- `sanity_check_fallback.vcr`

## Notes

- The software renderer returns tight RGBA8 rows directly.
- The GPU renderer handles padded row copies and strips padding before encoding.
- Output artifacts like local test renders are not source-controlled.
