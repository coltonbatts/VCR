# VCR (Video Component Renderer)

A headless motion graphics renderer that compiles declarative YAML scenes into broadcast-quality ProRes 4444 video with alpha transparency.
Built in Rust, GPU-accelerated on Apple Silicon, and designed to render in seconds.

![Sanity Check Demo](assets/sanity_check_demo.png)

*This animated demo was rendered from a `.vcr` manifest in under 2 seconds. The output is a ProRes 4444 `.mov` with full alpha transparency—ready to drop into any NLE.*

**Perfect for:** motion graphics overlays, animated lower thirds, branded graphics, procedural backgrounds, and repeatable graphics pipelines you would normally build in After Effects but want to control in code.

## Why VCR?

After Effects is powerful, but it is hard to automate, hard to version cleanly, and too slow for tight iteration loops. Remotion is great for web-style video generation, but post workflows often need NLE-ready ProRes 4444 with real alpha.

VCR is built for that gap: declarative manifests you can version, deterministic output (same input, same frames), and GPU-accelerated rendering that is fast enough for creative iteration. Write a manifest, render in seconds, and drop the `.mov` straight into Premiere, Resolve, or any NLE that supports alpha.

VCR is not a replacement for a full motion design GUI. It is best when you want reproducibility, speed, and automation.

## Feature Highlights

- **YAML scene manifests** (`.vcr`) that are human-readable and version-controllable.
- **Metal-backed GPU rendering** on Apple Silicon, with CPU fallback.
- **Deterministic rendering:** same input produces the same output.
- **Expression-driven animation:** `sin`, `cos`, `clamp`, `lerp`, `smoothstep`, `easeInOut`, `noise1d`, `env`, `saw`, `tri`, `glitch`, `random`, and more.
- **8 procedural primitives:** `solid_color`, `gradient`, `triangle`, `circle`, `rounded_rect`, `ring`, `line`, `polygon` — all available on both GPU and CPU backends.
- **Animatable procedural parameters:** colors, radii, thickness, and corner radius can be driven by expressions (e.g. `"sin(t) * 0.5"`).
- **Custom WGSL shader layers:** write your own fragment shaders inline or from file, with up to 8 expression-driven uniforms.
- **Native text rendering** via fontdue with Geist Pixel font family (Line, Square, Grid, Circle, Triangle variants).
- **Layered compositing** with z-order, transforms, rotation, groups, modulators, and alpha blending.
- **ProRes 4444 output** with transparent alpha channel.
- **CLI-first workflow:** no GUI dependency, no Electron overhead.

## Quick Example

`examples/welcome_terminal_scene.vcr` renders a terminal-style lower third overlay.

```yaml
version: 1
environment:
  resolution: { width: 2560, height: 1440 }
  fps: 24
  duration: { frames: 120 }

params:
  box_flash_speed: 1.0
  text_fade_delay: 20
  fade_out_start: 100

layers:
  - id: terminal_box
    z_index: 1
    pos_x: 180
    pos_y: 360
    opacity: "lerp((sin(t * box_flash_speed / 2) * 0.5 + 0.5), 1.0, smoothstep(10, 11, t)) * (1.0 - smoothstep(fade_out_start, fade_out_start + 20, t))"
    image:
      path: "../assets/terminal_box.png"

  - id: welcome_text
    z_index: 2
    pos_x: 320
    pos_y: 648
    scale: [0.8, 0.8]
    opacity: "smoothstep(text_fade_delay, text_fade_delay + 20, t) * (1.0 - smoothstep(fade_out_start, fade_out_start + 20, t))"
    image:
      path: "../assets/welcome_text_geist_pixel.png"
```

## Procedural Primitives

All primitives support animatable colors and parameters via expressions:

