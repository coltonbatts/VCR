# PRD: VCR (Video Component Renderer)

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

* **Manifest-Driven**: Scened defined in YAML with support for layers, timing, and scalar expressions.
* **Dual-Backend**:
  * **GPU (WGPU)**: High-performance rendering for local previews and production.
  * **Software (Tiny-Skia)**: Bit-exact rendering for CI/CD and verification.
* **Scalar Expressions**: Mathematical control over properties (`sin`, `cos`, `noise1d`, `clamp`) without full scripting complexity.

### 5.3 Agent-First Workflow

* **Agent Error Contract**: Machine-readable JSON error payloads (via `VCR_AGENT_MODE=1`) that suggest specific fixes to the AI.
* **SKILL.md Integration**: A comprehensive reference document designed for LLM context windows.

### 5.4 Specialized ASCII Modules

* **ASCII Stage**: Converts chat transcripts (`.vcrtxt`) into stylized terminal animations.
* **ASCII Capture**: Bridge for capturing live or library-based ASCII animations into ProRes 4444 video.
* **Deterministic Physics**: First-class support for Rapier physics sims within the rendering pipeline.

### 5.5 Ecosystem Integrations

* **Figma-VCR Workflow**: Direct conversion from Figma designs to VCR manifests.
* **Sidecar Metadata**: Every render produces a `.metadata.json` documenting frame hashes, parameters, and source attribution.

## 6. Technical Requirements

* **Language**: Rust (Stable)
* **Dependencies**: FFmpeg (encoding), WGPU (rendering), Serde (serialization).
* **Platforms**: macOS (Primary), Linux/WSL (Headless/Software).

## 7. Roadmap

### Phase 1: Core Stability (Current)

* [x] Deterministic Software Backend.

* [x] YAML Schema and Expression Support.
* [x] Basic CLI (build, preview, lint).
* [x] Agent Error Contract.

### Phase 2: Asset & Creative Expansion (Near-term)

* [ ] **Unified Rendering Bridge**: Integrating ThreeJS and WebGL-based sources into the ProRes alpha pipeline.
* [ ] **Procedural Expansion**: Shaders, Noise, Particle Systems, and physics-aware layers.
* [ ] **Temporal Coherence**: Hysteresis and smoothing for unstable mediums (ASCII, dithering).
* [ ] **Perceptual Glyph Selection**: Advanced metrics for better visual representation in ASCII/Dithered modes.
* [ ] **Native Alpha Orchestration**: Refined controls for layering multiple high-fidelity sources with transparency.

### Phase 3: Advanced Orchestration (Long-term)

* [ ] **High-Density Render Modes**: Advanced Unicode and block-element rendering with layout stability.
* [ ] **Multi-agent Scene Coordination**: Protocol for complex, multi-source timeline collaboration.
* [ ] **Real-time "Hot Reload" Preview TUI**: Dashboard for orchestrating Rust, ThreeJS, and Shader layers.
* [ ] **Plugin System**: Standardized architecture for custom GLSL/WGSL post-processing.

## 8. Success Metrics

* **Determinism**: 100% bit-identity on software-backend golden tests.
* **Speed**: <1s render time for standard 1080p frames.
* **Agent Autonomy**: Decrease in human intervention required for AI-generated scene fixes.
