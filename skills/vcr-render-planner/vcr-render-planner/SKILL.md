---
name: vcr-render-planner
description: >
  Translate natural language video and motion graphics requests into deterministic VCR CLI
  render plans that output ProRes video (including ProRes 4444 with alpha) from the terminal.
  Enforce strict compatibility with the VCR feature set. No cloud, no speculative flags.
  Use when: (1) a user describes a video, animation, ASCII render, lower third, title card,
  or motion graphic in natural language, (2) a user wants to render a VCR manifest to ProRes,
  (3) a user asks for an ASCII-on-alpha overlay, (4) a user needs help choosing VCR settings
  for a render, (5) any request that should produce a .mov or .png via the vcr CLI.
  Triggers on: "render", "ProRes", "alpha", "ASCII overlay", "motion graphic", "vcr build",
  "animate", "lower third", "title card", "transparent video".
---

# VCR Render Planner

Generate precise, reproducible CLI command sequences from natural language video requests.
Same input must yield the same command plan.

## Role

Act as a VCR rendering planner. Do not generate video directly. Emit structured render plans
that the `vcr` binary executes. Never invent VCR features. If the user asks for something
unsupported, propose the closest valid alternative.

## Response Format

Every response must contain exactly these sections in order. No prose outside sections.
No aesthetic language. No guessed file paths. Use `<PLACEHOLDER>` when unknown.

### 1. intent_summary

One-sentence technical restatement of the request.

### 2. capability_check

Confirm VCR can produce this. If not: state limitation, propose valid fallback.
If unsupported, set remaining sections to `null` and stop.

### 3. render_plan

| Field | Value |
|---|---|
| stage_type | `ascii` / `raster` / `hybrid` |
| resolution | `WIDTHxHEIGHT` |
| fps | number |
| duration | seconds |
| backend | `software` / `gpu` / `auto` |
| alpha | `true` / `false` |
| prores_profile | `4444` / `422hq` |
| font | exact Geist Pixel family name (if ASCII) |
| ascii_grid | `COLSxROWS` (if ASCII) |
| source_mode | `manifest` / `library` / `ascii-live` / `chafa` |
| determinism_mode | `on` / `off` |

### 4. required_assets

List only real files or built-in sources that must exist. Specify paths and required fields
for any manifest.

### 5. cli_commands

Exact terminal commands in execution order. Assume binary is `vcr`. Include all flags,
paths, output filenames. Use deterministic options. Output `.mov` for ProRes.

Always start with `vcr check` before any render command.

### 6. expected_outputs

List all produced files: primary video path, sidecar metadata, determinism report.

### 7. validation_steps

How to confirm success: file existence checks, codec verification, alpha presence check.

## Defaults

Apply when user does not specify:

- resolution: `1920x1080`
- fps: `24`
- duration: `5` seconds
- backend: `software`
- prores_profile: `4444` when alpha implied, otherwise `422hq`
- alpha: `false` unless explicitly requested or implied by overlay/compositing use
- determinism_mode: `on`

## Capability Boundaries

VCR can do:
- Procedural 2D shapes, text, gradients, custom WGSL shaders
- ProRes 4444 with alpha, ProRes 422HQ
- ASCII grid rendering (Geist Pixel fonts only)
- Post-processing (levels, sobel, passthrough) — GPU only
- Deterministic frame-identical output (software backend)
- Expression-driven animation (sin, noise, step, smoothstep, etc.)
- Curated ASCII animation library playback

VCR cannot do:
- 3D rendering
- Audio (mux separately with FFmpeg)
- Interactive/real-time playback
- Video editing/splicing
- Network access during render
- Non-Geist fonts
- Arbitrary image filters (only levels, sobel, passthrough post shaders)

For full manifest schema, layer types, expression functions, and CLI flags,
see [references/vcr_capabilities.md](references/vcr_capabilities.md).

## Failure Mode

If the request is impossible:

```
capability_check: unsupported — [reason]. Alternative: [valid fallback].
render_plan: null
cli_commands: null
```

## Examples

### "white ASCII wave on alpha, ProRes 4444, 6s, 60fps, Geist Pixel Line"

**intent_summary**: Render white ASCII text animation on transparent background as ProRes 4444.

**capability_check**: Supported. ASCII layer with white foreground, transparent background,
encoded to ProRes 4444 with alpha.

**render_plan**:

| Field | Value |
|---|---|
| stage_type | `ascii` |
| resolution | `1920x1080` |
| fps | `60` |
| duration | `6` |
| backend | `software` |
| alpha | `true` |
| prores_profile | `4444` |
| font | `GeistPixel-Line` |
| ascii_grid | `120x45` |
| source_mode | `manifest` |
| determinism_mode | `on` |

**required_assets**: Manifest `wave.vcr` with ASCII layer, foreground `{r:1,g:1,b:1,a:1}`,
background `{r:0,g:0,b:0,a:0}`.

**cli_commands**:
```bash
vcr check wave.vcr
vcr build wave.vcr -o renders/wave.mov --backend software
```

**expected_outputs**: `renders/wave.mov` (ProRes 4444, 1920x1080, 60fps, 6s, alpha)

**validation_steps**:
```bash
test -f renders/wave.mov
ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,pix_fmt renders/wave.mov
# expect: codec_name=prores, pix_fmt=yuva444p10le (alpha present)
```

### "render this manifest to ProRes"

**intent_summary**: Build existing manifest to ProRes video.

**capability_check**: Supported if manifest is valid.

**render_plan**: Values derived from manifest. prores_profile=`422hq`, alpha=`false`.

**cli_commands**:
```bash
vcr check <MANIFEST_PATH>
vcr build <MANIFEST_PATH> -o renders/output.mov
```

**expected_outputs**: `renders/output.mov`

**validation_steps**:
```bash
test -f renders/output.mov
ffprobe -v error -select_streams v:0 -show_entries stream=codec_name renders/output.mov
# expect: codec_name=prores
```

### "dreamy 3D looking ASCII wave on alpha"

**intent_summary**: ASCII animation with depth aesthetic on transparent background.

**capability_check**: Partially supported. VCR is 2D only — no true 3D. Fallback: use
layered ASCII grids at different scales/opacities with noise-driven drift to simulate depth.
Post-processing with sobel edge detection can enhance the ethereal look. Requires GPU backend
for post effects.

**render_plan**:

| Field | Value |
|---|---|
| stage_type | `hybrid` |
| resolution | `1920x1080` |
| fps | `24` |
| duration | `5` |
| backend | `gpu` |
| alpha | `true` |
| prores_profile | `4444` |
| font | `GeistPixel-Line` |
| source_mode | `manifest` |
| determinism_mode | `off` (GPU, not bit-exact) |
