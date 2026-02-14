# VCR MCP Server

Exposes VCR's broadcast-quality video rendering as MCP tools for AI agents.

## Tools

| Tool | Description |
|------|-------------|
| `vcr_doctor` | System health check (binary, FFmpeg, GPU) |
| `validate_vcr_manifest` | Validate YAML: schema (vcr check) + unreachable layers (vcr lint) |
| `lint_vcr_manifest` | Alias for validate_vcr_manifest |
| `vcr_render_frame` | Render a single frame to PNG (fast preview) |
| `vcr_list_examples` | List example manifests in examples/ |
| `vcr_render_plan` | Plan a render (returns JSON with CLI commands, does not execute) |
| `vcr_synthesize_manifest` | Generate manifest from prompt via LLM, validate with vcr check |
| `vcr_execute_plan` | Validate and render a manifest to ProRes video |
| `render_video_from_prompt` | Full pipeline: context → LLM → manifest → render |

## Recommended Workflow

1. **Check system**: `vcr_doctor` — verify binary, FFmpeg, GPU
2. **Create manifest**: `vcr_synthesize_manifest` — prompt → YAML (or write manually)
3. **Preview**: `vcr_render_frame` — single-frame PNG to verify before full render
4. **Render**: `vcr_execute_plan` — validate + build to .mov

For one-shot generation: `render_video_from_prompt` does steps 2–4 in one call.

## Environment Variables (LLM-based tools)

| Variable | Default | Description |
|----------|---------|-------------|
| `VCR_LLM_ENDPOINT` | `http://127.0.0.1:1234/v1` | OpenAI-compatible API base URL |
| `VCR_LLM_MODEL` | (auto from /models) | Model ID (e.g. `llama3`, `gpt-4`) |
| `VCR_LLM_API_KEY` | (empty) | Bearer token (omit for local models like LM Studio) |

## Setup

```bash
cd scripts/vcr-mcp-server
pip install -e .
# or: uv pip install -e .
```

## Claude Desktop / Claude Code

Add to your MCP config (`~/.claude/claude_desktop_config.json` or settings):

```json
{
  "mcpServers": {
    "vcr": {
      "command": "python",
      "args": ["/absolute/path/to/VCR/scripts/vcr-mcp-server/server.py"]
    }
  }
}
```

Or with `uv` (no install needed):

```json
{
  "mcpServers": {
    "vcr": {
      "command": "uv",
      "args": ["run", "--directory", "/absolute/path/to/VCR/scripts/vcr-mcp-server", "server.py"]
    }
  }
}
```

## Cursor

Add to `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "vcr": {
      "command": "python",
      "args": ["/absolute/path/to/VCR/scripts/vcr-mcp-server/server.py"]
    }
  }
}
```

## Prerequisites

- **VCR binary**: `cargo build` in the project root (or `vcr` on PATH)
- **FFmpeg**: Required by VCR for ProRes encoding
- **LLM provider** (for prompt-based tools): LM Studio, OpenAI, or any OpenAI-compatible API on `VCR_LLM_ENDPOINT`
