# VCR — Video Component Renderer

Motion graphics from your terminal. Write YAML, render video. No Adobe, no GUI, no clicking.

![VCR Pure Core Hero](assets/vcr_ascension_pure_loop.gif)

**[NEW: Follow the Golden Path Onboarding Guide →](docs/user_onboarding.md)**

---

## Features

- **Terminal-Driven**: No GUI, no clicking. Just YAML and shaders.
- **Deterministic**: Every render is identical, every time.
- **Alpha-First**: Native ProRes 4444 support for professional compositing.
- **[Element Library →](docs/library/neural_cores.md)**: Pro-grade shaders and technical guides.

For people who think code is faster than the Adobe ecosystem.

## Why VCR?

**No subscription tax.** One-time Rust install. Done.

**Deterministic rendering.** Same manifest, same output. Version control your animations. Ship them like code.

- **Terminal-native.** Render in scripts, CI/CD pipelines, or let AI agents generate your animations.

## VCR for Agents

VCR is designed to be **Agent-First**. It provides:

- **JSON Error Contract**: Set `VCR_AGENT_MODE=1` to get machine-readable error payloads with suggested fixes.
- **Deterministic Pipeline**: Agents can reason about frames and pixels without worrying about platform-specific variations.
- **Structured Manifests**: Declarative YAML makes it easy for LLMs to author and modify complex scenes.

**Built for Silicon Macs** (and anywhere else with Rust). Your M1/M2 can actually do something interesting.

**GPU + automatic CPU fallback.** Fast on real hardware. Still works everywhere.

## What You Get

- **YAML scene manifests** (`.vcr` files) that describe animations as data
- **Library-first assets** via pinned IDs (`library:<id>`) for reproducible renders
- **Layered compositing** with z-order control
- **Procedural sources**: solid colors, linear gradients, or bring your own assets
- **Per-layer animation**: position, scale, rotation, opacity (keyframed or expression-driven)
- **ProRes 4444 output** via FFmpeg
- **Headless rendering** without any graphics server

## Quick Install (One-liner)

If you have **Rust** and **FFmpeg** installed, you can install VCR with a single command:

```bash
curl -fsSL https://raw.githubusercontent.com/coltonbatts/VCR/main/scripts/install.sh | bash
```

*Works on macOS, Linux, and WSL.*

## Quick Start

### Requirements

- **Rust** (stable) — [install here](https://rustup.rs/)
- **FFmpeg** on your PATH — `brew install ffmpeg` (macOS) or equivalent for your OS

### Install & Build

```bash
git clone https://github.com/coltonbatts/VCR.git
cd VCR
```

#### Feature Flags

VCR is built with a modular architecture to keep the core renderer lightweight.

| Feature | Description | Dependency Cost |
| --- | --- | --- |
| **Core** (default) | Headless renderer, YAML compiler, ProRes export. | Minimal |
| `play` | Adds `vcr play` live preview window with hot-reload. | `winit`, `egui`, `notify` |
| `workflow` | Adds Figma and Frame.io integration tools. | `reqwest`, `tokio` |

#### Build Commands

| Target | Command |
| --- | --- |
| **Core Only** | `cargo build --release` |
| **With Preview** | `cargo build --release --features play` |
| **With Workflow** | `cargo build --release --features workflow` |
| **Full Suite** | `cargo build --release --features "play workflow"` |

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
vcr render hello.vcr -o hello.mov
```

### Copy-Paste High-End Demo

Want to see something more complex? Copy this to `demo.vcr`:

```yaml
version: 1
environment:
  resolution: { width: 1920, height: 1080 }
  fps: 24
  duration: { frames: 48 }
layers:
  - id: bg
    procedural:
      kind: gradient
      start_color: { r: 0.1, g: 0.1, b: 0.2, a: 1.0 }
      end_color: { r: 0.3, g: 0.0, b: 0.1, a: 1.0 }
      direction: vertical
  - id: circle
    pos_x: "1920/2 + sin(t * 0.2) * 200"
    pos_y: "1080/2"
    procedural:
      kind: solid_color
      color: { r: 0.9, g: 0.8, b: 0.2, a: 0.8 }
```

Render it with: `vcr render demo.vcr -o demo.mov`

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
cargo run -- render myanimation.vcr -o output.mov
```

Outputs a ProRes 4444 file.

## Library-First Trailer Workflow

The trailer workflow is library-first: manifests in `manifests/trailer/` reject raw asset paths by default and require `library:<id>` references.

Add assets:

```bash
cargo run -- library add ./path/to/clip.mov --id hero-clip --type video --normalize trailer
cargo run -- library add ./path/to/logo.png --id hero-logo --type image
```

Verify pinned hashes/specs:

```bash
cargo run -- library verify
```

Use in manifests:

```yaml
layers:
  - id: logo
    source: "library:hero-logo"
```

Render direct to ProRes 4444 MOV:

```bash
cargo run -- render manifests/trailer/title_card_vcr.vcr -o renders/trailer/title_card.mov --backend software --determinism-report
```

Create lightweight sampled PNG previews (no huge frame dump):

```bash
cargo run -- preview manifests/trailer/title_card_vcr.vcr --frames 12 -o renders/preview/title_card/
```

## Assets & Packs

VCR also supports pack-scoped assets and a unified catalog view:

```bash
# Friendly add (auto type + id suggestion)
cargo run -- add ./assets/logo.png

# Add directly into a pack
cargo run -- add ./assets/lower_third.png --pack social-kit --id lower-third

# Discover all assets (library + packs)
cargo run -- assets
cargo run -- assets search lower
cargo run -- assets info pack:social-kit/lower-third
```

Use in manifests:

```yaml
layers:
  - id: lower-third
    source: "pack:social-kit/lower-third"
```

See:

- `docs/ASSETS.md` for quickstart
- `docs/PACKS.md` for pack format and sharing

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
