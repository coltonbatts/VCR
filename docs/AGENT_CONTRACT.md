# VCR Agent Contract (`vcr.agent/1`)

The CLI is the single source of truth for normalization, validation, backend planning, rendering and
verification. The MCP server (`scripts/vcr-mcp-server/`) forwards the CLI's documents unchanged. If
the two ever disagree, that is a bug in the adapter.

## Stdout, stderr, exit codes

- With `--json`, **stdout is exactly one JSON document on one line**. Progress and logging go to
  stderr and may be ignored.
- Without `--json`, human output is unchanged.
- Argument-parsing failures are structured too when `--json` appears anywhere in argv.
- `VCR_AGENT_MODE=1` (legacy) still prints error JSON on stderr; it now uses this envelope plus the
  old top-level `error_type` / `summary` / `suggested_fix` / `context` keys.

| Exit | Meaning |
|---|---|
| 0 | ok (including `blocked` unless `--strict`) |
| 2 | usage / argument error, or a request the backend cannot honor (`UNSUPPORTED_SOFTWARE_*`) |
| 3 | manifest, lint, verification or artifact failure |
| 4 | missing runtime dependency (ffmpeg, ffprobe, fonts, GPU) |
| 5 | I/O or encoder failure |
| 6 | blocked: unresolved requirements (`vcr prompt --strict`) |

## Envelope

```jsonc
{
  "contract": "vcr.agent/1",
  "operation": "check",           // check | lint | explain | prompt | doctor | capabilities | dump |
                                  // inspect | render | build | render-frame | verify
  "ok": true,                     // status == "ok"
  "status": "ok",                 // ok | blocked | failed | error
  "engine": {"version": "0.1.2", "git_hash": "0dca420", "build_profile": "release",
             "os": "linux", "arch": "x86_64", "executable": "/…/vcr", "contract": "vcr.agent/1"},
  "result": { },                  // operation-specific data
  "diagnostics": [ ],             // findings (omitted when empty)
  "artifacts": [ ],               // files produced/consumed (omitted when empty)
  "error": { }                    // only when status == "error"
}
```

`status` meanings: **ok** acceptable; **blocked** the operation ran but requirements are unresolved
(never proceed); **failed** the operation ran and its answer is negative (lint findings,
verification mismatch, strict preflight); **error** the operation could not complete.

`engine` is the engine that produced *this document*. When verifying a file, `result.producer` is
the engine recorded in the file's provenance. A verifier's version is not evidence of the producer's.

### Diagnostic

