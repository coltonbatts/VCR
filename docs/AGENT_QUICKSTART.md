# Agent Quickstart

**discover → normalize → author → validate/preflight → inspect → revise → render → verify**

Every step prints one JSON document with `--json`; `status` is `ok | blocked | failed | error`.
Reference: [AGENT_CONTRACT.md](AGENT_CONTRACT.md). MCP tool names are in parentheses.

```bash
# 1. DISCOVER what this installed engine can do (fields, layers, fonts, codecs, time units, GPU/ffmpeg)
vcr capabilities --json [--schema]                               # (vcr_capabilities)

# 2. NORMALIZE the brief. Stop on status "blocked": ask the requester, never invent.
vcr prompt --json --text "5s alpha lower third 1920x1080 60fps ./renders/lt.mov"   # (vcr_normalize_brief)
#    Specification defaults are listed in result.defaults_applied. Text, palette, typeface and
#    assets are never defaulted: result.creative_inputs says which are unspecified.

# 3. AUTHOR the manifest yourself from result.normalized_spec (see "Traps" below).

# 4. VALIDATE → LINT → PREFLIGHT, in that order; fix the first failing stage.
vcr check scene.vcr --json                                       # (vcr_validate runs all three)
vcr lint scene.vcr --json
vcr explain scene.vcr --json --backend software                  # ready? resolved backend, blockers

# 5. INSPECT the motion before an expensive render. Look at contact_sheet.png too.
vcr inspect scene.vcr --json -o renders/scene_inspect --backend software   # (vcr_inspect)

# 6. REVISE one element: edit that layer by its stable id, or change a declared param.
vcr inspect scene.vcr --json --set slide_distance=120 -o renders/scene_inspect

# 7. RENDER (atomic: nothing is published unless it conforms) and 8. VERIFY the encoded file.
vcr render scene.vcr -o renders/scene.mov --backend software --json            # (vcr_render)
vcr verify renders/scene.mov --manifest scene.vcr --expect-transparency required --json   # (vcr_verify)
```

Done means `verify.status == "ok"` with the expectations you actually care about. A render alone is
not delivery. `*.provenance.json` records producer engine, ffmpeg, backend, hashes and inputs.

## Reading failures

`error.code` / `diagnostics[].code` are stable. `error.location` points at file/layer/field.
`error.recovery` is guidance. Do not change the requester's intent to make an error go away.
`error.retryable` is true only for transient failures; otherwise change an input.

## Traps (all discoverable from `vcr capabilities --json` → `time`, `procedural_geometry`)

- Expressions and keyframes use **frames** (`t` = frame index). Layer `start_time`/`end_time`/`time_offset` use **seconds**.
- Procedural shape geometry is **normalized 0–1**, not pixels. Pixel values draw nothing (`layer.renders_nothing`).
- `check` passing does not mean the backend can render it: shader/video/lottie layers and `post:` need the GPU.
- Unknown `font_family` silently becomes GeistPixel-Line (`text.font_family_fallback`).
- Text does not wrap; long copy hits the canvas edge (`layout.touches_canvas_edge`).
- Params are float/int/bool/color/vec2: **no string params**, so text content is part of the manifest.
- Software backend is the reproducible default. GPU output is not bit-identical across hardware.

## Packs

When a request references `packs/<id>/`, build the contact sheet first:
`scripts/pack_contact_sheet.sh --pack packs/<id> --out renders/<id>/contact_sheet.png --index-out renders/<id>/contact_sheet.index.tsv`.

## MCP

`python3 scripts/vcr-mcp-server/server.py` (see its README). Same documents, plus an `adapter` block
with the selected executable and identity. Pass `project_root`; outputs are relative to it.
