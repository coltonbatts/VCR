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
