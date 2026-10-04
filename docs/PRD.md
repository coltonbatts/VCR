# PRD: VCR (Video Component Renderer)

**Document status:** Last updated **2026-04-08**. The project is **actively maintained** again after a quiet period; this PRD is the canonical place for vision, scope, and phased delivery. Use it when opening issues or RFC-scale PRs so proposals stay aligned with the product arc.

## 1. Executive Summary

**VCR** is a local-first, deterministic motion graphics infrastructure written in Rust. It serves as a **unified "one-stop shop"** for generating high-quality motion assets with alpha transparency. By bridging the gap between AI agents and broadcast-quality production, VCR allows developers to orchestrate multiple graphics paradigms—from ASCII art and procedurals to advanced Shaders and ThreeJS simulations—into pixel-perfect, reproducible ProRes 4444 video.

## 2. Problem Statement

The current motion design landscape faces three primary challenges:

1. **Automation Fragility**: Professional tools (After Effects, etc.) are difficult to automate and version control.
2. **AI Hallucinations**: Direct video generation from LLMs/Diffusion models lacks precision and deterministic control.
3. **Reproducibility**: Most rendering pipelines are platform-dependent, making it hard to guarantee bit-exact output across CI/CD environments.

## 3. Product Vision

To become the **universal rendering target for Agentic Motion Design**, where any graphics source (Rust Core, Web/ThreeJS, Shaders) can be compiled into a broadcast-ready alpha-composite asset with deterministic execution.

## 4. Target Personas

* **AI Agents**: Programmatic consumers that need to generate video via structured data (YAML) and receive machine-readable feedback.
* **Creative Technologists**: Users who want to build custom motion workflows without the overhead of heavy GUI software.
* **Infrastructure Engineers**: Developers looking for a reliable, headless video rendering core for automated pipelines.

## 5. Key Features & Capabilities

### 5.1 Technical Foundation: Deterministic Rendering

* **Cell-Grid Logic**: Every render is modeled as a discrete 2D lattice, separating symbol selection from glyph rasterization.
* **Aspect Ratio Compensation**: Built-in math for mapping high-res pixels to non-square terminal cells (e.g., 9x16).
* **Luminance Mapping**: Deterministic Luma (Rec.601/709) and linear-light relative luminance extraction.
* **Ordered Dithering & Error Diffusion**: Stable Floyd-Steinberg and Bayer dithering implementations.
* **Unicode Lattice Stability**: Strict column-width modeling (POSIX `wcwidth()`) to prevent horizontal jitter in high-density renders.

### 5.2 Deterministic Rendering Core

* **Manifest-Driven**: Scenes defined in YAML with support for layers, timing, and scalar expressions.
* **Dual-Backend**:
  * **GPU (WGPU)**: High-performance rendering for local previews and production.
  * **Software (Tiny-Skia)**: Bit-exact rendering for CI/CD and verification.
* **Scalar Expressions**: Mathematical control over properties (`sin`, `cos`, `noise1d`, `clamp`) without full scripting complexity.

### 5.3 Agent-First Workflow

* **Prompt gate (`vcr prompt`)**: Normalizes natural language or loose YAML into a structured bundle (`standardized_vcr_prompt`, `normalized_spec`, `unknowns_and_fixes`) before manifest authoring—reduces silent invention of missing parameters.
* **Agent Error Contract**: Machine-readable JSON error payloads (via `VCR_AGENT_MODE=1`) that suggest specific fixes to the AI.
* **SKILL.md / AGENTS.md**: Reference documents designed for LLM and automation context windows.

### 5.4 Agent-Native Interface Layer

Treat AI agents as first-class users of the product, not just authors of manifests.

* **Semantic surfaces**: Every command, layer, and render artifact should expose stable identifiers, explicit roles, and machine-readable state.
* **Direct structured access**: Prefer CLI JSON, metadata sidecars, and MCP tools over screenshot-driven or log-scraping workflows.
* **Predictable failure modes**: Ambiguity must surface as a blocking normalization issue or structured error, not as hidden defaults.
* **Human + agent parity**: Keep the terminal workflow human-friendly, but ensure the same actions are available in structured form for automation.

### 5.5 Tapes-First Operator UX

* **Tape workflow**: Versioned manifests as immutable “tapes,” with CLI helpers for init/list/run.
* **Deck**: `vcr deck` launches the interactive controller; the Go **VHS Tape Deck** (`vhs-tape-deck/`) provides a Bubble Tea UI for the same mental model.

### 5.6 Specialized ASCII Modules

* **ASCII Stage**: Converts chat transcripts (`.vcrtxt`) into stylized terminal animations.
* **ASCII Capture**: Bridge for capturing live or library-based ASCII animations into ProRes 4444 video.
* **Deterministic Physics**: First-class support for Rapier physics sims within the rendering pipeline.

### 5.7 Ecosystem Integrations

