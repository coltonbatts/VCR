# Figma -> VCR Output Quality Polish

Date: 2026-02-10
Owner: Codex session handoff

## Scope
Polished MVP output quality across extraction, text asset generation, and manifest positioning so product-card renders are usable for demo review.

## Problems Addressed
- Placeholder/garbage text values were extracted from Figma (`"Result website title ..."`, `"No title"`, `"Site title"`).
- Text assets were using Figma-exported PNG snippets directly, which preserved bad placeholder content and produced inconsistent typography.
- Fallback manifest used hardcoded text positions (`pos_x/pos_y`), causing layout drift versus Figma.

## Implemented Fixes

### 1) Extraction and Validation (`src/workflow/figma_client.rs`)
- Added invalid text filtering and explicit validation for title/description placeholders.
- Added description-string fallback parsing for product name and price.
- Added alias-aware layer matching for common names:
  - `product_name | title | name`
  - `price | cost | amount | value`
  - `description | subtitle | caption | url`
  - `product_image | image | photo`
- Added optional verbose debug logs (`--verbose`) for:
  - selected layer id/name/text/font/bounds
  - extracted/fallback text sources
  - card bounds
- Added Figma `absoluteBoundingBox` extraction and persisted layout bounds in output JSON.

### 2) Text Asset Generation (`src/workflow/assets.rs`)
- Replaced direct download of text-layer PNGs with generated text PNGs for:
  - `product_name`
  - `price`
  - optional `description`
- Renderer uses Figma style hints (family/size/weight/line height) and text color hints.
- Implemented local Swift/AppKit text renderer for macOS with fallback font chain:
  - requested family -> `Geist Pixel` -> `ArialMT` -> `Helvetica` -> `Helvetica Neue`
- Added robust fallback: if text render fails, asset download falls back to Figma image URL.
- Added safe Swift module-cache env wiring for sandboxed runs.

### 3) Manifest Layout/Positioning (`src/workflow/manifest_generator.rs`)
- Replaced hardcoded fallback coordinates with bounds-driven placement when layout bounds are present.
- Uses card-relative geometry (scaled from Figma export scale) to compute:
  - image entry target and start x
  - product name position
  - price position
  - optional description position
- Keeps previous hardcoded values only as fallback when bounds are missing.

### 4) Data Model and CLI Wiring
- `src/workflow/types.rs`
  - Added `ProductCardLayout` and `NodeBounds` to `ProductCardData`.
- `src/bin/figma-vcr-workflow.rs`
  - Added `--verbose` CLI flag.
  - Passes user description into extractor for fallback text parsing.
  - Passes verbose mode into extractor/assets/manifest generator.

## Validation Performed
- `cargo check --offline` passed.
- `cargo test --offline` passed (all tests).
- Added/updated tests for:
  - invalid placeholder text rejection
  - description hint parsing (`product name`, `price`)
  - bounds-driven fallback manifest positioning

## Not Yet Validated in This Session
- Full end-to-end render against Luke's real Figma file was not re-run in this session because `FIGMA_TOKEN` was not available in environment at execution time.

## How To Run Final Validation

```bash
FIGMA_TOKEN="..." ANTHROPIC_API_KEY="..." \
cargo run --release --bin figma-vcr-workflow -- \
  --figma-file "<LUKE_FIGMA_FILE_OR_URL>" \
  --description "product card: pink skirt, $29.99" \
  --output-folder "./exports/polish_test" \
  --verbose
```

Review artifacts in generated run folder:
- `product_card_data.json`
- `product_name.png`, `price.png`, `description.png` (if present)
- `product_card.vcr`
- `product_card.mov`

## Expected Outcome After Fixes
- No placeholder title extraction in final text values.
- Cleaner text render with font/style hints and sane fallback fonts.
- Layout alignment closer to Figma due to bounds-based manifest coordinates.
- Better debugging visibility for extraction and placement issues.

## Remaining Risks / Next Iteration
- Font parity is best-effort (depends on local installed fonts; exact Figma font file parity not guaranteed).
- Swift text renderer is macOS-specific; a cross-platform renderer would be needed for Linux CI parity.
- Heuristics may still need tuning for highly irregular Figma naming conventions.
