# ASCII_RENDERER_SPEC

## 1. Interface

### 1.1 Inputs

Required:

- `frame_rgba8`: source frame bytes in RGBA8
- `source_width`, `source_height`
- `cols`
- `rows` or `auto_rows=true` with aspect policy
- `cell_aspect_num`, `cell_aspect_den`
- `character_ramp` (ordered ASCII bytes)
- `glyph_atlas_id` (resolved to fixed atlas bytes)
- `polarity` (`normal` or `inverse`)

Optional:

- `pre_luma_transform` (must be deterministic if enabled)
- `background_policy`
- `temporal_mode` (`none` or `hysteresis`)
- `hysteresis_band` (`u8`, only used when `temporal_mode=hysteresis`)
- `dither_mode` (`none` or `floyd_steinberg_cell`)
- `debug_stage_hashes` (`bool`, diagnostic only)

### 1.2 Outputs

Per frame:

- `ascii_bytes` (rows joined with `\n`, no trailing newline)
- `frame_hash_fnv1a64`
- `cols`, `rows`

Per sequence:

- Ordered list of frame hashes
- Sequence hash
- Metadata sidecar

## 2. Constraints

- Glyph output MUST be printable ASCII bytes unless mode explicitly extends charset
- `character_ramp` MUST be non-empty and deterministic order
- `cols >= 1`, `rows >= 1`
- Atlas dimensions and cell mapping MUST be fixed for run

## 3. Determinism Guarantees

Implementation MUST guarantee:

1. Same input bytes + same config + same atlas + same ramp => identical `ascii_bytes`
2. Identical `ascii_bytes` => identical FNV hash
3. Stage computations independent of terminal emulator runtime

Implementation SHOULD:

- Use integer luminance and quantization
- Use stable row-major traversal
- Use canonical serialization for hash input
- Keep canonical output hash independent from optional debug hash emission

Canonical mapping order:

1. Compute per-cell `Ycell`
2. Apply dither accumulator (if enabled) to obtain `Yeff`
3. Quantize nearest index from `Yeff`
4. Apply hysteresis (if enabled) using `Yeff` and previous mapped index
5. Emit final index/glyph, then diffuse quantization error (if enabled)

## 4. Backend Model

- `software` backend is reference and required
- `accelerated` backend is optional and non-canonical unless validated against software output
- Canonical test mode MUST force software backend

## 5. Font and Atlas Requirements

- Renderer MUST map bytes to glyphs through fixed atlas data
- Atlas artifact SHOULD be versioned and checksummed
- Runtime OS font stack MUST NOT participate in glyph-shape decisions

## 6. Aspect Correction

If `rows` auto-derived:

`rows = round((source_height / source_width) * cols / (cell_aspect_num/cell_aspect_den))`

Aspect policy MUST be included in metadata sidecar.

## 7. Hash Function

FNV-1a 64-bit:

- offset: `0xcbf29ce484222325`
- prime: `0x00000100000001B3`

Byte stream is exactly `ascii_bytes`.

## 8. Test Harness Requirements

Harness MUST include:

1. Fixture input frame or frame-sequence
2. Locked config JSON
3. Expected per-frame hashes
4. Expected sequence hash

Harness MUST perform:

- Repeatability run (`run A` vs `run B`)
- Cross-machine verification (same lock state)
- Parameter perturbation test (hashes must change)

## 9. Failure Contract

Hard error conditions:

- Invalid ramp (empty, duplicate if disallowed, non-ASCII)
- Missing atlas
- Invalid dimensions
- Hash mismatch in regression mode

Errors MUST be surfaced with explicit stage context.
