# VCR MCP Server

Exposes VCR's broadcast-quality video rendering as MCP tools for AI agents.

## Tools

| Tool | Description |
|------|-------------|
| `vcr_doctor` | System health check (binary, FFmpeg, GPU) |
| `lint_vcr_manifest` | Validate a VCR YAML manifest |
| `render_video_from_prompt` | Generate .mov video from a natural language prompt |

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
- **Go**: Required for the `render_video_from_prompt` tool (runs `skills/video-gen`)
- **FFmpeg**: Required by VCR for ProRes encoding
- **LM Studio**: Running on `127.0.0.1:1234` for prompt-based generation
