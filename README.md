# VCR (Video Component Renderer)

![Welcome to VCR](assets/welcome%20to%20VCR.jpg)

A headless motion graphics renderer that compiles declarative YAML scenes into broadcast-quality ProRes 4444 video with alpha transparency.
Built in Rust, GPU-accelerated on Apple Silicon, and designed to render in seconds.

**Perfect for:** motion graphics overlays, animated lower thirds, branded graphics, procedural backgrounds, and repeatable graphics pipelines you would normally build in After Effects but want to control in code.

## Why VCR?

After Effects is powerful, but it is hard to automate, hard to version cleanly, and too slow for tight iteration loops. Remotion is great for web-style video generation, but post workflows often need NLE-ready ProRes 4444 with real alpha.

VCR is built for that gap: declarative manifests you can version, deterministic output (same input, same frames), and GPU-accelerated rendering that is fast enough for creative iteration. Write a manifest, render in seconds, and drop the `.mov` straight into Premiere, Resolve, or any NLE that supports alpha.

VCR is not a replacement for a full motion design GUI. It is best when you want reproducibility, speed, and automation.

## Feature Highlights

- **VCR HUB (TUI)**: A brutalist, editorial terminal interface for managing your motion assets and hardware.
- **Agentic Prompting**: Generate full video manifests from natural language using local LLMs (LM Studio / Ollama).
- **Intelligence Tree**: A SQLite-backed context layer that "remembers" your creative style and project nodes.
- **YAML scene manifests** (`.vcr`) that are human-readable and version-controllable.
- **Metal-backed GPU rendering** on Apple Silicon, with CPU fallback.
- **Deterministic rendering**: same input produces the same output.
- **Expression-driven animation**: `sin`, `cos`, `clamp`, `lerp`, `smoothstep`, `easeInOut`, `noise1d`, `env`, `saw`, `tri`, `glitch`, `random`, and more.
- **8 procedural primitives**: `solid_color`, `gradient`, `triangle`, `circle`, `rounded_rect`, `ring`, `line`, `polygon`.
- **Native text rendering** via fontdue with Geist Pixel font family.
- **ProRes 4444 output** with transparent alpha channel.
- **Offline-first & Privacy-focused**: Everything runs locally. No clouds, no subscriptions.

## Quick Example

![Sanity Check Demo](assets/sanity_check_demo.gif)

*This demo was rendered from a `.vcr` manifest in under 2 seconds. The output is a ProRes 4444 `.mov` with full alpha transparency—ready to drop into any NLE.*

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

## Steerable Motion

VCR now supports first-class control surfaces via typed manifest params and runtime overrides.
For strict semantics (override precedence, substitution rules, escaping, and determinism guarantees), see `docs/PARAMS.md`.

### 1) Declare typed params

```yaml
params:
  speed:
    type: float
    default: 1.0
    min: 0.4
    max: 3.0
    description: "Global motion speed"
  accent_color:
    type: color
    default: { r: 0.95, g: 0.48, b: 0.22, a: 0.92 }
  drift:
    type: vec2
    default: [180.0, 40.0]
  invert_background:
    type: bool
    default: false
```

Legacy numeric params still work unchanged:

```yaml
params:
  energy: 0.85
  phase: 0.4
```

### 2) Reference params anywhere with `${param_name}`

```yaml
start_time: "${intro_start}"
time_scale: "${speed}"
position: "${drift}"
opacity: "clamp(0.30 + glow_strength * 0.45, 0.0, 1.0)"
start_color: "${accent_color}"
```

Expression variables continue to use bare names (`speed`, `glow_strength`, etc.).

### 3) Override params at runtime

```bash
cargo run --release --bin vcr -- build examples/steerable_motion.vcr \
  --set speed=2.2 \
  --set glow_strength=1.1 \
  --set accent_color=#4FE1B8 \
  -o renders/steerable_fast.mov
```

### 4) Inspect params and resolved values

```bash
# Print param catalog (type/default/range/description)
cargo run --release --bin vcr -- params examples/steerable_motion.vcr
# JSON catalog for scripts/CI
cargo run --release --bin vcr -- params examples/steerable_motion.vcr --json

# Print resolved render inputs (after --set overrides)
cargo run --release --bin vcr -- explain examples/steerable_motion.vcr --set speed=2.2
# JSON explain output
cargo run --release --bin vcr -- explain examples/steerable_motion.vcr --set speed=2.2 --json
```

Use `--quiet` on render/check/watch commands to suppress non-essential param dumps while keeping errors visible.

### 5) Reproducible metadata sidecars

Render commands emit a local `*.metadata.json` sidecar containing:

- resolved params
- frame count and frame window
- resolution and fps
- backend + reason
- VCR version
- manifest hash

## Playground Preset Runner

To regenerate all steerable playground previews in one pass:

```bash
./scripts/run_playground.sh
```

This runs all 9 presets across:

- `examples/instrument_typography.vcr`
- `examples/instrument_grid.vcr`
- `examples/instrument_logo_reveal.vcr`

Outputs are written to `renders/playground/<scene>/<preset>/` with:

- frame sequences (`frame_*.png`)
- per-preset metadata (`preview.metadata.json`)
- `renders/playground/index.json` (scene/preset/output/params/metadata index)
- optional per-scene contact sheets (`contact_sheet.png`) when `ffmpeg` is installed

For preset editing and output details, see `docs/PLAYGROUND.md`.

## Getting Started

