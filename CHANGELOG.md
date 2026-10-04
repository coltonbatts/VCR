# Changelog

## Unreleased: agent contract

- **Contract `vcr.agent/1`**: one envelope for every machine-facing operation; stable error codes with location/expected/observed/recovery/retryable; argument-parse errors structured under `--json`. New `--json` on `check`, `lint`, `dump`, `doctor`, `prompt`, `build`, `render-frame`; `render`/`verify`/`explain` keep their legacy top-level keys. New exit code 6 (`prompt --strict`). See docs/AGENT_CONTRACT.md.
- **Discovery**: `vcr capabilities [--schema] --json`; manifest JSON Schema generated from the engine types; expression-function table guarded by a drift test.
- **Preflight** (`explain --json`): resolved backend, runtime probes, software-ignored `post`/`ascii_post` (now rejected on software with `UNSUPPORTED_SOFTWARE_FEATURES`), font fallback detection; `vcr preflight` alias; `--strict`.
- **Inspection**: `vcr inspect` (motion-aware samples, contact sheet, exact per-layer bounds, clipping/timing diagnostics).
- **Verification**: `vcr verify` now checks resolution, exact frame rate, frame count, duration, container, codec/profile, alpha capability and decoded transparency, plus staleness/modification via provenance; fails with exit 3 on mismatch.
- **Delivery**: atomic publish (partial file → verify → rename), stale sidecar invalidation, `*.provenance.json` (engine, ffmpeg, backend, inputs, three hash levels, determinism scope).
- **Fixed**: `vcr preview --scale` rendered a cropped canvas instead of a scaled composition. `vcr doctor` now checks ffprobe.
- **MCP**: rewritten as thin typed wrappers over the CLI contract (explicit engine selection and identity, process-group timeouts, no adapter-side defaults, no mandatory LLM). `vcr_render_plan` lost its override parameters.
- **Docs**: AGENT_QUICKSTART, AGENT_CONTRACT; SKILL.md corrected (software backend rejects shader/post, determinism levels, units); installer no longer deletes a working install.

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
