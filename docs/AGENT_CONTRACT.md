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
  "operation": "check",          // check | lint | dump | doctor | prompt | explain
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
| `vcr prompt --json [--strict]` | Prompt gate. `result.defaults_applied` lists **specification** defaults only (resolution, fps, seed, output). `result.creative_inputs` reports whether text, palette, typeface and assets were specified; creative content is never defaulted. Unresolved items → `status: blocked` (exit 0, or 6 with `--strict`). |

`render`, `verify` and `params` keep their existing `--json` output. Their *errors* now use
the envelope when `--json` is passed.

## Compatibility

- **Software backend is now strict.** A manifest with `post:` effects or an enabled `ascii_post:` fails on the software backend with `UNSUPPORTED_SOFTWARE_FEATURES` (exit 2); it used to render with those silently ignored. This also applies when `auto` falls back to software for lack of a GPU. Unsupported *layers* were already rejected.
- `vcr doctor` now also checks `ffprobe` (every build runs it).

- Exit codes are unchanged except the new `6`, which only `prompt --strict` returns.
- `VCR_AGENT_MODE=1` consumers keep their keys; the document also gains the envelope fields.
- The exit-code classification that used to live in `main.rs` as message matching now lives in one
  place (`agent_contract`), with the same mapping.
