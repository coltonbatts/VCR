# VCR Project Custodian Overview

**Last Updated**: February 16, 2026
**Project**: VCR (Video Component Renderer)
**Version**: 0.1.2
**Status**: Release Reading (v1 Release)
**Language**: Rust
**Author**: Colton Batts

---

## Executive Summary

VCR is a **deterministic, local-first motion graphics rendering engine** built in Rust. It's designed for AI agents to author declarative YAML motion manifests that compile into reproducible video output. The core value proposition: **infrastructure for AI-generated motion graphics** where the engine guarantees pixel-perfect, reproducible renders with no hallucinations.

---

## Project Structure at a Glance

```text
VCR/
├── src/                    # Core library (39 Rust files)
│   ├── main.rs            # CLI entry point
│   ├── lib.rs             # Public API
│   ├── manifest.rs        # YAML manifest parsing & validation
│   ├── renderer.rs        # GPU rendering pipeline
│   ├── encoding.rs        # Video output encoding
│   ├── ascii_*.rs         # ASCII rendering subsystem
│   ├── workflow/          # Figma→VCR integration
│   ├── bin/               # Additional binaries (ascii-link-overlay, figma-vcr-workflow, etc.)
│   └── packs/             # Frame pack system
├── examples/              # 28 .vcr manifest examples
├── docs/                  # technical and custodian documentation
│   ├── PROJECT_CUSTODIAN.md # This file
│   └── ...                # 18+ other documentation files
├── .skills/               # 6 custom agent skills
├── .github/               # GitHub workflows and community docs
│   ├── CODE_OF_CONDUCT.md
│   ├── CONTRIBUTING.md
│   └── SECURITY.md
├── scripts/               # Automation & CLI helpers
├── tests/                 # 12 integration tests
├── benches/               # Performance benchmarks
├── assets/                # Fonts, glyph atlases, animations
├── renders/               # Output videos & preview frames
├── Cargo.toml             # Dependencies & features
├── Cargo.lock             # Lockfile
├── AGENTS.md              # Agent protocol
└── SKILL.md               # Primary AI reference
```

---

## Core Components

### 1. **Rendering Core** (`src/renderer.rs`, `src/encoding.rs`)

- GPU rendering pipeline (wgpu 0.20 with Metal support on macOS)
- Software fallback for deterministic rendering
- ProRes 4444 video output
- Frame-by-frame pipeline for repeatability

**Status**: ✅ Mature
**Key Dependencies**: `wgpu`, `tiny-skia`, `image`
**Known Limitations**:

- GPU path is platform-specific (macOS Metal native, others via software)
- FFmpeg integration is phase 2 (scaffolding exists)

---

### 2. **Manifest System** (`src/manifest.rs`, `src/schema.rs`)

- YAML-based scene description language
- Typed parameters with runtime overrides (`--set`)
- Expression system for animatable properties
- Validation & error reporting with agent-mode JSON output

**Status**: ✅ Stable
**Format**: `.vcr` files
**Examples**: 28 complete examples in `examples/`

---

### 3. **ASCII Rendering Subsystem** (7 related files)

- `ascii_stage.rs` - Transcript → styled terminal video
- `ascii_capture.rs` - Animated ASCII → ProRes encoding
- `ascii_pipeline.rs` - Frame processing pipeline
- `ascii_sources.rs` - Curated library of ASCII animations

**Status**: ⚠️ In Development
**Key Use Cases**:

- Social-friendly vertical clips
- Tool-call transcripts
- ASCII art overlays

**Missing/TODO**:

- Live streaming integration (`ascii-live:*` protocol partially stubbed)
- More curated sources
- Better color handling for ASCII

---

### 4. **Workflow Engine** (`src/workflow/`)

- Figma design → VCR manifest conversion
- `figma-vcr-workflow` binary for agent integration
- Asset extraction and media handling

**Status**: 🟡 Partially Complete
**Key Files**:

- `figma_client.rs` - Figma API integration
- `manifest_generator.rs` - YAML generation from design
- `vcr_renderer.rs` - Render coordination

**Missing**:

- Full Frame.io integration (stubbed in code)
- Real-time Figma→VCR sync
- Batch processing for multiple files

---

