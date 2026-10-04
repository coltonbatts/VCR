# Changelog

## Unreleased

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