* **Figma-VCR Workflow**: Direct conversion from Figma designs to VCR manifests (optional `workflow` feature).
* **Three.js sidecar (experimental)**: `threejs_renderer/` — headless Three.js + Puppeteer path for 3D-sourced frames; integration with the main ProRes pipeline remains **Phase 2** work.
* **Sidecar Metadata**: Every render produces a `.metadata.json` documenting frame hashes, parameters, and source attribution.

## 6. Technical Requirements

* **Language**: Rust (Stable)
* **Dependencies**: FFmpeg (encoding), WGPU (rendering), Serde (serialization).
* **Platforms**: macOS (Primary), Linux/WSL (Headless/Software).

## 7. Roadmap

### Near-term focus (maintenance restart, 2026 H1)

Priority is **shipping reliability and contributor clarity**, not scope expansion:

1. Keep **determinism and CI** green (software backend, contract tests, agent-mode errors).
2. **Document and harden** the tapes + Deck path so new users can render without memorizing flags.
3. **Close the loop** on the Three.js sidecar: reproducible setup, documented hand-off into the Rust encoder, and CI smoke where feasible.
4. **Make agent behavior explicit**: document the structured contract for prompt normalization, metadata, and render planning so agents can operate without guessing.

### Phase 1: Core Stability (largely complete)

* [x] Deterministic software backend (with explicit GPU vs CPU behavior).
* [x] YAML schema and expression support.
* [x] CLI: check, lint, render/build, preview, library/packs workflows.
* [x] Agent error contract (`VCR_AGENT_MODE=1`).
* [x] Prompt gate (`vcr prompt`) for normalized agent authoring.
* [x] Tapes-first workflow and Deck entrypoints (`vcr deck`, `vhs-tape-deck/`).

### Phase 2: Asset & Creative Expansion (Near-term)

* [ ] **Unified rendering bridge**: Wire **Three.js / WebGL** sources into the ProRes alpha pipeline end-to-end (building on `threejs_renderer/`).
* [ ] **Procedural Expansion**: Shaders, Noise, Particle Systems, and physics-aware layers.
* [ ] **Temporal Coherence**: Hysteresis and smoothing for unstable mediums (ASCII, dithering).
* [ ] **Perceptual Glyph Selection**: Advanced metrics for better visual representation in ASCII/Dithered modes.
* [ ] **Native Alpha Orchestration**: Refined controls for layering multiple high-fidelity sources with transparency.
* [ ] **Agent-native contract**: Expand `vcr prompt`, `vcr check`, `vcr lint`, and render metadata so every automation step has a stable machine-readable input/output contract.
* [ ] **Agent-safe state export**: Add or extend metadata fields for layer roles, supported actions, unresolved ambiguities, and last-evaluated layer state.
* [ ] **Structured planning API**: Expose render planning and preview decisions through MCP/JSON so agents can inspect what VCR will do before building.

### Phase 3: Advanced Orchestration (Long-term)

* [ ] **High-Density Render Modes**: Advanced Unicode and block-element rendering with layout stability.
* [ ] **Multi-agent Scene Coordination**: Protocol for complex, multi-source timeline collaboration.
* [ ] **Real-time "Hot Reload" Preview TUI**: Dashboard for orchestrating Rust, ThreeJS, and Shader layers.
* [ ] **Plugin System**: Standardized architecture for custom GLSL/WGSL post-processing.

## 8. Success Metrics

* **Determinism**: 100% bit-identity on software-backend golden tests.
* **Speed**: <1s render time for standard 1080p frames.
* **Agent Autonomy**: Decrease in human intervention required for AI-generated scene fixes.

## 9. ASPECT_PRESET_SPEC_V1

Allowed aspect preset set (closed enum):

- `cinema`
- `social`
- `phone`

Exact pixel dimensions (normative):

- `cinema` = `1920x1080`
- `social` = `1080x1350`
- `phone` = `1080x1920`

Safe-area insets (integer-only, deterministic):

- `cinema`: 5% inset on all sides
- `social`: 6% inset on all sides
- `phone`: 7% inset on all sides
- Rounding rule: `inset_px = floor(dimension_px * inset_percent / 100)` using integer division

Grid-to-canvas mapping rule (integer-only letterbox):

- Grid presets remain unchanged (for example `120x45`, `80x24`)
- Grid content is rasterized first, then placed into a centered content window
- Scale is an integer only: `scale = min(content_window_w / src_w, content_window_h / src_h)`
- Fractional scales are forbidden
- No cropping is allowed
- Centering tie-break is deterministic: for odd remainder, extra pixel goes to right/bottom

Output conventions:

- Output folder must include aspect: `out/<pack_id>/<pack_version>/<aspect>_<fps>/`
- Artifact filename must include aspect:
  `<pack_id>__<artifact_id>__<aspect>__<fps>__core-<core_version>__pack-<pack_version>.mov`

Versioning rule:

- Any change to the allowed aspect set, dimensions, safe-area math, or mapping behavior requires a `core_version` bump.
