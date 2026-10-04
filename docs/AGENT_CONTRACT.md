# VCR Agent Contract (`vcr.agent/1`)

One machine-readable shape for operations an agent depends on, so it never has to scrape human
text. This document covers what exists today; operations not listed here keep their current output.

## Stdout, stderr, exit codes

- With `--json`, **stdout is exactly one JSON document on one line**. Progress and logging go to
  stderr and may be ignored.
- Without `--json`, human output is unchanged.
- Argument-parsing failures are structured too when `--json` appears anywhere in argv.
- `VCR_AGENT_MODE=1` (legacy) still prints error JSON on stderr, now in this envelope plus the old
  top-level `error_type` / `summary` / `suggested_fix` / `context` keys (pretty-printed, as before).

| Exit | Meaning |
|---|---|
| 0 | ok (including `blocked`, unless `--strict`) |
| 2 | usage / argument error |
| 3 | manifest or lint failure |
| 4 | missing runtime dependency |
| 5 | I/O error |
| 6 | blocked: unresolved requirements (`vcr prompt --strict` only) |

## Envelope

```jsonc
{
  "contract": "vcr.agent/1",
  "operation": "check",          // check | lint | dump | doctor | prompt | explain | capabilities | inspect | render | build | render-frame | verify
  "ok": true,                    // status == "ok"
  "status": "ok",                // ok | blocked | failed | error
  "engine": {"version": "0.1.2", "git_hash": "…", "build_profile": "release", "os": "linux", "arch": "x86_64", "executable": "/…/vcr", "contract": "vcr.agent/1"},
  "result": { },                 // operation-specific
  "diagnostics": [ ],            // omitted when empty
  "artifacts": [ ],              // omitted when empty
  "error": { }                   // only when status == "error"
}
```

- **ok**: acceptable. **blocked**: ran, but requirements are unresolved; do not proceed.
  **failed**: ran, and the answer is negative (lint findings, missing dependency in `doctor`).
  **error**: the operation could not complete.
