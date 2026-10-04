# VCR Determinism Specification

## 0. Scope levels (read first)

"Deterministic" means different things at different layers; `*.provenance.json` records the conditions for each:

| Level | Guarantee |
|---|---|
| Scene and settings (`manifest_hash`, `resolved_manifest_hash`) | Always reproducible |
| Raster frames before encoding (`hashes.raster_frames_sha256`) | Software backend: expected identical for the same engine build (the golden frame hash matches on macOS and Linux). GPU: not guaranteed across hardware/drivers/OS |
| Decoded frames (`hashes.decoded_frames_sha256`) / encoded file bytes (`hashes.output_file_sha256`) | Same ffmpeg build and arguments only. `tests/golden_manifest_stability.rs` pins an encoded-file hash from one ffmpeg build; on a different ffmpeg the raster hash still matches while the file hash does not (observed with ffmpeg 6.1.1 on Linux) |

Compare `engine.*`, `toolchain.ffmpeg` and `backend` in provenance before treating a hash difference as a defect.

## 1. Contract

**Deterministic run**: Same inputs → identical frame RGBA bytes.

Inputs:
- Manifest file (content hash)
- Param overrides (sorted, stable)
- Frame index
- Seed (manifest `seed` + any CLI `--seed`)
- Backend (software vs GPU)
- Toolchain + Cargo.lock

## 2. Sources of Nondeterminism (Audited)

| Source | Status | Mitigation |
|--------|--------|------------|
| **GPU backend** | Not bit-exact across platforms | Use software backend for CI; document as hardware-bound |
| **Floating point** | Platform differences | Accept; scope guarantee to same machine |
| **Parallelism** | Frame render is sequential | No parallel frame dispatch |
| **RNG** | `noise1d`, `random`, `glitch` | Seeded via `context.seed` (manifest.seed) |
| **System time** | Not used in render path | N/A |
| **File ordering** | BTreeMap for params | Stable key ordering |
| **External API** | Core has none | Integrations isolated |
| **HashMap iteration** | Glyph caches, figma client | Does not affect frame output; layer order is stable |

## 3. Explicit RNG Seeding

- Manifest `seed: u64` (default 0)
- `ExpressionContext { t, params, seed }` passed to all expression evaluation
- `noise1d(x, seed)`, `random(x)` use `hash_to_unit_range` with seed
- ASCII stage/capture: `--seed` CLI flag

## 4. Stable Ordering

- Params: `BTreeMap` for resolved_params, applied_param_overrides
- Layers: Vector order preserved; z_index for draw order
- JSON output: `serde_json` with BTreeMap → stable key order

## 5. Float Tolerances

- No cross-platform float comparison in tests
- Determinism tests use software backend only
- Golden *frame* hashes are portable across machines for the software backend; golden *file* hashes are ffmpeg-build-specific; CI compares against CI baseline

## 6. Determinism Tests

- `tests/determinism.rs`: Same manifest + overrides → same hash (software)
- `tests/cli_contract.rs`: `ascii lab` output deterministic
- Golden scenes: `examples/white_on_alpha.vcr`, `examples/steerable_motion.vcr`

## 7. CI Verification

- `cargo test --test determinism` (software backend)
- Optional: `--determinism-report` renders golden frames and emits hashes

## 8. Guarantee Scope

**We guarantee**: Deterministic on identical hardware + driver + backend + toolchain.

**We do not guarantee**: Bit-exact GPU output across macOS vs Linux vs Windows.
