# Changelog

## v0.1.2 (2026-02-14)

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
