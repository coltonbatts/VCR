# ASCII Video and Motion

## 1. Frame-by-Frame ASCII Playback Model

ASCII video is a sequence of text framebuffers emitted at target cadence.
For frame `t`:

1. Acquire decoded pixel frame `I_t`
2. Convert to luminance field `L_t`
3. Resample to character grid `D_t`
4. Map to glyph grid `G_t`
5. Write `G_t` to terminal buffer

## 2. Timing Constraints

Effective playback rate is bounded by:

- Decode time per frame
- Mapping time per frame
- Terminal write throughput (bytes/frame)
- Emulator flush latency

Budget equation:

`T_total = T_decode + T_map + T_write + T_flush`

For target FPS `F`, requirement is:

`T_total <= 1/F`

## 3. Why ASCII Video Flickers

Flicker mechanisms:

- Full-screen clear before redraw exposes blank intermediate states
- Cursor-visible writes reveal scan order
- Multi-call writes produce partial-frame visibility
- Terminal compositor asynchronous repaint

Mitigations:

- Hide cursor during playback
- Use alternate screen buffer
- Emit one contiguous frame write when possible
- Use diff-based updates with stable order

## 4. Deterministic Rendering Challenges

Even with fixed input video:

- Font fallback differences change glyph shape
- Terminal width/height changes alter sampling grid
- Floating-point math differences can alter quantization at boundaries
- Decoder configuration differences can alter source RGB values

Deterministic system must lock all of these.

## 5. CPU vs GPU in ASCII Pipelines

GPU path advantages:

- Parallel luminance/downsampling
- Higher throughput for high resolutions

GPU path risks for determinism:

- Driver variability
- Shader precision/rounding differences
- Thread scheduling nondeterminism in reduction order

Software raster path is preferred as baseline for deterministic tests.

## 6. Text Buffer Reuse and Double Buffering

Represent current and previous frame as cell arrays.

- `prev[row][col]`
- `curr[row][col]`

Update rule:

- If `curr != prev`, emit cursor-move + glyph write
- Else skip write

Double-buffering avoids full clears and reduces bandwidth.
Stable row-major diff traversal ensures reproducible output order.

## 7. Deterministic Frame Hashing

Each ASCII frame should be hashable for regression testing.
Use canonical serialization:

- UTF-8 or ASCII bytes of rows joined by `\n`
- Final newline policy fixed (either always or never)

FNV-1a 64-bit reference:

- Offset basis: `0xcbf29ce484222325`
- Prime: `0x00000100000001B3`
- For each byte: `hash = (hash XOR byte) * prime` (wrapping)

## 8. Deterministic ASCII Video Pipeline for VCR

`Video Frame`
`-> Resize`
`-> Luminance extraction`
`-> Glyph mapping`
`-> Frame buffer write`
`-> Hash validation`

Where:

- Resize uses fixed algorithm (box average)
- Luminance uses fixed integer BT.709 coefficients
- Glyph ramp is frozen or versioned artifact
- Write order is deterministic row-major
- Per-frame hash stored in sidecar for replay checks

## 9. Regression Strategy

For an input fixture:

1. Run pipeline twice on same machine; hashes must match
2. Run pipeline on another machine with same pinned toolchain/font/atlas; hashes must match
3. If mismatch, diff stage hashes (luma hash, grid hash, final frame hash)

This isolates nondeterministic stage boundaries.
