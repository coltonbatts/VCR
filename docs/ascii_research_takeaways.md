# ASCII Research Takeaways (Pulled from deep-research-report.md)

This document extracts the highest-value, codebase-relevant items from:

- `/Users/coltonbatts/Desktop/VCR/docs/deep-research-report.md`

## What Is Already Strong in VCR

- Determinism-first framing (specs, tests, and FNV-1a hashing) is already well established.
- Glyph atlas locking is already present via committed atlas data in `src/ascii_atlas_data.rs`.
- Aspect correction, ramp ordering, and deterministic traversal are already documented and partially implemented.

## High-Value Pull-Through Opportunities

### 1) Unify Luma Model Across Code + Specs (High)

The report reinforces that luminance model drift causes subtle cross-run output changes.
Current docs emphasize BT.709 / deterministic arithmetic, while `src/ascii_render.rs` currently uses float BT.601-style coefficients (`0.299/0.587/0.114`).

Recommendation:

- Standardize on one canonical luma path for ASCII video conversion.
- Prefer deterministic integer BT.709 math for the reference path.
- Keep any stylistic/exposure boosts explicit and optional so canonical determinism remains clean.

### 2) Add Deterministic Temporal Coherence Mode (High)

The report's hysteresis idea is a practical anti-flicker improvement for video ASCII.
Small per-cell luma changes currently can flip neighboring glyphs frame-to-frame.

Recommendation:

- Add an optional deterministic hysteresis mode:
  - keep previous glyph unless luma crosses a threshold band.
- Preserve strict deterministic behavior by fixing threshold constants and traversal order.

### 3) Expose Deterministic Cell-Level Dithering Modes (Medium)

The report highlights that deterministic error diffusion can improve perceived tonal detail.

Recommendation:

- Add an explicit mapping option set:
  - `none` (current baseline)
  - `floyd_steinberg_cell` (fixed kernel, fixed scan order)
- Lock scan order, boundary handling, and precision in spec docs and tests.

### 4) Strengthen Stage-Level Hash Debugging (Medium)

The report's suggestion to hash stage outputs is useful for fast determinism triage.

Recommendation:

- Optionally emit hashes for:
  - luma plane
  - cell-luma grid
  - final ASCII frame bytes
- Keep per-frame output hash as the canonical contract; stage hashes are diagnostic.

### 5) Unicode Mode Policy (Future)

The report is right that Unicode density modes are powerful but width-sensitive.
For VCR's deterministic baseline, ASCII-only remains the safest default.

Recommendation:

- Keep canonical mode ASCII-only.
- If Unicode mode is added, define:
  - allowed glyph subset
  - width policy (`wcwidth` behavior)
  - fallback behavior for unsupported glyphs/fonts

## Suggested Execution Order

1. Luma model unification
2. Temporal hysteresis mode
3. Deterministic dithering mode
4. Stage-hash diagnostics
5. Optional Unicode policy design
