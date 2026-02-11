# VCR Agentic Skill Protocol (v1)

This document defines how the **VCR Hub** (TUI) and external **Skills** (Agents) communicate via standard I/O.

## Transport

- **Input**: Command-line arguments.
- **Output**: JSON-RPC-like objects printed to `stdout`, one per line.
- **Errors/Logs**: Plain text printed to `stderr` (captured by the Hub for debugging).

## Message Schema (JSON)

Every message must have a `type` field.

### 1. Status Update

Used to inform the user of what the skill is doing (e.g., "Reading DB", "Thinking").

```json
{ "type": "status", "status": "Consulting local brain..." }
```

### 2. Progress Update

Used to drive UI progress bars.

```json
{ "type": "progress", "percent": 0.45, "status": "Rendering 54/120" }
```

### 3. Artifact Notification

Used when a final asset is generated.

```json
{ "type": "artifact", "path": "renders/output.mov", "status": "Render Complete" }
```

### 4. Error

Used for fatal failures.

```json
{ "type": "error", "message": "LM Studio not found.", "code": 404 }
```

## Best Practices

1. **Silence Stdout**: Skills must NEVER print plain text to `stdout`. All logs should go to `stderr`.
2. **Atomic Artifacts**: Do not emit `artifact` until the file is fully written and closed.
3. **Graceful Exit**: Exit with code `0` on success, and non-zero on error after emitting an `error` message.
