# VCR MCP Server

Typed MCP access to the **installed VCR engine**. Every tool forwards the engine's `--json`
document (contract `vcr.agent/1`, see [docs/AGENT_CONTRACT.md](../../docs/AGENT_CONTRACT.md)) and adds
an `adapter` block: selected executable, how it was chosen, engine identity, warnings, argv, elapsed
time. The adapter never normalizes, validates, plans or invents scene settings; the engine does,
so CLI and MCP produce identical settings, diagnostics and verified outputs.

The calling agent authors and revises manifests itself. No tool requires a second LLM.

## Tools

| Tool | Engine call | Purpose |
|---|---|---|
| `vcr_capabilities` | `capabilities --json [--schema]` | what this installation can do and what is usable here |
| `vcr_doctor` | `doctor --json` | ffmpeg / ffprobe / fonts / GPU |
| `vcr_normalize_brief` | `prompt --json [--strict]` | prompt gate; `status: blocked` = stop and ask |
| `vcr_render_plan` | `prompt --json` | normalized spec + ordered steps (executes nothing; no adapter defaults) |
| `vcr_validate` | `check` → `lint` → `explain` | three separate stages, first failing stage reported |
| `vcr_inspect` | `inspect --json` | sampled frames, contact sheet, bounds/timing diagnostics |
| `vcr_render_frame` | `render-frame --json` | one full-resolution frame |
| `vcr_render` | `render --json` then `verify` | atomic render + verification |
| `vcr_verify` | `verify --json` | encoded-media contract, staleness, provenance |
| `vcr_execute_plan` | validate → render → verify | back-compat convenience |
| `vcr_list_examples` | – | example manifests |
| `validate_vcr_manifest`, `lint_vcr_manifest` | – | back-compat aliases of `vcr_validate` |
| `vcr_synthesize_manifest`, `render_video_from_prompt` | – | OPTIONAL LLM drafting; gated by normalization + validation |

## Context is explicit

- `project_root` (default: this checkout, or `VCR_PROJECT_ROOT`) is the subprocess working directory.
  Manifest and output paths are relative to it; escapes (`..`, absolute) are refused. Asset paths
  inside a manifest resolve relative to the manifest file.
- Inline YAML (`manifest_yaml`) is saved to `save_as` (default `.vcr_mcp/<hash>.vcr`), a real path,
  because assets resolve from the manifest's directory.
- Param overrides: `overrides={"name": value}` → `--set name=value` (colors `#RRGGBBAA`).

## Engine selection

Order: `engine_path` argument → `VCR_BIN` → checkout build (`target/release|debug/vcr`, newest) →
`PATH`. The checkout build precedes `PATH` so an older installed binary cannot silently become the
engine. Each candidate is probed with `capabilities --json`; an engine that does not speak
`vcr.agent/1` is rejected (`engine.incompatible`) before any operation. If another usable engine
has a different build, a warning names both. Identity is in every response (`adapter.engine_identity`).

## Failure handling

Timeouts terminate the engine's whole process group (including ffmpeg) and return
`timeout.exceeded` (retryable). The engine publishes atomically, so nothing half-written is
delivered; the killed run's hidden `.partial-<pid>` file is removed and stale ones are swept by the
next render. A non-contract stdout is reported as `engine.protocol_error`.

## Setup

```bash
cargo build --release            # engine
cd scripts/vcr-mcp-server
pip install -e .                 # or: uv pip install -e .
python3 -m unittest discover -s tests -v
```

Claude Desktop / Claude Code:

```json
{ "mcpServers": { "vcr": { "command": "python3", "args": ["/absolute/path/to/VCR/scripts/vcr-mcp-server/server.py"] } } }
```

Optional LLM synthesis uses `VCR_LLM_ENDPOINT` (default `http://127.0.0.1:1234/v1`), `VCR_LLM_MODEL`,
`VCR_LLM_API_KEY`.

## Migration from 0.1

`vcr_render_plan` no longer accepts `resolution/fps/duration/alpha/backend` (the planner used to
default to 24 fps / 5 s / alpha=false while the engine defaults to 60 fps and requires a duration;
put those in the brief). Tools return structured documents, not prose. Absolute output paths were
previously passed to the engine, which rejects them; outputs are now project-relative.