### 5. **Frame Pack System** (`src/packs/`, `src/animation_engine.rs`)

- Import/export frame sequences as reusable components
- Three.js + Rapier physics boilerplate for deterministic animation
- Glyph atlas generation for text rendering

**Status**: 🟡 Active
**Key Feature**: Frame packs live in `assets/animations/<name>/`

---

## External Interfaces & Integrations

### Binaries (in `src/bin/`)

1. **`vcr`** (main CLI) - Rendering, linting, watching
2. **`figma-vcr-workflow`** - Figma→VCR conversion
3. **`ascii-link-overlay`** - ASCII.co.uk URL → frame pack
4. **`ascii_explore`** - Debug ASCII rendering
5. **`generate_ascii_atlas`** - Build glyph atlases
6. **`generate_glyph_atlas_png`** - PNG glyph sheet generation

### Custom Skills (in `.skills/`)

1. **`vcr-manifest-author`** - AI-guided manifest creation
2. **`vcr-ascii-pipeline`** - ASCII workflow orchestration
3. **`vcr-figma-workflow`** - Design-to-render conversion
4. **`vcr-debugger`** - Manifest inspection & validation
5. **`vcr-style-skill-template`** - Template for new skills

---

## Output Artifacts

### Render Outputs

- **Video**: ProRes 422/4444 MOV files (deterministic frame hash tracking)
- **Stills**: PNG sequences with metadata sidecars
- **Metadata**: `.metadata.json` for each render (attribution, artist tags, frame info)

### Key Output Directories

- `renders/` - Full renders and test outputs
- `renders/baseline/` - Determinism baselines for CI
- `renders/playground/` - Preset testing outputs
- `preview_frames/` - Quick preview stills

---

## Testing & Validation

### Test Suite (12 integration tests)

1. **`cli_contract.rs`** - CLI interface compliance
2. **`determinism.rs`** - Frame hash reproducibility
3. **`params_reliability.rs`** - Parameter validation & overrides
4. **`spec_compliance.rs`** - Manifest format compliance
5. **`agent_error_reporting.rs`** - JSON error contract (v0.1.x)
6. **`pack_compile_cli.rs`** - Frame pack compilation
7. **`aspect_preset_determinism.rs`** - Preset consistency
8. **`sequence_determinism.rs`** - Timeline ordering
9. **`font_assets.rs`** - Font loading & fallback
10. **`wgpu_shader_smoke.rs`** - Basic GPU shader tests
11. **`security_tests.rs`** - Manifest injection/escape testing

### Benchmarks

- `render_frame.rs` - Performance baseline (criterion)

### CI/CD

- **`ci.yml`** - Main test + build pipeline
- **`baseline.yml`** - Determinism baseline capture

---

## Documentation Inventory

| File | Purpose | Status |
|------|---------|--------|
| `SKILL.md` | **Primary AI reference** - manifest format, CLI, expressions | ✅ Root (Standardized) |
| `AGENTS.md` | Agent protocol & self-identification | ✅ Root (Standardized) |
| `PROJECT_CUSTODIAN.md` | Custodian overview & project map | ✅ Relocated to `docs/` |

---

## Scripts & Automation

### Available Commands

1. **`./scripts/baseline_report.sh`** - Capture determinism baseline across manifest matrix
2. **`./scripts/run_playground.sh`** - Render all 9 presets for demo scenes
3. **`./scripts/ascii_link_overlay.sh`** - Batch import ASCII.co.uk URLs
4. **`./scripts/vcr-mcp-server/`** - MCP server for Claude/agent integration

---

## Dependencies & Features

### Core Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `wgpu` | 0.20 | GPU rendering |
| `serde_yaml` | 0.9 | Manifest parsing |
| `image` | 0.25 | Image I/O (JPEG, PNG, WebP) |
| `fontdue` | 0.9 | Font rasterization |
| `rapier3d` | 0.19 | Physics engine |
| `tokio` | 1.0 | Async runtime |
| `clap` | 4.5 | CLI argument parsing |
| `tiny-skia` | 0.11 | 2D drawing |
| `egui` | 0.28 | UI for TUI (experimental) |

### Optional Features

- **`sidecar_ffmpeg`** - External FFmpeg encoding (disabled by default)
- **`wgpu_layers`** - Advanced GPU layer features (unstable)