```yaml
# Pulsing circle with expression-driven radius and color
- id: pulse
  procedural:
    kind: circle
    center: { x: 0.5, y: 0.5 }
    radius: "0.1 + sin(t * 0.2) * 0.05"
    color:
      r: "0.5 + sin(t * 0.1) * 0.5"
      g: 0.2
      b: 0.8
      a: 1.0

# Regular hexagon
- id: hex
  procedural:
    kind: polygon
    center: { x: 0.5, y: 0.5 }
    radius: 0.15
    sides: 6
    color: { r: 0.4, g: 0.9, b: 0.4, a: 1.0 }

# Rounded rectangle
- id: card
  procedural:
    kind: rounded_rect
    center: { x: 0.5, y: 0.5 }
    size: { x: 0.4, y: 0.25 }
    corner_radius: 0.02
    color: { r: 0.2, g: 0.6, b: 0.9, a: 1.0 }
```

## Custom Shaders

Write WGSL fragment shaders directly in your manifest:

```yaml
- id: plasma
  shader:
    fragment: |
      fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
        let t = u.time;
        let speed = u.custom[0];
        let v = sin(uv.x * 10.0 + t * speed) + sin(uv.y * 10.0 + t * speed * 0.7);
        let r = sin(v * 3.14159) * 0.5 + 0.5;
        let g = sin(v * 3.14159 + 2.094) * 0.5 + 0.5;
        let b = sin(v * 3.14159 + 4.189) * 0.5 + 0.5;
        return vec4<f32>(r, g, b, 1.0);
      }
    uniforms:
      speed: 2.0
```

Built-in uniforms: `time` (seconds), `frame` (index), `resolution` (vec2). Custom uniforms accessible as `u.custom[0..7]`.

## Getting Started

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo build --release

# Validate a manifest
cargo run --release --bin vcr -- lint examples/primitives_test.vcr

# Preview frames
cargo run --release --bin vcr -- preview examples/primitives_test.vcr --image-sequence -o ./preview_frames

# Render to ProRes 4444
cargo run --release --bin vcr -- build examples/welcome_terminal_scene.vcr -o output.mov
```

## Requirements

- Rust (stable)
- FFmpeg (required for `.mov`/ProRes encoding)
- macOS recommended for Apple Silicon Metal acceleration
- Linux/Windows: CPU fallback available, not officially tested

## CLI Reference

| Command | Description |
|---------|-------------|
| `check <manifest>` | Validate and summarize a manifest |
| `lint <manifest>` | Report common scene issues |
| `dump <manifest> --frame N` | Print resolved state at frame N |
| `preview <manifest>` | Render preview frames (`--image-sequence` skips FFmpeg) |
| `render-frame <manifest> --frame N -o frame.png` | Render a single frame |
| `render-frames <manifest> --start-frame N --frames X -o dir` | Render a frame range |
| `build <manifest> -o output.mov` | Full ProRes 4444 render |
| `watch <manifest>` | Hot-reload preview on manifest changes |

Run `cargo run --release --bin vcr -- --help` for full options.

## Figma to VCR Workflow (MVP)

Extract product card designs from Figma and render them as motion graphics:

```bash
cargo run --release --bin figma-vcr-workflow -- \
  --figma-file "https://www.figma.com/file/<FILE_KEY>/Product-Cards" \
  --description "product card: pink skirt, $29.99" \
  --output-folder "./exports"
```

Requires `FIGMA_TOKEN` environment variable. See `--help` for full options.

## Performance

Measured on M1 Max (`2560x1440`, 5 seconds, demo scene):

- **GPU (Metal):** ~1.4s pipeline, ~2.5s wall time
- **CPU fallback:** ~12.6s pipeline, ~13.1s wall time
- Deterministic: same input, same render

## Roadmap

- Audio integration (FFT data as expression variables)
- Shader hot-reload during `watch`
- Video layer support for compositing footage
- Blend modes and additional effects

## License

MIT

## About

Built by Colton Batts ([@coltonbatts](https://github.com/coltonbatts)), a creative technologist in Fort Worth, TX.
VCR is part of a larger effort to help creatives adopt technical tools without losing their artistic voice.

- [coltonbatts.com](https://coltonbatts.com)
- [First Principles](https://youtube.com/@firstprinciples)
