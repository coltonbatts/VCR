# ASCII Rendering Architecture

## 1. Scope

This document defines a deterministic ASCII rendering architecture for VCR with:

- Pure software reference implementation
- Backend abstraction for optional accelerators
- Font/atlas locking
- Hash-based reproducibility validation

## 2. Core Data Types

### 2.1 Frame Inputs

- `FrameRgba8 { width, height, pixels }`
- `Timestamp { frame_index, pts }`

### 2.2 Intermediate Buffers

- `LumaPlaneU8 { width, height, y }`
- `CellLumaGrid { cols, rows, y_cell }`
- `AsciiFrame { cols, rows, bytes }` (`bytes` contains printable ASCII + `\n` separators)

### 2.3 Static Assets

- `GlyphAtlas` (fixed binary glyph bitmaps)
- `CharacterRamp` (density-sorted glyph list)

## 3. Pipeline Stages

1. `decode_frame` (external source)
2. `extract_luma`
3. `resample_to_cells`
4. `map_luma_to_glyphs`
5. `compose_ascii_frame`
6. `emit_or_store_frame`
7. `hash_frame`

Each stage has deterministic inputs/outputs and can be independently hashed.

## 4. Required Renderer Interface

```rust
pub trait AsciiRenderer {
    fn configure(&mut self, config: AsciiRenderConfig) -> anyhow::Result<()>;
    fn render_frame(&mut self, frame: &FrameRgba8) -> anyhow::Result<AsciiFrame>;
    fn frame_hash(&self, frame: &AsciiFrame) -> u64;
}
```

`AsciiRenderConfig` must include:

- `cols`
- `rows` or aspect-derived row rule
- `cell_aspect_num`, `cell_aspect_den`
- `ramp` (explicit bytes)
- `polarity`
- `normalization policy`

## 5. Backend Abstraction Layer

### 5.1 Software Backend (Reference)

- Integer luminance math
- Integer box-resample indexing
- Stable row-major traversal
- Mandatory for determinism tests

### 5.2 Optional Accelerated Backend

- Allowed for preview/performance modes
- Must be validated against software hashes or disabled for canonical builds

## 6. Character Ramp Injection

Ramp must be externally injectible and versioned.

- Source options: file path, embedded default, generated ramp artifact
- Validation: ASCII printable only, non-empty, unique bytes
- Version string should be captured in metadata sidecar

## 7. Font Locking Strategy

Deterministic output requires glyph lock.

Recommended hierarchy:

1. Precomputed atlas committed to repository (`src/ascii_atlas_data.rs`)
2. Optional atlas generation tool with pinned font files and thresholds
3. Runtime rendering must use atlas bits, not terminal-provided glyph rasterization

## 8. Deterministic Font Measurement

If atlas regeneration is required:

- Pin font file checksum
- Pin rasterizer version (`fontdue` crate version)
- Pin glyph cell size and alpha threshold
- Serialize atlas with canonical formatting

Atlas checksum should be included in build metadata.

## 9. Fixed Aspect Correction

Character cells are non-square. Use explicit ratio:

`cell_aspect = cell_height / cell_width`

If `rows` is derived from source aspect:

`rows = round((src_h / src_w) * cols / cell_aspect)`

Do not infer from terminal runtime.

## 10. Output Contracts

For each rendered frame:

- ASCII payload bytes
- Frame hash (`fnv1a64`)
- Frame index
- Config hash

For sequences:

- Ordered frame hash list
- Sequence hash over concatenated frame hashes

## 11. Failure Handling

Hard-fail conditions:

- Invalid ramp
- Zero dimensions
- Atlas mismatch
- Non-ASCII glyph byte in output

Soft-fail optional:

- Missing accelerated backend (fallback to software)

## 12. Integration Points in VCR

Relevant existing modules:

- `/Users/coltonbatts/Desktop/VCR/src/ascii_render.rs`
- `/Users/coltonbatts/Desktop/VCR/src/ascii.rs`
- `/Users/coltonbatts/Desktop/VCR/src/ascii_atlas.rs`
- `/Users/coltonbatts/Desktop/VCR/src/ascii_atlas_data.rs`

These form the initial substrate for implementing this architecture without introducing OS text-render dependencies.