`severity` (`blocker|error|warning|info`), `code`, `stage` (`prompt|check|lint|preflight|inspect|verify|doctor`),
`message`, and when applicable `location {file, layer, field, line, column}`, `expected`, `observed`,
`recovery[]` (guidance only; never permission to change the requester's intent), and `basis`
(`exact` or `approximate`).

### Error

`code` (stable), `category` (`usage|manifest|dependency|backend|asset|io|encoder|artifact|timeout|internal`),
`message`, `summary`, `operation`, `location`, `expected`, `observed`, `recovery[]`, `retryable`
(true only when re-running **unchanged** inputs plausibly succeeds), `exit_code`, `details`.

| Code | Category | Meaning |
|---|---|---|
| `usage.invalid_argument` | usage | bad flag/value/`--set` |
| `manifest.schema_invalid` | manifest | unknown/missing field, wrong type (location has file/field/line) |
| `manifest.validation_failed` | manifest | semantic validation failure |
| `asset.missing` | asset | referenced file absent (location layer + field, `observed` = path) |
| `UNSUPPORTED_SOFTWARE_LAYER_TYPES` | backend | layer needs GPU (`expected` = supported kinds, `observed` = layers) |
| `UNSUPPORTED_SOFTWARE_FEATURES` | backend | `post`/`ascii_post` would be silently ignored on software |
| `backend.unavailable` | backend | requested GPU, none present |
| `dependency.ffmpeg_missing` / `ffprobe_missing` / `font_missing` / `missing` | dependency | tool/asset missing |
| `encoder.failed` | encoder | ffmpeg failure |
| `artifact.nonconformant` / `artifact.unreadable` | artifact | encoded output fails its contract / cannot be read |
| `io.read_failed` / `io.write_failed` | io | filesystem |
| `timeout.exceeded` | timeout | adapter-side; process group terminated, nothing published |
| `engine.incompatible` / `engine.protocol_error` | dependency / internal | adapter-side engine problems |

Pre-contract codes (`UNSUPPORTED_SOFTWARE_*`, `invalid_aspect_preset`) are preserved verbatim.

## Operations

| Operation | Command | Stage question |
|---|---|---|
| discover | `vcr capabilities --json [--schema]` | what can this installation do, and what is usable here? |
| normalize | `vcr prompt --json [--strict]` | what does the brief specify, and what is unresolved? |
| validate | `vcr check --json` | is the scene structurally/semantically valid? |
| lint | `vcr lint --json` | likely visibility/timing problems? |
| preflight | `vcr explain --json [--strict]` (alias `preflight`) | can the requested backend render it on this machine? (`result.backend_preflight.ready`) |
| inspect | `vcr inspect --json` | what does the motion actually look like; any mechanical defects? |
| render | `vcr render --json` / `vcr build --json` | produce the artifact (atomic) |
| verify | `vcr verify FILE --json` | does the encoded media satisfy the contract? |

Do not treat any stage as proof of another: normalization does not prove a scene exists; `check`
does not prove a backend can run it or assets render; preflight does not judge design; verification
does not judge design either.

### Normalization

`vcr prompt --json` returns `normalized_spec`, `defaults_applied` (structured **specification**
defaults only: resolution 1920x1080, fps 60, seed 0, output path, …), `unknowns_and_fixes`,
`assumptions_applied` (prose), `acceptance_checks`, and `creative_inputs` (heuristic status of text,
palette, typeface and assets in the brief). **Creative content is never defaulted.** Unresolved
items make `status: "blocked"` with `blocker` diagnostics.

### Inspect

Samples are chosen from the evaluated layer-state timeline: first/last frame plus start/mid/end of
every motion or still run, topped up with evenly spaced frames. Each sample has the exact `frame`,
`time_seconds`, `time_rational` (`frame/fps`), `phase` (`entrance|hold|exit|ending|…`), evaluated
layer state, exact per-layer pixel bounds (each layer rendered alone), and whole-frame pixel stats.
Diagnostics: `layout.touches_canvas_edge` (within 0.5% / ≥4 px of an edge; a warning at rest, info
during motion), `layout.outside_safe_area` (info, hold phase only), `layer.renders_nothing`,
`timing.motion_on_last_frame`, `timing.hold_short` (approximate), `timing.leading_empty` /
`ends_empty` / `nothing_visible`, `text.small` (approximate). Judgment calls stay with the reviewer
looking at `contact_sheet.png`.

### Render and verify

`render`/`build` encode to a hidden `.<stem>.partial-<pid>.<ext>`, check conformance on that file
(container, codec/profile, resolution, exact frame rate, packet count, duration, alpha capability,
colour tags), then rename it into place. Sidecars describing a previous artifact at the same path are
removed first. Written files: `<out>`, `<out>.metadata.json` (scene record, deterministic,
machine-independent), `<out>.provenance.json` (execution record, written last).

`vcr verify FILE` checks against expectations from, in increasing precedence: the recorded
provenance, `--manifest` (which also detects **stale** output), explicit `--expect-*` flags.
`--expect-transparency required|none|any` is measured on decoded RGBA, not inferred from `pix_fmt`.
Checks: container, codec, profile, resolution, `frame_rate` (exact rational), `frame_count` (demuxed
packets), `duration_seconds` (±½ frame), `alpha_capable`, `transparency`; plus provenance integrity
(`verify.modified_since_render`, `verify.incomplete_export`, `verify.stale_artifact`,
`verify.no_provenance`) and `verify.unreadable_media`. Limits are listed in `result.media.limits`.

## Determinism scope

| Level | Where recorded | Guarantee |
|---|---|---|
| Scene and settings | `manifest_hash`, `resolved_manifest_hash` | reproducible |
| Raster frames (pre-encode) | `hashes.raster_frames_sha256` | software backend: identical for the same engine build; GPU: not guaranteed across hardware/drivers |
| Decoded frames | `hashes.decoded_frames_sha256` | follows the encoded bytes |
| Encoded file bytes | `hashes.output_file_sha256` | same ffmpeg build + args only (`toolchain.ffmpeg`) |

## Compatibility and migration

Within `vcr.agent/1` fields are only added. Pre-contract consumers keep working:

- `render --json`: the six legacy top-level keys (`manifest, backend, frame_count, frame_hash,
  output_hash, duration_ms`) remain at the top level *and* in `result`. Deprecated top level; will
  move in contract 2.
- `verify --json`: legacy `file_path, hash, tool_version, backend` remain at top level. `tool_version`
  is the verifier. `backend` is now filled from provenance when present.
- `explain --json`: legacy keys remain at top level; preflight fields were added to
  `backend_preflight` (`resolved_backend, gpu_available, ready, incompatibilities, runtime, checks`).
- `params --json` is unchanged (not enveloped).
- `vcr verify` now **fails (exit 3)** on mismatches; previously it only printed a hash.
- Software renders of manifests with `post:` / enabled `ascii_post:` now fail with
  `UNSUPPORTED_SOFTWARE_FEATURES` (they were silently ignored).
- `vcr preview --scale` now renders the full composition and downsamples; it previously drew layers
  at original pixel coordinates into a smaller canvas (a cropped view).
- `vcr doctor` also checks `ffprobe`.
- New exit code 6 (only with `prompt --strict`).
- MCP: `vcr_render_plan` no longer takes resolution/fps/duration/alpha/backend overrides (the engine
  owns those); the engine is selected explicitly (`engine_path` → `VCR_BIN` → checkout build → PATH)
  and its identity is reported in every response's `adapter` block.
