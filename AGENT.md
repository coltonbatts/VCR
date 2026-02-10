# AGENT.md - VCR for AI-Powered Motion Graphics

This file is for coding agents (Codex, Claude Code, etc.) working inside this repository to turn natural-language briefs into rendered motion graphics.

## What You Are

You are an execution agent for VCR (Video Component Renderer). Your job is to:

1. Understand the user brief
2. Generate or update a `.vcr` manifest
3. Validate and render with the VCR CLI
4. Return the output path and iterate quickly on changes

You are not building a GUI, not inventing new renderer features, and not doing speculative architecture work.

## Repository Context

- `README.md`: User-facing overview and quickstart
- `examples/`: Working manifests to copy patterns from
- `src/main.rs`: CLI commands and flags
- `src/schema.rs`: Manifest schema and expression behavior
- `BUILD_APPLE_SILICON.md`: Apple Silicon performance notes
- `assets/`: Image assets used by manifests
- `exports/`: Final render outputs

## Golden Workflow

### 1) Parse the brief

Extract:
- Text/content
- Visual style
- Motion/timing
- Duration
- Resolution (default to `2560x1440` unless user says otherwise)

If key info is missing, ask one concise question, then proceed.

### 2) Reuse example patterns first

Inspect `examples/` before writing from scratch. Follow existing field names and expression style.

### 3) Create/update a manifest

Place new manifests in `examples/` unless the user requests another path.

Minimum skeleton:

```yaml
version: 1
environment:
  resolution:
    width: 2560
    height: 1440
  fps: 24
  duration:
    frames: 120

params: {}
modulators: {}
groups: []

layers:
  - id: title
    z_index: 1
    pos_x: 320
    pos_y: 648
    opacity: "clamp((t - 20) / 20, 0, 1) * (1.0 - smoothstep(100, 120, t))"
    image:
      path: "../assets/title.png"
```

Important:
- For alpha output, do not add an opaque full-frame background unless requested.
- `image.path` and `source_path` are resolved relative to the manifest file's directory.
- Use `image.path` for new manifests; `source_path` is legacy-compatible.

### 4) Asset handling

VCR composites assets; it does not generate text/images itself.

If required assets are missing:
- Ask for the asset file, or
- Offer quick instructions to create it (Figma/Image editor), then continue once provided.

### 5) Validate

```bash
cargo run --release -- check examples/<name>.vcr
```

Fix all errors before rendering.

### 6) Fast preview (recommended)

```bash
cargo run --release -- preview examples/<name>.vcr --image-sequence -o /tmp/vcr_preview
```

Use this for quick iteration without FFmpeg encode overhead.

### 7) Final render

```bash
cargo run --release -- build examples/<name>.vcr -o exports/<name>.mov
```

Expected result: ProRes 4444 `.mov` with alpha channel.

### 8) Deliver

Return:
- Manifest path
- Output path
- Brief summary of what was rendered
- Render timing (if available)

Then offer one-line iteration options (timing/color/position tweaks).

## Expression Rules (Current Engine)

Expressions are arithmetic/function expressions only. Do not use `if/else` blocks.

Available variables:
- `t` (current frame as float)
- Param names directly (for example `energy`, `fade_speed`) from top-level `params`

Available functions:
- `sin(x)`, `cos(x)`, `abs(x)`
- `clamp(x, min, max)`
- `lerp(a, b, t)`
- `smoothstep(edge0, edge1, x)`
- `easeInOut(x)`
- `noise1d(x)` or `noise1d(x, seed_offset)`
- `env(t)` or `env(t, attack, decay)`

Valid examples:

```yaml
opacity: "clamp((t - 20) / 30, 0, 1)"
pos_x: "320 + sin(t / 12) * 24"
opacity: "smoothstep(10, 30, t) * (1.0 - smoothstep(90, 120, t))"
```

Invalid examples (do not use):

```yaml
# unsupported expression syntax in this engine
opacity: |
  if (t < 30) { 0 } else { 1 }

# params are not accessed with params.<name>
opacity: "params.energy"
```

## Command Reference

```bash
# Validate
cargo run --release -- check <manifest.vcr>

# Diagnostics
cargo run --release -- lint <manifest.vcr>
cargo run --release -- dump <manifest.vcr> --frame 48

# Preview
cargo run --release -- preview <manifest.vcr> --image-sequence -o <folder>

# Single frame
cargo run --release -- render-frame <manifest.vcr> --frame 72 -o frame.png

# Frame range
cargo run --release -- render-frames <manifest.vcr> --start-frame 0 --frames 30 -o <folder>

# Final video
cargo run --release -- build <manifest.vcr> -o exports/output.mov

# Hot reload preview
cargo run --release -- watch <manifest.vcr> --image-sequence -o <folder>
```

## Troubleshooting

- Missing asset path:
  - Verify file exists
  - Verify path is correct relative to manifest location
- Unknown field/schema error:
  - Copy structure from `examples/`
  - Check `src/schema.rs` for accepted fields
- FFmpeg missing:
  - Install FFmpeg (`brew install ffmpeg`)
  - Use `preview --image-sequence` while fixing environment
- No GPU:
  - CPU fallback is expected on unsupported environments
  - Output remains correct, just slower

## Behavioral Rules for Agents

1. Prefer working examples over inventing schema fields.
2. Keep manifests simple first, then layer in motion complexity.
3. Validate before preview; preview before full build.
4. Do not claim assets/fonts exist unless they exist in repo or user provided them.
5. Be explicit about output paths so users can immediately open the render.

## Done Criteria Per Request

- A valid `.vcr` manifest exists
- `check` passes
- Preview or final render command was executed
- Output file path is provided to the user
- User is offered concrete iteration options
