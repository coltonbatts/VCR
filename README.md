# VCR — Video Component Renderer

Motion graphics from your terminal. Write YAML, render video. No Adobe, no GUI, no clicking.

For people who think code is faster than the Adobe ecosystem.

## Why VCR?

**No subscription tax.** One-time Rust install. Done.

**Deterministic rendering.** Same manifest, same output. Version control your animations. Ship them like code.

**Terminal-native.** Render in scripts, CI/CD pipelines, or let AI agents generate your animations.

**Built for Silicon Macs** (and anywhere else with Rust). Your M1/M2 can actually do something interesting.

**GPU + automatic CPU fallback.** Fast on real hardware. Still works everywhere.

## What You Get

- **YAML scene manifests** (`.vcr` files) that describe animations as data
- **Layered compositing** with z-order control
- **Procedural sources**: solid colors, linear gradients, or bring your own assets
- **Per-layer animation**: position, scale, rotation, opacity (keyframed or expression-driven)
- **ProRes 4444 output** via FFmpeg
- **Headless rendering** without any graphics server

## Quick Start

### Requirements

- **Rust** (stable) — [install here](https://rustup.rs/)
- **FFmpeg** on your PATH — `brew install ffmpeg` (macOS) or equivalent for your OS

### Install & Build

```bash
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo build --release
```

### Your First Render

Create a file called `hello.vcr`:

```yaml
environment:
  resolution:
    width: 1920
    height: 1080
  fps: 24
  duration:
    frames: 120

layers:
  - id: background
    z_index: 0
    procedural:
      kind: solid_color
      color: { r: 0.05, g: 0.05, b: 0.05, a: 1.0 }

  - id: animated_gradient
    z_index: 1
    pos_x: "t * 20"
    procedural:
      kind: gradient
      start_color: { r: 0.2, g: 0.6, b: 1.0, a: 0.9 }
      end_color: { r: 1.0, g: 0.3, b: 0.2, a: 0.9 }
      direction: horizontal
```

Render it:

```bash
cargo run --release -- build hello.vcr -o hello.mov
```

That's it. You now have a `hello.mov` file with an animated gradient.

## Manifest Reference

### Top Level

```yaml
environment:
  resolution:
    width: 1920
    height: 1080
  fps: 24
  duration:
    frames: 240

layers:
  - # ... layer definitions
```

### Layer Structure

Each layer has:

- **`id`**: unique identifier (string)
- **`z_index`**: stacking order (lower renders first)
- **Animation properties** (optional):
  - `pos_x` / `pos_y` / `position`: position (can be expressions like `"t * 10"`)
  - `scale`: scale factor
  - `rotation_degrees`: rotation in degrees
  - `opacity`: 0.0 to 1.0
- **Source** (either `procedural` or `asset`):
  - `procedural`: generates content procedurally
  - `asset`: loads from file

### Procedural Sources

**Solid Color:**

```yaml
procedural:
  kind: solid_color
  color: { r: 0.1, g: 0.5, b: 0.9, a: 1.0 }
```

**Linear Gradient:**

```yaml
procedural:
  kind: gradient
  start_color: { r: 0.15, g: 0.45, b: 0.95, a: 0.85 }
  end_color: { r: 0.95, g: 0.35, b: 0.15, a: 0.85 }
  direction: horizontal  # or vertical
```

### Animation Expressions

Time-based variables in animation fields:

- **`t`**: current frame (0 at start)
- **`dt`**: delta time (frame duration in seconds)
- **`f`**: total number of frames

Example: `pos_x: "t * 20"` moves the layer 20 units per frame.

## CLI

```bash
cargo run -- --help
```

### Commands

**Check a manifest:**

```bash
cargo run -- check myanimation.vcr
```

Validates the file and prints a summary.

**Render to video:**

```bash
cargo run -- build myanimation.vcr -o output.mov
```

Outputs a ProRes 4444 file.

## Examples

The repo includes test manifests:

- **`sanity_check.vcr`** — basic example with GPU rendering
- **`sanity_check_fallback.vcr`** — triggers CPU fallback when no GPU is present

Run them:

```bash
cargo run --release -- build sanity_check.vcr -o test.mov
```

## Performance Notes

**GPU rendering (wgpu):**

- Fast on M1/M2 Macs and dedicated GPUs
- Automatically selected when available

**CPU fallback (tiny-skia):**

- Slower but works everywhere
- Triggered automatically with a warning when no GPU is detected
- Still significantly faster than many alternatives for simple procedural content

## Troubleshooting

### "FFmpeg not found"

Make sure FFmpeg is on your `PATH`:

```bash
brew install ffmpeg  # macOS
# or
apt-get install ffmpeg  # Linux
# or
choco install ffmpeg  # Windows
```

Verify:

```bash
ffmpeg -version
```

### "No GPU found. Falling back to CPU rendering."

This is normal. VCR will render, just slower. If you're on a system with GPU support, check:

- macOS: Should work on M1+ and dedicated GPUs
- Linux: Ensure your graphics drivers are installed
- Windows: Update your GPU drivers

### Build fails with Rust errors

Make sure you have a recent stable Rust:

```bash
rustup update
```

Then try again:

```bash
cargo build --release
```

## Design Philosophy

VCR exists because:

1. **Motion graphics shouldn't require a subscription.**
2. **Animations are data.** They should be version controlled, scriptable, and deterministic.
3. **Your hardware is powerful.** Use it.
4. **Headless is better.** No GUI overhead, no bloat, no clicks.

It's not trying to be After Effects. It's trying to be the tool you reach for when you want motion graphics that fit into a pipeline, live in code, and render fast.

## Contributing

This is open source. If you have ideas, issues, or PRs—go for it.

## License

[MIT License](LICENSE)

---

**Built with Rust, wgpu, tiny-skia, and frustration with the Adobe ecosystem.**