```bash
# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo build --release

# Validate a manifest
cargo run --release --bin vcr -- lint examples/demo_scene.vcr

# Preview frames
cargo run --release --bin vcr -- preview examples/demo_scene.vcr --image-sequence -o ./preview_frames

# Interactive live preview
cargo run --release --bin vcr -- play examples/demo_scene.vcr
# Start paused at a specific frame
cargo run --release --bin vcr -- play examples/demo_scene.vcr --start-frame 48 --paused

# Render to ProRes 4444
cargo run --release --bin vcr -- build examples/welcome_terminal_scene.vcr -o output.mov
```

## VCR HUB (Terminal OS)

Launch the VCR Hub to manage your hardware, context, and generation pipeline in a high-tension, brutalist terminal interface.

```bash
# Launch the Hub
go run ./cmd/vcr-tui/main.go
```

The Hub performs a "Cold Start" sequence:

- **Hardware Scan**: Detects GPU acceleration (WGPU) and local LLM endpoints.
- **Brain Handshake**: Proactively verifies connectivity and model readiness.
- **Intelligence Tree**: Syncs your creative context from a local SQLite database.

## Agentic Prompting

Generate broadcast-quality motion graphics using natural language. VCR connects to **LM Studio** or **Ollama** to translate your intent into valid VCR YAML.

1. **Speak**: *"Make me a Title safe lower third with a dark gradient."*
2. **Context**: The Hub pulls your previous style nodes from the **Intelligence Tree**.
3. **Draft**: The local model generates a strict VCR manifest.
4. **Render**: The Rust engine compiles the ProRes 4444 artifact in real-time.

---

## Requirements

- **Rust (stable)**
- **Go 1.21+** (for the TUI Hub)
- **FFmpeg** (required for ProRes encoding)
- **macOS** (Metal acceleration) or **Linux/Windows** (CPU fallback)
- **LM Studio** or **Ollama** (for Agentic features)

## The 5-Minute Golden Path

Achieve a high-quality render in three steps:

1. **Diagnose:** Ensure your system is ready.

    ```bash
    cargo run --release --bin vcr -- doctor
    ```

2. **Lint:** Verify your scene manifest.

    ```bash
    cargo run --release --bin vcr -- lint examples/white_on_alpha.vcr
    ```

3. **Preview:** View a fast image sequence (no FFmpeg involved).

    ```bash
    cargo run --release --bin vcr -- preview examples/white_on_alpha.vcr --image-sequence
    ```

4. **Build:** Render the final ProRes 4444 `.mov`.

    ```bash
    cargo run --release --bin vcr -- build examples/white_on_alpha.vcr -o final.mov
    ```

## CLI Reference

| Command | Description |
| --- | --- |
| `check <manifest>` | Validate and summarize a manifest |
| `lint <manifest>` | Report common scene issues |
| `dump <manifest> --frame N` | Print resolved state at frame N |
| `params <manifest>` | Print declared params (type/default/range/description), or `--json` |
| `explain <manifest>` | Print resolved params + manifest hash (supports `--set`, `--json`) |
| `preview <manifest>` | Render preview frames (`--image-sequence` skips FFmpeg) |
| `play <manifest>` | Interactive real-time preview window with hot-reload |
| `render-frame <manifest> --frame N -o frame.png` | Render a single frame |
| `render-frames <manifest> --start-frame N --frames X -o dir` | Render a frame range |
| `build <manifest> -o output.mov` | Full ProRes 4444 render |
| `watch <manifest>` | Hot-reload preview on manifest changes |
| `doctor` | Check FFmpeg, fonts, and backend status |

Run `cargo run --release --bin vcr -- --help` for full options.
Exit code contract: `docs/EXIT_CODES.md`.

Most render and inspect commands accept repeatable runtime overrides:

```bash
--set name=value --set other_name=value
```

## Interactive Preview (`play`)

Use `play` for live scene iteration in a window at your manifest resolution:

```bash
cargo run --release --bin vcr -- play examples/primitives_test.vcr
```

Optional flags:

- `--paused` start paused
- `--start-frame N` start at a specific frame

Controls:

- `Space`: Play/Pause
- `Left` / `Right`: Seek one frame
- `R`: Restart
- `Esc`: Quit
- Drag seek bar: Scrub timeline

Hot-reload behavior:

- Save the `.vcr` file and the scene reloads in-place.
- If reload parsing fails, the error is printed and the previous valid scene keeps rendering.

## Figma to VCR Workflow (MVP)

Extract product card designs from Figma and render them as motion graphics:

```bash
cargo run --release --bin figma-vcr-workflow -- \
  --figma-file "https://www.figma.com/file/<FILE_KEY>/Product-Cards" \
  --description "product card: pink skirt, $29.99" \
  --output-folder "./exports"
```

Requires `FIGMA_TOKEN` environment variable. See `--help` for full options.

## Determinism & Testing

VCR is designed to be deterministic: the same manifest always produces the same frames. To ensure this remains true as the codebase evolves, we run regression tests that compare the FNV-1a hash of rendered frames against "golden" values.

Tests are performed using the **Software Renderer** to ensure cross-platform stability in CI environments.

```bash
# Run all tests (including determinism)
cargo test

# Run only determinism tests
cargo test --test determinism
```

### Updating Golden Hashes

If you intentionally change rendering logic (e.g., fixing a rounding bug or adjusting a primitive's default behavior), the regression tests will fail. To update the hashes:

1. Run the tests and capture the "Actual" hash from the failure message.
2. Update the corresponding `expected_hash` in `tests/determinism.rs`.
3. Commit the change with a description of why the output changed.

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
