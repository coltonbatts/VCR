---
name: vcr-manifest-author
description: Author and refine VCR `.vcr` YAML manifests for deterministic 2D motion graphics renders. Use when creating new scenes, modifying layer behavior, adding expressions/params, or validating manifest schema before rendering with `vcr check`, `vcr render-frame`, and `vcr build`.
---

# VCR Manifest Author
Write valid, deterministic VCR manifests and validate them before full renders.

## Workflow
1. Define or update `version`, `environment`, and `layers` in the target `.vcr` file.
2. Run validation:
   - `vcr check <scene>.vcr`
3. Run quick visual verification:
   - `vcr render-frame <scene>.vcr --frame 0 -o renders/<scene>_f0.png`
4. Build final output:
   - `vcr build <scene>.vcr -o renders/<scene>.mov`
5. If parameters are used, enumerate and test overrides:
   - `vcr params <scene>.vcr --json`
   - `vcr build <scene>.vcr --set key=value -o renders/<scene>_variant.mov`

## Authoring Rules
- Always include at least one layer.
- Use unique non-empty `id` values for all layers.
- Keep image paths relative to the manifest directory.
- Treat `t` as frame number, not seconds.
- Prefer typed params for reusable manifests.
- Re-run `vcr check` after every meaningful edit.

## Success Criteria
- `vcr check` exits with code `0`.
- A frame preview is generated successfully.
- Final render artifact exists at the expected output path.

## Failure Handling
- Unknown field/variant: fix key spelling or enum value and re-run `vcr check`.
- Expression failure: replace unsupported functions with supported expression functions and retry.
- Missing dependency: run `vcr doctor` and resolve FFmpeg/toolchain issues.
- Shader/post effects not visible: ensure GPU backend (`--backend gpu` or `auto`) is used.
