---
name: vcr-figma-workflow
description: Convert Figma + prompt context into VCR-compatible outputs using the `figma-vcr-workflow` binary and VCR validation/render commands. Use when translating design intent into deterministic VCR scenes for preview/build pipelines.
---

# VCR Figma Workflow
Translate design intent into deterministic VCR artifacts through the workflow binary.

## Workflow
1. Build required binaries:
   - `cargo build --release --bin vcr --bin figma-vcr-workflow`
2. Run Figma-to-VCR workflow with project-specific inputs.
3. Validate generated scene:
   - `./target/release/vcr check <generated>.vcr`
4. Generate first-frame verification:
   - `./target/release/vcr render-frame <generated>.vcr --frame 0 -o renders/figma_f0.png`
5. Build final output:
   - `./target/release/vcr build <generated>.vcr -o renders/figma_output.mov`

## Guardrails
- Keep generated manifests editable and re-validatable via `vcr check`.
- Preserve deterministic settings (`seed`, typed params) when refining generated scenes.
- Avoid introducing non-portable absolute asset paths.

## Success Criteria
- Generated manifest validates cleanly.
- Preview frame visually matches intended composition.
- Final output is produced with deterministic settings captured in metadata/inputs.

## Failure Handling
- Workflow generation issue: inspect workflow input assumptions and rerun with reduced scope.
- Validation errors in generated manifest: patch manifest, then re-run `check`.
- Visual mismatch: iterate on prompt/context and repeat frame-level verification before full build.
