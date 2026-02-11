# VCR (Video Component Renderer)

![Welcome to VCR](assets/welcome%20to%20VCR.jpg)

VCR is a local, deterministic motion graphics engine in Rust with an agent-first workflow on top.
It turns structured scene specs into repeatable renders, preview frames, and metadata that can be consumed by humans, scripts, or agents.

## What This Repo Is

- A rendering core (`vcr`) for YAML motion manifests
- A workflow binary (`figma-vcr-workflow`) that converts Figma + prompt context into VCR-ready outputs
- A practical playground for steerable presets (`scripts/run_playground.sh`)

This is not a SaaS product. It is a buildable, inspectable toolchain.

## Why It Exists

Most motion design tools are hard to version, hard to automate, and hard to run deterministically.
VCR exists to make motion generation scriptable and reproducible.

## What Works Today

- Typed params with runtime overrides (`--set`)
- Deterministic output and deterministic metadata sidecars
- GPU backend on supported systems with software fallback
- Preview/build/watch flows via CLI
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
- `chat render`
- `ascii stage`
- `doctor`

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

Exit code contract is documented in `docs/EXIT_CODES.md`.
Param semantics are documented in `docs/PARAMS.md`.

## Project Docs

- `docs/PARAMS.md` - typed params and `--set` semantics
- `docs/PLAYGROUND.md` - preset playground runner and outputs
- `docs/CHAT_RENDER.md` - tagged transcript to animated terminal video
- `docs/ASCII_STAGE.md` - stylized `.vcrtxt` transcript rendering with camera/preset options
- `docs/SKILLS_PROTOCOL.md` - agent update protocol
- `docs/EXIT_CODES.md` - CLI exit code contract

## License

MIT

## Author

Colton Batts ([@coltonbatts](https://github.com/coltonbatts))
