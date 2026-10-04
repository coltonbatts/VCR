# VCR Architecture

## 1. Overview

VCR is a deterministic video renderer: YAML scene manifests → reproducible frame output. No SaaS. No network in core. Offline-first.

## 2. Components

### 2.1 Rendering Core

- **Scope**: Parse manifest → evaluate expressions → render frames → emit RGBA.
- **Location**: `src/renderer.rs`, `src/timeline.rs`, `src/schema.rs`, `src/post_process.rs`, `src/ascii*.rs`
- **Backends**: GPU (wgpu/Metal) or software (tiny-skia). Determinism tests use software only. The software backend supports asset/image/procedural/text/ascii/sequence layers; `shader`, `wgpu_shader`, `video`, and `lottie` layers fail with `UNSUPPORTED_SOFTWARE_LAYER_TYPES` instead of being silently dropped.
- **Pipeline**: Manifest → validated schema → layer evaluation per frame → compositing → optional post (levels, ASCII).

### 2.2 CLI

- **Scope**: Subcommands, flags, exit codes, machine-readable output.
- **Location**: `src/main.rs`
- **Contract**: Stable flags, documented exit codes, and the agent contract `vcr.agent/1` (`docs/AGENT_CONTRACT.md`): `--json` on capabilities/prompt/check/lint/explain/inspect/render/verify/doctor prints one envelope document on stdout. `vcr explain --json` is the backend preflight: resolved backend on this machine, runtime probes (ffmpeg, ffprobe, fonts, GPU), layers and manifest features the software backend cannot honor (`post`, `ascii_post`), font fallback, and blockers.
- **Modules**: `agent_contract` (envelope, error taxonomy, engine identity), `capabilities` (discovery, schema derived with `schemars`), `preflight`, `inspect`, `media_verify`, `provenance` (library); `src/agent_cli.rs` (`--json` operations).

### 2.3 Optional Workflow / Integration Layers

- **Scope**: Figma → VCR, agent workflows, external APIs.
- **Location**: `src/workflow/`, `src/bin/figma-vcr-workflow.rs`
- **Dependency**: Requires `FIGMA_TOKEN`, optional `ANTHROPIC_API_KEY`. Core render does **not** use these.

### 2.4 Sidecars

- **Tape Deck**: `vhs-tape-deck/` is the supported Go TUI sidecar for repeatable tape runs. It shares the same default config path as `vcr tape`, and CI runs its test suite.
- **Three.js renderer**: `threejs_renderer/` is an optional experimental Node sidecar for 3D rendering experiments. It is not part of the core render path.

## 3. In Scope vs Out of Scope

| In Scope | Out of Scope |
|----------|--------------|
| Deterministic frame output | Cross-platform GPU bit-exactness |
| Offline manifest rendering | Realtime editing |
| Param overrides, typed DSL | Loops, conditionals, arbitrary scripting |
| Software fallback for determinism | SaaS, hosted rendering |
| Golden tests, frame hashing | Marketing features |

## 4. Determinism Definition

**Deterministic**: Same manifest + params + seed + backend → identical frame bytes.

- **Software backend**: Bitwise identical on same machine. Cross-platform may differ (float, fonts).
- **GPU backend**: Not guaranteed bit-exact across drivers/hardware. Use software for CI and verification.
- **Scope**: Same machine + same toolchain + same backend. See `docs/DETERMINISM_SPEC.md`.

## 5. Classification

| Category | Contents |
|----------|----------|
| **Core** | Manifest parsing, schema, timeline, renderer, encoding, post-process |
| **Tooling** | CLI, doctor, lint, params, explain |
| **Experimental** | ASCII stage/capture, chat render, figma workflow |
| **Dead weight** | Archived scenes, unused Go modules, stray experiment binaries |

## 6. Boundaries

- **Core render**: No `reqwest`, no `tokio`, no external API calls. Offline.
- **Integrations**: Live in `integrations/` or behind feature flags. Require network/tokens.
- **DSL**: Static manifest. Param substitution `${name}`. Expression language for scalars (sin, clamp, noise1d). No loops, no conditionals, no templating engines.
