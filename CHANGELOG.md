# Changelog

## Unreleased

- **MCP**: the adapter in `scripts/vcr-mcp-server/` is rewritten as thin typed wrappers over the CLI contract (explicit engine selection and identity, process-group timeouts, no adapter-side defaults, no mandatory LLM). `vcr_render_plan` loses its override parameters.
- **Docs**: `docs/AGENT_QUICKSTART.md` (the short agent workflow), README/AGENTS reconciled with the contract, a handoff note and the first lower-third proof evidence under `docs/agent-first/`.
- **Benchmark**: a pre-registered 10-task agent benchmark with a scorer and scripted reference (`docs/agent-benchmark/`). No autonomous-agent runs have been performed.
- **Installer**: `scripts/install.sh` no longer deletes a working install; the old one is moved aside and restored on any failure.
- **Verified delivery**: `vcr render`/`build` publish atomically (hidden partial file → conformance check → rename), remove stale sidecars first, and write `*.provenance.json` (engine build, ffmpeg, backend, inputs, raster/decoded/file hashes, determinism scope). `render --json`, `build --json` and `render-frame --json` emit the contract envelope; `render --json` keeps its legacy top-level keys.
- **`vcr verify` now verifies** resolution, exact frame rate, frame count, duration, container, codec/profile, alpha capability and decoded-pixel transparency, and detects stale, modified and truncated output. **It exits 3 on mismatch** (it used to print only a hash). Legacy `--json` keys preserved.
- Determinism docs split into scene / raster / decoded+encoded levels; the encoded level depends on the ffmpeg build.

- **Discovery**: `vcr capabilities [--schema] --json` reports what the installed engine can do and what is usable on this machine (layers with backend requirements, fonts, encoding profiles, expression functions, explicit time units). The manifest JSON Schema is generated from the engine's types with `schemars` (new dependency); the expression-function table has a drift test.
- **Inspection**: `vcr inspect` samples the timeline (entrance, hold, exit, ending), writes a labelled contact sheet over an alpha checkerboard, and reports exact per-layer bounds plus clipping and timing diagnostics.

- **Preflight** (`vcr explain --json`, alias `vcr preflight`): resolved backend, ffmpeg/ffprobe/font/GPU probes, layers and manifest features the software backend cannot honor, font-fallback detection, a single `ready` flag, `--strict`. Output is in the `vcr.agent/1` envelope; legacy top-level keys are preserved.
- **Behavior change:** the software backend now refuses manifests with `post:` / enabled `ascii_post:` (`UNSUPPORTED_SOFTWARE_FEATURES`) instead of silently ignoring them, including when `auto` falls back to software. `vcr doctor` also checks `ffprobe`.
- SKILL.md no longer claims shader layers fall back to transparent or that `post:` is skipped on software.

- **Agent contract `vcr.agent/1`** (see `docs/AGENT_CONTRACT.md`): one JSON envelope with stable error codes, locations and recovery for `check`, `lint`, `dump`, `doctor` and `prompt` via `--json` (stdout is exactly one line). Argument-parse errors are structured when `--json` is passed. `prompt --json` separates specification defaults (`defaults_applied`) from unspecified creative inputs and reports blockers as `status: blocked`. New exit code 6 (`prompt --strict`). `VCR_AGENT_MODE=1` errors now use the envelope plus the legacy keys.

## v0.1.2 (2026-02-16)

- **Release Readiness & Documentation**
  - README: Added **"VCR for Agents"** section highlighting the JSON error contract and deterministic pipeline.
  - README: Added a high-impact **Copy-Paste High-End Demo** for immediate quickstart.
  - PROJECT_CUSTODIAN: Updated status to "Release-Ready" for v1 release.
  - Version bump to `v0.1.2` across `Cargo.toml` and documentation.
- **Feature-modularized release**
  - Core renderer: 0 optional dependencies
  - Optional: `play` (GUI previewer with hot-reload)
  - Optional: `workflow` (Figma/Frame.io integrations)
  - Reduced default binary size and dependency footprint
- **MCP Server**: Major improvements to `scripts/vcr-mcp-server/`
  - Path resolution: manifest and output paths resolved consistently relative to project root
  - Validation: `validate_vcr_manifest` runs `vcr check` (schema) first, then optional `vcr lint` (unreachable layers)
  - New tools: `vcr_render_frame` (single-frame PNG preview), `vcr_list_examples` (list example manifests)
  - `readOnlyHint` annotations for read-only tools
  - Improved error messages with actionable next steps
  - README: full tool list, recommended workflow, env vars, removed outdated Go reference

## v0.1.1 (2026-02-13)

- **Agent-mode JSON errors**: Stabilized the error contract; `suggested_fix` restored for deterministic cases.
- **Project Structure**: Cleaned up repository root; ephemeral artifacts moved to `renders/` or ignored.
- **Documentation**: Added `CONTRIBUTING.md`, `SECURITY.md`, and `CODE_OF_CONDUCT.md`.
- **GitHub Tools**: Added issue templates for bugs and features; added pull request template.
- **Logging**: Moved runtime logs to a dedicated `logs/` directory.
