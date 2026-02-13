---
name: vcr-debugger
description: Diagnose and fix VCR manifest/render failures. Use when `vcr check`, `vcr lint`, `vcr preview`, `vcr render-frame`, or `vcr build` fail; when output is blank/incorrect; or when determinism and backend behavior must be investigated.
---

# VCR Debugger
Debug VCR failures quickly with a deterministic triage sequence.

## Triage Sequence
1. Validate manifest shape and references:
   - `vcr check <scene>.vcr`
   - `vcr lint <scene>.vcr`
2. Inspect frame-level state:
   - `vcr dump <scene>.vcr --frame <n>`
3. Reproduce with a single frame:
   - `vcr render-frame <scene>.vcr --frame <n> -o renders/debug_f<n>.png`
4. Verify runtime/dependency health:
   - `vcr doctor`
5. If determinism is in question:
   - `vcr determinism-report <scene>.vcr --frame <n> --json`

## Debug Heuristics
- Start with `check` before any expensive render.
- Prefer minimal frame repro (`render-frame`) over full build during debugging.
- Isolate params and disable optional complexity (post/shaders) when narrowing root cause.
- Confirm backend assumptions for GPU-only features.

## Common Root Causes
- Schema/key typos (`deny_unknown_fields`).
- Missing required environment fields.
- Invalid expression functions/variables.
- Invalid or out-of-scope asset paths.
- GPU-only features executed on software backend.

## Success Criteria
- Reproducible root cause identified.
- Error state removed (`check` passes).
- Target frame or preview matches expected visual behavior.

## Failure Handling
- If failing command output is unclear, rerun on reduced scene complexity.
- If render mismatch persists, compare with a known-good minimal manifest.
- If backend-specific behavior diverges, document backend + platform in repro notes.
