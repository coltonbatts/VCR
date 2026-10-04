# VCR — Video Component Renderer

Motion graphics from your terminal. Write YAML, render video. No Adobe, no GUI, no clicking.

![VCR Pure Core Hero](assets/vcr_ascension_pure_loop.gif)

**[NEW: Follow the Golden Path Onboarding Guide →](docs/user_onboarding.md)**

---

## Features

- **Terminal-Driven**: No GUI, no clicking. Just YAML and shaders.
- **Deterministic**: Every render is identical, every time.
- **Alpha-First**: Native ProRes 4444 support for professional compositing.
- **Element Library**: A curated collection of drag-and-drop ProRes movies and their source manifests.

For people who think code is faster than the Adobe ecosystem.

## Why VCR?

**No subscription tax.** One-time Rust install. Done.

**Deterministic rendering.** Same manifest, same output. Version control your animations. Ship them like code.

- **Terminal-native.** Render in scripts, CI/CD pipelines, or let AI agents generate your animations.

## VCR for Agents

VCR is designed to be **Agent-First**. It provides:

- **One machine contract (`vcr.agent/1`)**: `--json` on `capabilities`, `prompt`, `check`, `lint`, `explain`, `inspect`, `render`, `verify`, `doctor` prints exactly one JSON document (status, diagnostics, artifacts, typed errors with locations and recovery). See [docs/AGENT_CONTRACT.md](docs/AGENT_CONTRACT.md).
- **Discoverable**: `vcr capabilities --json --schema` reports what *this installation* can do (layers, fonts, codecs, time units, manifest JSON Schema generated from the engine's types, ffmpeg/GPU availability).
- **Inspectable**: `vcr inspect` samples the timeline (entrance, hold, exit, ending), writes a labelled contact sheet over an alpha checkerboard, and reports exact layer bounds plus clipping/timing diagnostics.
- **Verifiable**: `vcr verify` compares the encoded `.mov` with the request (resolution, exact frame rate, frame count, codec/profile, alpha, *decoded* transparency) and detects stale, modified or truncated output. Renders publish atomically and write `*.provenance.json`.
- **Legacy JSON Error Contract**: `VCR_AGENT_MODE=1` still prints machine-readable errors on stderr (now in the contract envelope plus the old keys).
- **Preflight and quickstart**: `vcr explain --json` (alias `vcr preflight`) tells you before rendering whether *this machine* can render the scene with the chosen backend: the resolved backend, unsupported layers and ignored `post` effects, ffmpeg/GPU availability, font fallback, blockers, and a single `ready` flag. The full eight-step flow is in the [Agent Quickstart](docs/AGENT_QUICKSTART.md).
- **Scoped determinism**: scene settings are always reproducible; software-backend raster frames are expected identical for the same engine build; encoded file bytes also depend on the ffmpeg build; GPU output is not bit-identical across hardware. Provenance records the conditions.
- **Structured Manifests**: Declarative YAML makes it easy for LLMs to author and modify complex scenes.

### The agent workflow

discover → normalize → author → validate/preflight → inspect → revise → render → verify. Every step prints one JSON line on stdout with `--json`; read `status` (`ok | blocked | failed | error`), `diagnostics` and `error.code`, never stderr.

```bash
vcr capabilities --json                      # what can this installation do? (add --schema for the manifest schema)
vcr prompt --json --text "5s alpha lower third 1920x1080 60fps"   # normalize; status "blocked" = ask the requester
vcr check scene.vcr --json                   # then: vcr lint --json, vcr explain --json (backend preflight, `ready`)
vcr inspect scene.vcr --json -o renders/inspect   # sampled frames + contact sheet + clipping/timing diagnostics
vcr render scene.vcr -o renders/scene.mov --json  # atomic publish + provenance
vcr verify renders/scene.mov --manifest scene.vcr --expect-transparency required --json
```

Delivered means `verify` passed, not "render exited 0". A scene can be schema-valid and still clipped or mistimed, which is what `inspect` is for; `check` passing does not mean the chosen backend can render it, which is what preflight is for.

**MCP.** `scripts/vcr-mcp-server/` exposes the same operations as typed tools (`vcr_capabilities`, `vcr_normalize_brief`, `vcr_validate`, `vcr_inspect`, `vcr_render`, `vcr_verify`, …). It forwards the CLI's documents unchanged, adds the selected engine's identity, and never invents scene settings. See its [README](scripts/vcr-mcp-server/README.md).

**Status, stated plainly.** The contract, discovery, inspection and verification are covered by deterministic tests, and a 1080p60 alpha lower third (plus a 12-variant matrix) has been rendered and verified on decoded pixels. A 10-task [agent benchmark](docs/agent-benchmark/README.md) is defined, but **no autonomous-agent runs have been performed yet** and there is no comparison against other tools, so VCR makes no "best available" claim. GPU-only paths (shader/video/lottie/post) are untested on CI hardware.

## Visual-First Development

VCR follows a **Visual-First** methodology. Because motion graphics are often too complex to verify via unit tests alone, we use automated visual verification loops:

- **Snapshot Previews**: Immediate keyframe extraction to verify framing and initial state.
- **Contact Sheets**: 3x3 mosaics that capture the start, middle, and end of animations to ensure stability throughout the entire duration.
- **SOP Compliance**: All elements delivered to the VCR library must pass the [Standard Operating Procedure](VCR_SOP.md).

**Never ship blind.** If you can't see the middle of your animation, you haven't finished the render.

**Built for Silicon Macs** (and anywhere else with Rust). Your M1/M2 can actually do something interesting.

**GPU + automatic CPU fallback for supported layer types.** Fast on real hardware. Unsupported `shader`, `wgpu_shader`, `video`, and `lottie` layers fail explicitly instead of rendering transparent output.

## What You Get

- **YAML scene manifests** (`.vcr` files) that describe animations as data
- **VCR Element Library** (`library/elements/`): Reusable, production-ready video clips with source manifests.
- **Layered compositing** with z-order control
- **Procedural sources**: solid colors, linear gradients, or bring your own assets
- **Per-layer animation**: position, scale, rotation, opacity (keyframed or expression-driven)
- **ProRes 4444 output** via FFmpeg
- **Headless rendering** without any graphics server

## Quick Start

The fastest way to get VCR and the Tape Deck interactive UI is via our one-liner install script. *(Requires [FFmpeg](https://ffmpeg.org/download.html) to be installed on your system).*

```bash
curl -fsSL https://raw.githubusercontent.com/coltonbatts/VCR/main/scripts/install.sh | bash
```

*(Make sure `~/.local/bin` is in your shell's PATH after installation!)*

### Open the Deck

VCR uses a **Tapes-First Workflow**. You treat your animation manifests as immutable recipes ("Tapes"), VCR as the render engine, and the "Tape Deck" as your interactive controller UI.

Instead of memorizing long rendering commands to manage your files, just open the Deck in your terminal:

```bash
vcr deck
```

From the Deck interface, you can effortlessly:

- **Initialize** your project's `tapes.yaml` environment automatically.
- **Create** new tapes from templates.
- **Preview & Play** your animations instantly.

### Manual Build / Cargo Install

If you're looking to modify VCR or prefer building from source, make sure you have Rust stable installed.

```bash
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo xtask deck-install
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

### Basic File Usage (Headless Flow)

If you're using VCR headlessly (e.g. CI/CD or Agent usage without the Tape Deck), you can still point the renderer directly at `.vcr` composition files:

```bash
vcr render manifests/hello.vcr -o hello.mov
```

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

## Documentation

Comprehensive guides and technical references are available in the `docs/` directory:

- **[VCR Skill Reference](SKILL.md)**: The primary guide for authors and AI agents.
- **[Standard Operating Procedure](VCR_SOP.md)**: The mandatory "Golden Path" for render verification.
- **[Prompt Guide](docs/PROMPT_GUIDE.md)**: How to write bulletproof VCR prompts.
- **[Agent Protocol](AGENTS.md)**: How automated tools interact with the engine.
- **[Agent Quickstart](docs/AGENT_QUICKSTART.md)**: The short, authoritative agent workflow.
- **[Agent Contract](docs/AGENT_CONTRACT.md)**: `vcr.agent/1` envelope, error codes, exit codes, determinism scope, migration notes.
- **[Agent Benchmark](docs/agent-benchmark/README.md)**: Pre-registered 10-task suite, scoring, and what counts as evidence.
- **[Lower-third proof](docs/agent-first/proof/README.md)**: The first production proof and its limits.
- **[Project Custodian](docs/PROJECT_CUSTODIAN.md)**: Architectural overview and project map.
- **[PRD](docs/PRD.md)**: Product vision and roadmap.
- **[Changelog](CHANGELOG.md)**: Latest features and fixes.

Community guidelines can be found in the `.github/` directory:

- **[Contributing](.github/CONTRIBUTING.md)**
- **[Code of Conduct](.github/CODE_OF_CONDUCT.md)**
- **[Security Policy](.github/SECURITY.md)**

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
cargo run -- render manifests/hello.vcr -o output.mov
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

## VCR Element Library

The VCR Element Library (`library/elements/`) is the designated home for production-ready motion graphics elements. Each element includes its high-quality ProRes MOV file and the original `.vcr` manifest.

### Exporting to Library

Use the library export script to "promote" a manifest and its render to the library:

```bash
./scripts/lib_export.sh my_element.vcr renders/my_element.mov
```

This copies the files directly into `library/elements/`:

- `my_element.mov` (The drag-and-drop video file)
- `my_element.vcr` (The source manifest for reproducibility)
- `my_element_preview.png` (Optional preview image)

## Examples

The repo includes runnable manifests:

- **`manifests/hello.vcr`** — minimal starter scene
- **`examples/ai_company_hero.vcr`** — shader + text composition example
- **`examples/white_on_alpha.vcr`** — transparent background render sample

Run them:

```bash
cargo run --release -- render manifests/hello.vcr -o renders/hello.mov
cargo run --release -- render examples/white_on_alpha.vcr -o renders/white_on_alpha.mov
```

## Performance Notes

**GPU rendering (wgpu):**

- Fast on M1/M2 Macs and dedicated GPUs
- Automatically selected when available

**CPU fallback (tiny-skia):**

- Slower, but deterministic for supported layer types
- Triggered automatically with a warning when no GPU is detected
- Rejects `shader`, `wgpu_shader`, `video`, and `lottie` layers instead of silently dropping them
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

This is normal for manifests the software backend can render. VCR will render, just slower. If the manifest includes `shader`, `wgpu_shader`, `video`, or `lottie` layers, VCR now fails clearly instead of producing transparent output. If you're on a system with GPU support, check:

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
