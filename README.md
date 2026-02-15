# VCR (Video Component Renderer)

```text
██╗   ██╗ ██████╗██████╗ 
██║   ██║██╔════╝██╔══██╗
██║   ██║██║     ██████╔╝
╚██╗ ██╔╝██║     ██╔══██╗
 ╚████╔╝ ╚██████╗██║  ██║
  ╚═══╝   ╚═════╝╚═╝  ╚═╝
```

```text
vcr version 0.1.2
```

![Welcome to VCR](assets/welcome%20to%20VCR.jpg)

VCR is a local, deterministic motion graphics engine in Rust with an agent-first workflow on top.
It turns structured scene specs into repeatable renders, preview frames, and metadata that can be consumed by humans, scripts, or agents.

## What This Repo Is

- A rendering core (`vcr`) for YAML motion manifests
- A workflow binary (`figma-vcr-workflow`) that converts Figma + prompt context into VCR-ready outputs
- A practical playground for steerable presets (`scripts/run_playground.sh`)

This is not a SaaS product. It is a buildable, inspectable toolchain.

## AI-Driven Workflow

**VCR is designed for AI agents.** Instead of generating pixels directly, AI writes declarative YAML manifests that VCR compiles into deterministic, broadcast-quality video.

```bash
# AI reads the skill, writes a manifest, you render it
AI: "Create a lower third for 'Colton Batts' with fade-in"
   ↓ (generates YAML manifest)
You: cargo run --bin vcr -- build manifest.vcr -o output.mov
   ↓ (renders in <1 second)
Result: ProRes 4444 with alpha, pixel-perfect, reproducible
```

**Read [`SKILL.md`](SKILL.md)** - the complete AI agent reference for VCR's manifest format, layer types, expressions, and CLI commands.

This is **infrastructure for AI-generated motion graphics**: AI authors the spec, VCR guarantees deterministic execution. No hallucinations in the pixels.

## Machine-readable Output