---

## What's Complete ✅

1. **Core rendering pipeline** - GPU + CPU fallback, deterministic output
2. **Manifest language** - Full YAML spec with expressions, params, validation
3. **CLI interface** - 16+ commands with robust error handling
4. **ASCII subsystem** - Styling, transitions, camera moves, presets
5. **Determinism guarantees** - Frame hash tracking, reproducible builds
6. **Test coverage** - 12 integration tests, benchmark suite
7. **Agent integration** - JSON error contract, agent mode, skill reference
8. **Documentation** - Comprehensive docs + SKILL.md reference
9. **Example library** - 28 working manifest examples
10. **Frame pack system** - Physics boilerplate, animation engine

---

## What's Missing or Incomplete 🟡

### High Priority

1. **Frame.io integration** - Stubbed in `workflow/frame_client.rs`, incomplete
2. **FFmpeg sidecar mode** - Scaffolded but feature-flagged; needs testing
3. **Live ASCII streaming** - `ascii-live:*` protocol partially stubbed
4. **Batch manifest processing** - No multi-scene compilation workflow
5. **Web-based preview** - Currently TUI-only (egui integration started but incomplete)

### Medium Priority

1. **Audio sync** - No audio track support yet
2. **3D mesh rendering** - Rapier physics works, but no mesh import/export
3. **Shader hot-reload** - Watch mode exists but shader changes require rebuild
4. **Color management** - No ICC profile or HDR support
5. **More ASCII sources** - Library is small; needs community contributions

### Low Priority (Nice to Have)

1. **Plugin system** - No Lua/WASM plugin support
2. **Web assembly target** - Rust compiles but GPU path untested
3. **Docker containerization** - No official Docker image
4. **Remote rendering** - No server mode for distributed rendering
5. **Diffing tool** - No visual diff between renders

---

## Known Issues & Caveats

1. **GPU Determinism** - GPU path is not fully deterministic across different hardware; use `--backend software` for CI/guaranteed reproducibility
2. **macOS Metal Only** - GPU rendering uses Metal on macOS; Linux/Windows fall back to software
3. **Font Fallback** - Limited font fallback chain; missing characters may render as blank
4. **Huge Video Files** - ProRes output is uncompressed; 1-hour video = ~2-3 GB
5. **No Real-time Preview** - Preview is image sequence; live playback requires external media player
6. **Figma API Rate Limits** - Workflow binary not rate-limit aware; batching needed for 100+ files
7. **ASCII Color Depth** - Terminal ASCII limited to 256 colors; no 24-bit truecolor yet

---

## Quick Command Reference

```bash
# Build everything
cargo build --release

# Run tests
cargo test

# Lint a manifest
./target/release/vcr lint examples/demo_scene.vcr

# Preview (image sequence)
./target/release/vcr preview examples/demo_scene.vcr --image-sequence -o renders/preview

# Render to video
./target/release/vcr build examples/demo_scene.vcr -o renders/output.mov

# Override parameters at render time
./target/release/vcr preview examples/steerable_motion.vcr --set speed=2.2 --set accent_color=#4FE1B8

# ASCII transcript → video
./target/release/vcr ascii stage --in examples/ascii/demo.vcrtxt --out renders/ascii.mp4 --preset x

# Capture ASCII animation library
./target/release/vcr ascii capture --source library:geist-wave --out renders/wave.mov

# Run all tests
cargo test --test '*'

# Determinism check
cargo test --test determinism

# Baseline for CI
./scripts/baseline_report.sh

# Playground (all presets)
./scripts/run_playground.sh
```

---

## File Inventory by Category

### Configuration Files

- `Cargo.toml` / `Cargo.lock` - Rust dependencies
- `.gitignore` - Git exclusions
- `.mcp.json` - MCP server config
- `agent_manifest.yaml` - Agent protocol definition

### Example Scenes (28 files)

**Tier 1 (Learning)**:

- `demo_scene.vcr` - Basic intro
- `envelope_scene.vcr` - Simple shape animation
- `skill_01_static_shapes.vcr` through `skill_05_custom_shader.vcr` - Progressive tutorials

**Tier 2 (Production)**:

