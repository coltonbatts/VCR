---
name: vcr-style-skill-template
description: Define and apply a reusable VCR visual style contract (fonts, palette, motion, layout, and output defaults) without changing engine code. Use when creating team/persona-specific creative direction on top of VCR manifests.
---

# VCR Style Skill Template
Use this as a base skill for style-specific VCR output.

## Goal
Treat VCR as a stable render engine and encode creative direction as a style profile.

## What This Skill Controls
- Typography defaults (font family, size ramps, spacing)
- Color system (brand/accent/neutral palette)
- Motion language (easing families, timing windows, entrance/exit behavior)
- Composition conventions (safe margins, z-index tiers, anchor rules)
- Render defaults (fps, resolution classes, output naming)

## How to Customize
1. Copy `references/style_tokens.example.yaml` to `references/style_tokens.yaml`.
2. Replace token values with your preferred look-and-feel.
3. Keep token names stable; only change values unless adding a new style capability.
4. Use `assets/presets/` for reusable scene snippets that reflect the style.

## Style Application Workflow
1. Read `references/style_tokens.yaml`.
2. Author or patch target `.vcr` manifest to conform to tokens.
3. Validate:
   - `vcr check <scene>.vcr`
4. Snapshot first frame:
   - `vcr render-frame <scene>.vcr --frame 0 -o renders/<scene>_style_f0.png`
5. Build output:
   - `vcr build <scene>.vcr -o renders/<scene>_style.mov`

## Conformance Checks
- Font usage matches approved token set.
- Palette use stays within tokenized colors unless explicitly overridden.
- Timing/easing follows style defaults for entrances/exits.
- Layer spacing and alignment obey composition tokens.
- Validation and render commands complete successfully.

## Failure Handling
- Style drift: diff manifest values against token file and normalize.
- Visual mismatch: adjust token values, not engine internals.
- Invalid manifest: resolve schema/expression errors via `vcr check`.
- Backend-specific effects mismatch: confirm backend and re-test minimal scene.

## Packaging Guidance
- Keep this skill lean; place detailed guides in `references/`.
- Create derivative skills by copying this folder and changing frontmatter + tokens.
