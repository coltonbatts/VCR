# ASCII Determinism Spec

## 1. Purpose

Define normative rules to guarantee: same inputs produce identical ASCII outputs across machines.

## 2. Terminology

- `MUST`: required for conformance
- `SHOULD`: recommended unless documented reason exists
- `MAY`: optional

## 3. Deterministic Inputs

A conforming run MUST freeze:

1. Source frame bytes (or decoded frame bytes)
2. Renderer config (`cols`, `rows`, aspect ratio, polarity)
3. Glyph atlas bytes
4. Character ramp bytes
5. Luminance and resampling algorithms
6. Output newline policy
7. Toolchain + dependency lockfile

## 4. Numeric Rules

### 4.1 Luminance

Renderer MUST compute luma with deterministic arithmetic.
Recommended integer form:

`Y8 = floor((2126R + 7152G + 722B + 5000) / 10000)`

### 4.2 Cell Averaging

Renderer MUST use deterministic traversal order and deterministic rounding.
For box average:

`Ycell = floor((sum(Y8) + N/2) / N)`

### 4.3 Quantization

Given ramp length `N`, index MUST be:

`i = floor((Ycell * (N - 1) + 127) / 255)`

with clamp to `[0, N-1]`.

Reconstructed quantized luma for mapped index `i` is:

`Yq = floor((i * 255 + floor((N - 1)/2)) / (N - 1))` for `N > 1` (else `Yq = 0`).

### 4.4 Temporal Coherence (`hysteresis`)

Temporal mode is explicitly configured:

- `none`: no cross-frame state
- `hysteresis`: per-cell hold against prior mapped index

For `hysteresis`, with previous index `p`, nearest index `n`, effective luma `Yeff`, and configured band `B`:

- If `p == n`, output `n`
- Else let `center = Yq(p)`, `low = max(0, center - B)`, `high = min(255, center + B)`
- If `Yeff` in `[low, high]`, output `p`; otherwise output `n`

Traversal and state update MUST be row-major and deterministic.

### 4.5 Cell Dithering (`floyd_steinberg_cell`)

Dither mode is explicitly configured:

- `none`: direct quantization
- `floyd_steinberg_cell`: deterministic error diffusion on cell grid

For `floyd_steinberg_cell`, implementation MUST use:

- Scan order: top-to-bottom, left-to-right (row-major)
- Boundary handling: drop contributions outside grid (intentional deterministic edge bias)
- Error kernel (same-row/next-row): `7/16`, `3/16`, `5/16`, `1/16`
- Fixed-point accumulator in `1/16` units
- Error-to-luma adjustment with explicit helper:
  - `div_round_nearest_ties_away_from_zero(numer, denom)`:
    - `abs_q = floor((abs(numer) + floor(denom/2)) / denom)`
    - return `-abs_q` if `numer < 0`, else `abs_q`
  - `Yeff = clamp(Ycell + div_round_nearest_ties_away_from_zero(err16, 16), 0, 255)`
- Quantization error: `e = Yeff - Yq(mapped_index)`
- Diffusion adds integer numerators: `7e, 3e, 5e, 1e` to the four kernel neighbors

### 4.6 Canonical Mapping Order

Canonical per-cell order (row-major) is:

1. Start from `Ycell` (post BT.709 + cell averaging + alpha gate + boost)
2. Apply dither accumulator (`err16`) to get `Yeff` with the helper above
3. Compute nearest index from `Yeff`
4. Apply hysteresis decision using `Yeff` and previous mapped index (if enabled)
5. Emit final mapped index and glyph byte
6. Compute quantization error from final mapped index and diffuse to future cells

## 5. Glyph Selection Rules

- Output glyphs MUST be bytes from configured ramp
- Ramp ties (if generated from densities) MUST be resolved by codepoint ascending
- Renderer MUST NOT depend on terminal runtime font metrics

## 6. Canonical Frame Serialization

Before hashing, frame bytes MUST be serialized identically:

- Rows in top-to-bottom order
- Columns left-to-right
- Row separator: `\n`
- Trailing newline policy fixed and documented (this spec uses no trailing newline)

## 7. Hash Specification

Frame hash MUST use FNV-1a 64-bit:

- Offset basis: `0xcbf29ce484222325`
- Prime: `0x00000100000001B3`
- Algorithm:
  - `hash = offset`
  - For each byte `b`: `hash ^= b`; `hash *= prime` (wrapping)

Sequence hash SHOULD be computed over concatenated little-endian frame hashes.

## 8. Environment Locking

Conforming implementations MUST document:

- Rust toolchain version
- Dependency lockfile hash
- Atlas artifact hash
- Ramp artifact hash

Terminal emulator choice MUST NOT alter generated ASCII bytes because rendering is computed in software before output.

Scope clarity:

- Covered by this spec (deterministic when inputs/config are fixed):
  - Luma math, averaging, quantization, dither/hysteresis logic, traversal order, frame/sequence hashing
- Not covered unless separately locked:
  - Decode output bytes, font rasterization/runtime font fallback, terminal emulator display differences, Unicode width/grapheme policy, locale-dependent behavior

## 9. Compliance Tests

A deterministic implementation MUST pass:

1. Same-run repeatability: run same fixture twice, hashes equal
2. Cross-machine repeatability: run fixture on second machine with same lock state, hashes equal
3. Sensitivity test: change one parameter, hashes differ

## 10. Required Sidecar Metadata

For each run, sidecar SHOULD include:

- Input identifier + hash
- Config JSON + hash
- Atlas hash
- Ramp hash
- Per-frame hash list
- Sequence hash

When debug stage hashes are enabled, sidecar SHOULD also include per-frame:

- `luma_grid_hash` over row-major post-alpha/boost `Ycell` bytes
- `mapped_grid_hash` over row-major mapped index bytes (little-endian `u16`)
- `frame_chars_hash` over row-major final frame character bytes

## 11. Non-Conformance Conditions

Implementation is non-conformant if any of the following occur:

- Uses runtime terminal glyph rasterization
- Uses unspecified rounding behavior
- Allows unordered map iteration to affect frame output
- Emits locale-dependent bytes
- Produces different hashes for identical fixtures under locked environment

## 12. Regression Test Examples

Use the experiment binaries as deterministic probes.

### 12.1 Luminance Quantization Regression

```bash
cargo run --bin luminance_to_char_mapping_test -- \
  --width 128 \
  --height 72 \
  --ramp " .:-=+*#%@" \
  --expected-hash 0x6d64dcf935129ffb
```

Expected output includes:

- `regression_check=pass`
- `hash_fnv1a64=0x6d64dcf935129ffb`

### 12.2 Frame Sequence Pipeline Regression

```bash
cargo run --bin frame_to_ascii_pipeline_prototype -- \
  --synthetic-frames 4 \
  --cols 96 \
  --rows 54 \
  --ramp " .:-=+*#%@" \
  --expected-sequence-hash 0xb3aca483e66d062b
```

Expected output includes:

- `regression_check=pass`
- `sequence_hash_fnv1a64=0xb3aca483e66d062b`

### 12.3 Glyph Ramp Reproducibility Probe

```bash
cargo run --bin character_density_analysis -- \
  --variant regular \
  --write-ramp /tmp/vcr_ramp_regular.txt
```

Current reference values (this repository state):

- `ramp_hash_fnv1a64=0x418f6077f6ff8d8a`