- **Diagnostic**: `severity` (`blocker|error|warning|info`), `code`, `stage`, `message`, and when
  applicable `location {file, layer, field, line, column}`, `expected`, `observed`, `recovery[]`
  (guidance only; never permission to change the requester's intent), `basis` (`exact|approximate`).
- **Error**: `code` (stable), `category`, `message`, `summary`, `operation`, `location`, `expected`,
  `observed`, `recovery[]`, `retryable` (true only when re-running *unchanged* inputs plausibly
  succeeds), `exit_code`, `details`. `code`/`message`/`details` are the pre-contract fields, unchanged.

| Error code | Category | Meaning |
|---|---|---|
| `usage.invalid_argument` | usage | bad flag/value/`--set` |
| `manifest.schema_invalid` | manifest | unknown/missing field, wrong type (location: file, field, line) |
| `manifest.validation_failed` | manifest | semantic validation failure |
| `asset.missing` | asset | referenced file absent (location: layer + field; `observed` = authored path) |
| `dependency.ffmpeg_missing` / `ffprobe_missing` / `font_missing` / `missing` | dependency | tool or bundled asset missing |
| `backend.unavailable` | backend | GPU requested, none present |
| `UNSUPPORTED_SOFTWARE_LAYER_TYPES` | backend | a layer needs the GPU (`expected` = software-supported kinds, `observed` = layers) |
| `UNSUPPORTED_SOFTWARE_FEATURES` | backend | `post:` / enabled `ascii_post:` would be silently ignored on software |
| `artifact.nonconformant` / `artifact.unreadable` | artifact | encoded output fails its contract / cannot be read as media |
| `encoder.failed` | encoder | ffmpeg failure |
| `io.read_failed` / `io.write_failed` | io | filesystem |

Existing coded errors (e.g. `invalid_aspect_preset`) keep their code verbatim.

## Operations

| Command | Notes |
|---|---|
| `vcr check M --json` | `result`: resolved environment (size, fps, frames, exact `frames/fps` duration), layers, params. Structure and semantics only: it does **not** prove a backend can render the scene or that assets render. |
| `vcr lint M --json` | Findings are `diagnostics` (`lint.unreachable_layer`, `lint.alpha_blocked`), `status: failed`, exit 3. |
| `vcr dump M --frame N --json` | Evaluated layer state at an exact frame; `time_rational` = `frame/fps`. |
| `vcr doctor --json` | Runtime probes: ffmpeg, ffprobe, fonts, GPU. Missing dependency → `status: failed`, exit 4. |
| `vcr explain M --json [--strict]` (alias `vcr preflight`) | **Backend preflight.** `result.backend_preflight`: `requested_backend`, `resolved_backend` (null = cannot run here), `gpu_available`, `ready`, `incompatibilities` (layers and the manifest features `post`/`ascii_post` the software backend cannot honor), `runtime` (ffmpeg, ffprobe, fonts, GPU probes), `checks` (diagnostics: `backend.software_unsupported_layer`, `backend.software_ignores_feature`, `backend.gpu_unavailable`, `runtime.*_missing`, `text.font_family_fallback`) and the legacy `blockers`. Describes by default (exit 0); `--strict` makes `ready: false` a `failed` result (exit 3). Legacy top-level keys are preserved. |
| `vcr capabilities --json [--schema]` | **Discovery from the installed engine.** Engine identity, contract and manifest versions, compiled features, runtime probes, supported layer kinds with backend requirements (`compiled_backends` vs `usable_here`: compiled support is not the same as usable on this machine), procedural kinds, post shaders, encoding profiles, fonts, expression functions, and the **time units** (expressions/keyframes use frames; layer `start_time`/`end_time`/`time_offset` use seconds; `start_time` does not shift keyframes). `--schema` adds the manifest JSON Schema generated from the engine's own types. Lists are derived from the engine's definitions, with tests that fail on drift. |
| `vcr inspect M --json [-o DIR] [--samples N] [--set …]` | **Temporal evidence.** Samples the evaluated layer-state timeline (first/last frame plus start/mid/end of every motion or still run, topped up with evenly spaced frames) and writes `sample_<frame>.png`, `contact_sheet.png` (alpha checkerboard, labelled with frame and time) and `inspection.json` under `DIR` (default `renders/<stem>_inspect`). Each sample has the exact `frame`, `time_seconds`, `time_rational` (`frame/fps`), `phase` (`entrance|hold|exit|ending|…`), evaluated layer state, exact per-layer pixel bounds (each layer rendered alone) and whole-frame pixel stats. |
| `vcr render M -o OUT --json` / `vcr build …` | **Atomic publish.** Encodes to a hidden `.<stem>.partial-<pid>.<ext>`, checks it (container, codec/profile, resolution, exact frame rate, packet count, duration, alpha capability, colour tags), then renames it into place. A failed or interrupted render never leaves a half-written file at `OUT`, and a previous good file is not touched. Sidecars describing an earlier artifact at `OUT` are removed first, so an old file cannot pass as new. Writes `OUT.metadata.json` (scene record: deterministic, no machine-specific data, byte-identical for identical effective inputs) and `OUT.provenance.json` (execution record, written last; its presence with a matching output hash marks a completed export). Stale partial files of the same output are swept on the next render. The envelope's `artifacts` list `output`, `metadata`, `provenance`. |
| `vcr render-frame M --frame N --json` | `artifacts`: the PNG (with exact `frame`/`time_seconds`) and its metadata sidecar. |
| `vcr verify FILE [--manifest M] [--set …] [--expect-* …] --json` | **Encoded-media verification.** Expectations come from, in increasing precedence: the file's provenance, `--manifest` (which also detects **stale** output), explicit flags (`--expect-width/height/fps/frames/container/codec/profile/alpha-capable`, `--expect-transparency required|none|any`). Checks: `container`, `codec`, `profile`, `resolution`, `frame_rate` (exact rational), `frame_count` (demuxed packets), `duration_seconds` (±½ frame), `alpha_capable`, `transparency` (**measured on decoded RGBA**, never inferred from `pix_fmt`: an alpha-capable file may still be fully opaque, and an opaque result is correct when opacity is intended). Provenance integrity: `verify.modified_since_render`, `verify.incomplete_export`, `verify.stale_artifact`, `verify.no_provenance` (warning unless `--require-provenance`), `verify.unreadable_media`. Mismatch → `status: failed`, exit 3. `result.producer` is the engine recorded in provenance; the top-level `engine` is only the verifier, and a verifier's version is not evidence of the producer's. `result.media.limits` lists what was and wasn't checked. |
| `vcr prompt --json [--strict]` | Prompt gate. `result.defaults_applied` lists **specification** defaults only (resolution, fps, seed, output). `result.creative_inputs` reports whether text, palette, typeface and assets were specified; creative content is never defaulted. Unresolved items → `status: blocked` (exit 0, or 6 with `--strict`). |

### Inspect diagnostics

`exact` unless marked: `layout.touches_canvas_edge` (content within 0.5% / ≥4 px of an edge: warning at rest, info during motion), `layout.outside_safe_area` (info, hold phase only), `layer.renders_nothing`, `timing.motion_on_last_frame`, `timing.leading_empty` / `ends_empty` / `nothing_visible`, `timing.hold_short` (approximate), `text.small` (approximate). Inspection reports mechanical facts; whether the design is readable, balanced or on-brief is a judgment for whoever looks at `contact_sheet.png`. Revise by editing the layer with the reported stable `id`, or by changing declared params with `--set`, then inspect again.

## Determinism scope

| Level | Where recorded | Guarantee |
|---|---|---|
| Scene and settings | `manifest_hash`, `resolved_manifest_hash` | reproducible |
| Raster frames (pre-encode) | `provenance.hashes.raster_frames_sha256` | software backend: identical for the same engine build; GPU: not guaranteed across hardware or drivers |
| Decoded frames | `provenance.hashes.decoded_frames_sha256` | follows the encoded bytes |
| Encoded file bytes | `provenance.hashes.output_file_sha256` | same ffmpeg build and arguments only (`toolchain.ffmpeg`) |

Compare `engine.*`, `toolchain.ffmpeg` and `backend` in provenance before treating a hash difference as a defect. Observed: the same raster frames encoded by ffmpeg 6.1.1 (Linux) and the macOS build behind the repo's golden hash differ at the file level only.

`render`, `verify` and `params` keep their existing `--json` output. Their *errors* now use
the envelope when `--json` is passed.

## Compatibility

- `render --json`: the six legacy top-level keys (`manifest, backend, frame_count, frame_hash, output_hash, duration_ms`) remain at the top level *and* in `result`. The top-level copies are deprecated and will move in contract 2.
- `verify --json`: legacy `file_path, hash, tool_version, backend` remain at the top level (`tool_version` is the verifier; `backend` is filled from provenance when present).
- **`vcr verify` now fails (exit 3) on mismatch** (it used to print only a hash). With no expectations and no provenance it reports file integrity and probe facts and says so.
- Renders no longer write directly to the output path: they publish atomically and also write `.provenance.json`. Anything globbing the output directory should ignore hidden `.*.partial-*` files.

- **Software backend is now strict.** A manifest with `post:` effects or an enabled `ascii_post:` fails on the software backend with `UNSUPPORTED_SOFTWARE_FEATURES` (exit 2); it used to render with those silently ignored. This also applies when `auto` falls back to software for lack of a GPU. Unsupported *layers* were already rejected.
- `vcr doctor` now also checks `ffprobe` (every build runs it).

- Exit codes are unchanged except the new `6`, which only `prompt --strict` returns.
- `VCR_AGENT_MODE=1` consumers keep their keys; the document also gains the envelope fields.
- The exit-code classification that used to live in `main.rs` as message matching now lives in one
  place (`agent_contract`), with the same mapping.