- `instrument_*.vcr` (3 files) - Complex compositions
- `geist_*.vcr` (3 files) - Commercial demos
- `terminal_*.vcr` (3 files) - ASCII/terminal effects
- `typewriter_preset.vcr` - Preset demo

**Tier 3 (Experimental)**:

- `dreamcore_*.vcr` - Experimental AI-generated
- `retro_pyramid.vcr` - Minimal example
- `wgpu_shader_test.vcr` - GPU shader testing

### Agent Skills (5 custom skills)

All in `.skills/` with their own `SKILL.md` files:

- vcr-manifest-author
- vcr-ascii-pipeline
- vcr-figma-workflow
- vcr-debugger
- vcr-style-skill-template

### Assets

- `assets/fonts/` - TTF font files
- `assets/glyph_atlas/` - Pre-baked glyph sheets
- `assets/animations/` - Frame pack library (ASCII, physics, etc.)
- `assets/readme_assets/` - Documentation images

---

## Project Health Checklist

| Aspect | Status | Notes |
|--------|--------|-------|
| **Core Functionality** | ✅ Stable | Manifest parsing, rendering, encoding working |
| **Test Coverage** | ✅ Good | 12 integration tests + bench suite |
| **Documentation** | ✅ Comprehensive | SKILL.md is primary reference; 18 docs total |
| **CI/CD** | ✅ Active | `ci.yml` and `baseline.yml` configured |
| **Dependencies** | ✅ Managed | Cargo dependencies locked; no deprecated packages |
| **Code Quality** | ✅ High | Few TODO comments (only 2 in src/) |
| **Performance** | 🟡 Monitored | Benchmark exists; determinism tracked |
| **GPU Support** | 🟡 Limited | Metal on macOS; software fallback elsewhere |
| **Examples** | ✅ Rich | 28 working examples across difficulty levels |
| **Error Handling** | ✅ Mature | Agent-mode JSON errors + exit code contract |
| **Feature Completeness** | 🟡 70% | Core done; Figma workflow & ASCII streaming in progress |

---

## Recommended Next Steps

### For You (Project Custodian)

1. **Weekly**: Review render outputs in `renders/` for quality regressions
2. **Monthly**: Run determinism baseline (`./scripts/baseline_report.sh`)
3. **Per Release**: Update `CHANGELOG.md` and bump version in `Cargo.toml`
4. **As Needed**: Update `docs/` to reflect feature additions

### For Contributors / AI Agents

1. Read `SKILL.md` first (primary reference)
2. **Visual Verification is Mandatory**: AI agents MUST generate a 3x3 contact sheet using `scripts/vcr_contact_sheet.py` before proposing or delivering any VCR manifest or shader.
3. Study `examples/skill_01_*.vcr` through `skill_05_*.vcr` for progression
4. Use `vcr lint` and `vcr explain` to validate manifests
5. Run `cargo test` before committing changes
6. Check determinism with `cargo test --test determinism`

### To Close Feature Gaps

1. **Frame.io**: Complete `workflow/frame_client.rs` for video review integration
2. **FFmpeg**: Test & stabilize sidecar mode (feature-flagged)
3. **ASCII Live**: Implement `ascii-live:*` protocol for streaming
4. **Audio**: Design audio track format + sync mechanism
5. **Web Preview**: Complete egui integration for interactive preview

---

## Contact & Governance

**Maintainer**: Colton Batts
**Repository**: <https://github.com/coltonbatts/VCR>
**License**: MIT
**Stability**: Pre-1.0 (API may change)

---

## Summary

VCR is a **well-organized, actively maintained motion graphics rendering engine** with:

- ✅ Solid core functionality (rendering, manifest system, ASCII effects)
- ✅ Comprehensive testing and determinism guarantees
- ✅ Rich documentation and 28 working examples
- ✅ Clean architecture with modular subsystems
- 🟡 Some incomplete integrations (Figma workflow, FFmpeg, Frame.io)
- 🟡 Room for community contributions (ASCII sources, plugins, etc.)

The project is production-ready for **core rendering workflows** and **AI-driven scene generation**. Experimental features (live ASCII, web preview, advanced physics) are in active development.

---

**Last Custodian Review**: February 13, 2026