Agent consumers should use the [Agent Error Contract (v0.1.x)](#agent-error-contract-v01x) for machine-readable failures emitted with `VCR_AGENT_MODE=1`.

## Why It Exists

Most motion design tools are hard to version, hard to automate, and hard to run deterministically.
VCR exists to make motion generation scriptable and reproducible.

## What Works Today

- Typed params with runtime overrides (`--set`)
- Deterministic output and deterministic metadata sidecars
- GPU backend on supported systems with software fallback
- Preview/build/watch flows via CLI
- High-fidelity QuickTime compatibility (`-vendor apl0`)
- Explicit ProRes profile control (`proxy` to `prores4444_xq`)
- Alpha-consistency linting for transparent renders
- Built-in WGSL Standard Library (`#include "vcr:<module>"`)
- Agent-oriented workflow protocol docs (`docs/SKILLS_PROTOCOL.md`)
- Figma-to-scene workflow binary (`figma-vcr-workflow`)

## Quickstart

### Requirements

- Rust (stable)
- FFmpeg
- macOS recommended for best GPU path (software fallback exists)

### Build

```bash
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo build --release --bin vcr --bin figma-vcr-workflow
```

### Sanity Run

```bash
./target/release/vcr doctor
./target/release/vcr lint examples/demo_scene.vcr
./target/release/vcr preview examples/demo_scene.vcr --image-sequence --frames 24 -o renders/quick_preview
```

### Render a `.mov`

```bash
./target/release/vcr build examples/welcome_terminal_scene.vcr -o renders/output.mov

# FFmpeg mode (Phase 2 scaffolding)
./target/release/vcr --ffmpeg system build examples/welcome_terminal_scene.vcr -o renders/output.mov
# sidecar mode requires feature flag:
# cargo run --features sidecar_ffmpeg --bin vcr -- --ffmpeg sidecar build ...
```

### First Boot: Natural Language -> Deterministic Spec

Use `vcr prompt` to translate natural language (or loose YAML) into a standardized engine-ready prompt plus a QA report of unknowns/fixes.

```bash
./target/release/vcr prompt \
  --text "Make a cinematic 5s title card at 60fps, transparent alpha, output ./renders/title.mov"
```

You can also read from a file and write the translated YAML:

```bash
./target/release/vcr prompt \
  --in ./specs/request.yaml \
  -o ./specs/request.normalized.yaml
```

## ASCII Stage (Transcript -> Terminal Cinema)

`vcr ascii stage` renders a plain-text `.vcrtxt` chat transcript into a stylized terminal video with deterministic timing.

```bash
./target/release/vcr ascii stage \
  --in examples/ascii/demo.vcrtxt \
  --out renders/ascii_demo_x.mp4 \
  --preset x \
  --camera slow-zoom \
  --seed 0
```

Use cases:

- Social-friendly vertical clips (`--preset x`)
- Tool-call heavy interaction replays (`examples/ascii/tool-heavy.vcrtxt`)
- Scripted dramatic pacing (`examples/ascii/drama.vcrtxt`)

See `docs/ASCII_STAGE.md` for format, flags, and presets.

## ASCII Capture (ASCII Animation -> ProRes)

`vcr ascii capture` captures animated ASCII from remote streams (`ascii-live:*`), offline built-in dev sources (`library:*`), or local media (via `chafa`), then encodes ProRes 422 MOV output.

```bash
./target/release/vcr ascii capture \
  --source library:geist-wave \
  --out renders/library_wave.mov \
  --duration 8 \
  --fps 30 \
  --size 120x45
```

List curated source ids:

```bash
./target/release/vcr ascii sources
```

See `docs/ASCII_CAPTURE.md` for source formats, flags, parser limitations, and determinism scope.

## VCR Standard Library (WGSL)

VCR includes a built-in library of WGSL modules for custom shaders. Use `#include "vcr:<module>"` to inject them.

| Module | Description |
| --- | --- |
| `vcr:common` | Math (`rotate3d`) and color (`hsv2rgb`) utilities |
| `vcr:noise` | 2D/3D Simplex, Voronoi, and fBm noise |
| `vcr:sdf` | 3D SDF primitives (Sphere, Box, Torus) and booleans |
| `vcr:raymarch` | Anti-aliased raymarching boilerplate with perfect alpha edges |

Example usage:

```wgsl
#include "vcr:common"
#include "vcr:sdf"

fn map(p: vec3<f32>, u: ShaderUniforms) -> f32 {
    return sdSphere(rotate3d(p, vec3<f32>(u.time*0.1)), 1.0);
}
```

## ASCII Animation Engine (Frame Packs -> Overlay Layer)

VCR now includes a modular ASCII animation engine for importing frame packs from `assets/animations/<name>/` and compositing them as foreground/background layers in the render pipeline.

- Engine docs: `docs/ANIMATION_ENGINE.md`
- Boilerplate example: `examples/ascii_animation_boilerplate.rs`
- **Rapier + Three.js Boilerplate**: `examples/rapier-three-boilerplate/` (Deterministic physics handshake)
- Curated Library: `assets/animations/library/` (discover via `vcr ascii library`)

### URL -> White Alpha Overlay (ascii.co.uk)

One command to import an `ascii.co.uk/animated-art/...` page and render white text on transparent alpha:

```bash
cargo run --bin ascii-link-overlay -- \
  --url "https://www.ascii.co.uk/animated-art/milk-water-droplet-animated-ascii-art.html" \
  --width 1920 --height 1080 --fps 24
```

Defaults tuned for drag-and-drop style imports:

- leading blank source frames are auto-trimmed (`--trim-leading-blank true`)
- glyph color is pure white; background is transparent alpha
- checker preview MP4 is generated for quick visibility check

Wrapper script (single or multiple URLs):

```bash
./scripts/ascii_link_overlay.sh \
  "https://www.ascii.co.uk/animated-art/milk-water-droplet-animated-ascii-art.html" \
  "https://www.ascii.co.uk/animated-art/3d-tunnel.html" \
  -- --width 1920 --height 1080 --fps 24
```

### Production Validation

Renders are validated for automated pipelines:

- **Frame 0**: Guaranteed non-blank (auto-trimmed leading empty frames).
- **Strict Channels**: Foreground is clamped to pure white (`#FFFFFF`); background is 0% alpha transparent.
- **Sidecar Metadata**: Each render includes a `.metadata.json` with source attribution and artist tags.

Outputs:

- `assets/animations/ascii_co_uk_<slug>/` frame pack + metadata
- `renders/ascii_co_uk_<slug>_white_alpha.mov` (ProRes 4444 alpha)
- `renders/ascii_co_uk_<slug>_white_alpha.png` (frame 0 still)
- `renders/ascii_co_uk_<slug>_white_alpha_checker.mp4` (visibility preview)

## One-Command Playground (9 Presets)

```bash
./scripts/run_playground.sh
```

This runs all presets for:

- `examples/instrument_typography.vcr`
- `examples/instrument_grid.vcr`
- `examples/instrument_logo_reveal.vcr`

Outputs:

- `renders/playground/<scene>/<preset>/frame_*.png`
- `renders/playground/<scene>/<preset>/preview.metadata.json`
- `renders/playground/index.json`
- optional `renders/playground/<scene>/contact_sheet.png` when FFmpeg is available

See `docs/PLAYGROUND.md` for details.

## Baseline Performance + Determinism Report

Capture a reproducible Phase 0 baseline (timing + frame hash) across a small manifest matrix:

```bash
./scripts/baseline_report.sh
```

Default output:

- `renders/baseline/baseline_report.json`
- per-case rendered PNGs + metadata sidecars in `renders/baseline/`

Optional custom output directory:

```bash
./scripts/baseline_report.sh renders/my_baseline
```

CI-safe mode (skip GPU-forced cases):

```bash
BASELINE_GPU=0 ./scripts/baseline_report.sh
```

Optional stress sample:

```bash
BASELINE_STRESS=1 ./scripts/baseline_report.sh
```

## Agent Workflow (Figma -> VCR)

```bash
./target/release/figma-vcr-workflow \
  --figma-file "https://www.figma.com/file/<FILE_KEY>/<FILE_NAME>" \
  --description "product card with price callout" \
  --output-folder "./exports"
```

Notes:

- Requires `FIGMA_TOKEN` in environment
- Generates VCR artifacts/workflow outputs in the selected folder

## Minimal Param Example

```yaml
version: 1
environment:
  resolution: { width: 1280, height: 720 }
  fps: 24
  duration: { frames: 96 }

params:
  speed:
    type: float
    default: 1.0
    min: 0.5
    max: 3.0
  accent_color:
    type: color
    default: { r: 0.95, g: 0.48, b: 0.22, a: 1.0 }

layers:
  - id: bg
    procedural:
      kind: gradient
      start_color: "${accent_color}"
      end_color: { r: 0.05, g: 0.06, b: 0.08, a: 1.0 }
      direction: vertical
```

Override at runtime:

```bash
./target/release/vcr preview examples/steerable_motion.vcr \
  --image-sequence \
  --frames 24 \
  --set speed=2.2 \
  --set accent_color=#4FE1B8 \
  -o renders/steerable_fast
```

## CLI Commands

`vcr` supports:

- `build`
- `check`
- `lint`
- `dump`
- `params`
- `explain`
- `preview`
- `play`
- `render-frame`
- `render-frames`
- `watch`
- `prompt`
- `chat render`
- `ascii stage`
- `ascii capture`
- `ascii sources`
- `doctor`
- `determinism-report` (frame hash for golden tests)

Use `--version` for version + git hash, `--backend software` for deterministic CPU rendering.

For full help:

```bash
./target/release/vcr --help
```

## Reliability and Testing

```bash
cargo test
cargo test --test determinism
cargo test --test cli_contract
```

- Exit codes: `docs/EXIT_CODES.md`
- Param semantics: `docs/PARAMS.md`
- Architecture: `docs/ARCHITECTURE.md`
- Determinism: `docs/DETERMINISM_SPEC.md`
- Reproducible build: `docs/REPRODUCIBLE_BUILD.md`

## Agent Error Contract (v0.1.x)

When `VCR_AGENT_MODE=1` and a command fails, VCR emits a machine-readable JSON payload to stderr.

Contract (v0.1.x, additive-only):

- Required keys:
  - `error_type` (string)
  - `summary` (string)
- Optional keys:
  - `suggested_fix` (object)
  - `context` (object)
  - `validation_errors` (array of objects)
- Compatibility policy:
  - Existing keys keep their meaning within `v0.1.x`
  - New keys may be added
  - Consumers must tolerate unknown fields and absent optional fields

Example payload (missing required `environment.duration`):

```json
{
  "error_type": "validation",
  "summary": "failed to decode manifest. missing field `duration`",
  "suggested_fix": {
    "description": "Add the required environment.duration field",
    "yaml_snippet": "environment:\n  duration: 3.0",
    "actions": [
      {
        "type": "add_field",
        "path": "environment",
        "field_name": "duration",
        "example_value": 3.0
      }
    ]
  },
  "context": {
    "file": "/absolute/or/relative/path/to/scene.vcr"
  },
  "validation_errors": [
    {
      "path": "environment.duration",
      "message": "failed to decode manifest. missing field `duration`"
    }
  ]
}
```

## Project Docs

- `docs/PRD.md` - Product Requirements Document (Vision & Roadmap)
- `docs/PARAMS.md` - typed params and `--set` semantics
- `docs/PLAYGROUND.md` - preset playground runner and outputs
- `docs/CHAT_RENDER.md` - tagged transcript to animated terminal video
- `docs/ASCII_STAGE.md` - stylized `.vcrtxt` transcript rendering with camera/preset options
- `docs/ASCII_CAPTURE.md` - animated ASCII capture (`ascii-live` / `chafa`) to ProRes MOV
- `docs/ASCII_SOURCES.md` - curated static registry for animated ASCII stream/pack/tool sources
- `docs/ASCII_CAPTURE_MILESTONE.md` - current Earth overlay quality/status and recommended command
- `docs/deep-research-report.md` - imported deep research report on ASCII as a rendering medium
- `docs/ascii_research_takeaways.md` - prioritized, codebase-specific pull-through items from the report
- `docs/SKILLS_PROTOCOL.md` - agent update protocol
- `docs/EXIT_CODES.md` - CLI exit code contract
- `docs/ARCHITECTURE.md` - system overview
- `docs/DETERMINISM_SPEC.md` - determinism contract
- `docs/REPRODUCIBLE_BUILD.md` - build instructions
- `docs/BENCHMARK_REPORT.md` - benchmark guide

## License

[MIT License](LICENSE)

## Author

Colton Batts ([@coltonbatts](https://github.com/coltonbatts))
