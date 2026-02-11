# ASCII Perception and Density

## 1. Character Density as a Measurable Quantity

For deterministic ASCII mapping, each glyph needs a numeric darkness score.
For a binary glyph bitmap `g(x,y) in {0,1}` on `W x H` grid:

`density(g) = (1 / (W*H)) * sum g(x,y)`

Equivalent black-pixel percentage:

`black_pct = density(g) * 100`

This metric is reproducible and font-specific.

## 2. Why Ramp Quality Depends on Font

A ramp is an ordered list of glyphs from low to high darkness.
The same glyph code point has different rasterization in different fonts:

- Stroke thickness differs
- Hinting strategy differs
- Side bearings and glyph centering differ
- Antialias coverage differs

Therefore ramp order is not portable across fonts.

## 3. Anti-Aliasing and Subpixel Effects

Terminal rendering may include:

- Grayscale antialiasing
- Subpixel RGB antialiasing
- Hinting on/off

These alter perceived darkness at runtime even when code points are unchanged.
For deterministic pipelines, density measurement must occur on a fixed glyph atlas, not live terminal raster output.

## 4. Why Geist Pixel Changes Mapping Stability

A pixel-style font with fixed geometry reduces ambiguity:

- Binary-like glyph edges reduce fractional coverage variance
- Small deterministic atlas (`16x16` in this repo) allows exact bit counting
- No dependence on OS text stack at render time if atlas is precomputed

This improves cross-machine mapping consistency.

## 5. Density Measurement Program Design

Program requirements:

1. Enumerate printable ASCII (`0x20..0x7E`)
2. Render each glyph to bitmap (or use fixed pre-rasterized atlas)
3. Count filled pixels
4. Compute normalized density
5. Sort glyphs by density ascending
6. Resolve ties by codepoint ascending
7. Emit ranked ramp and per-glyph metrics

Output artifacts:

- `density_table.tsv` (codepoint, glyph, on_pixels, density)
- `ramp.txt` (single-line ordered glyph ramp)

## 6. Deterministic Sorting Rule

Stable ordering is mandatory.
Sort key:

1. `on_pixels` ascending
2. `codepoint` ascending

Without tie-breakers, equal-density glyph ordering may vary by sort implementation.

## 7. Mapping Function Selection

Given luminance `Y8 in [0,255]` and ramp length `N`:

`index = floor((Y8 * (N - 1) + 127) / 255)`

This maps low luminance to low-density glyphs under normal polarity.
If inverse polarity is required, use:

`index_inv = (N - 1) - index`

## 8. Ramp Validation Metrics

For a generated ramp `R`, validate:

- Monotonic non-decreasing density
- Minimum adjacent density delta statistics
- Histogram utilization on representative luminance fields

Regression target: same ramp string for same atlas and algorithm.

## 9. Practical Failure Modes

- Non-monospace terminal font: horizontal drift and overlap
- Unlocked font fallback: code points map to different glyph outlines
- Locale/encoding mismatch: bytes interpreted under wrong charset
- Gamma/contrast pre-processing changes: different luminance distribution

## 10. Reproducible Ramp Generator in This Repo

The experiment binary `character_density_analysis` implements deterministic ramp generation from the built-in atlas.
It avoids OS-dependent live font rasterization by using atlas bits in `src/ascii_atlas_data.rs`.

CLI pattern:

```bash
cargo run --bin character_density_analysis -- --variant regular --write-ramp /tmp/ramp_regular.txt
```

Determinism guarantee:

- Identical atlas data + algorithm => identical ramp bytes.
