# ASCII Foundations

## 1. Encoding Substrate

ASCII is a 7-bit character encoding with 128 code points (`0x00` to `0x7F`).
Printable ASCII occupies `0x20` to `0x7E`.

For ASCII rendering systems, code points are not semantic text first; they are glyph selectors.
A renderer maps luminance samples to a subset of printable glyphs, then writes byte values to a terminal buffer.

## 2. Why Glyphs Have Different Visual Density

A glyph is a binary or anti-aliased raster over a fixed cell.
If a cell has width `W` and height `H`, and glyph bitmap value is `g(x,y) in {0,1}` for binary glyphs, glyph darkness is:

`density = (sum_{y=0..H-1} sum_{x=0..W-1} g(x,y)) / (W * H)`

Higher filled-area density yields darker appearance against a light background, or brighter appearance in inverse polarity.

## 3. Monospace and Terminal Cell Geometry

Terminal ASCII rendering assumes one glyph per cell in a uniform grid:

- Cell width: `cw` pixels
- Cell height: `ch` pixels
- Grid dimensions: `cols x rows`
- Display area: `(cols * cw) x (rows * ch)`

Monospacing is required for deterministic placement. Proportional fonts break the one-cell-per-symbol model.

## 4. Pixel Grid vs Character Grid Mismatch

Input images are pixel grids (`Pw x Ph`), but terminal outputs are character grids (`cols x rows`).
Resampling is mandatory.

Given target `cols`, corrected `rows` can be derived from source aspect and cell aspect ratio (`cell_aspect = ch/cw`):

`rows = round((Ph / Pw) * cols / cell_aspect)`

Without this correction, images appear vertically stretched or compressed.

## 5. Luminance Field Construction

Convert RGB to luminance per pixel. Use BT.709 coefficients:

`Y = 0.2126R + 0.7152G + 0.0722B`

For deterministic integer arithmetic on 8-bit channels:

`Y8 = floor((2126R + 7152G + 722B + 5000) / 10000)`

where `Y8 in [0,255]`.

## 6. Downsampling Operators

For each output cell, map to a source pixel region and compute representative luminance.

### Nearest

Sample center coordinate only:

`Ycell = Y(round(xc), round(yc))`

Fast, but high aliasing.

### Box Filter / Average

Average all source pixels in the cell footprint:

`Ycell = (1/N) * sum_{p in footprint} Y(p)`

Deterministic and stable for terminal use.

### Weighted Filters

Higher-order filters reduce aliasing but introduce more floating-point sensitivity and computational cost.

## 7. Quantization and Character Ramp Mapping

Given a ramp `R = [r0, r1, ..., r(n-1)]` sorted by glyph density (light to dark), map `Ycell` to index:

`i = round((Ycell / 255) * (n - 1))`

Then output `R[i]`.

Equivalent integer form:

`i = floor((Ycell * (n - 1) + 127) / 255)`

This avoids float drift.

## 8. Full Pipeline Model

ASCII rendering pipeline from first principles:

`Image -> Luminance field -> Downsample -> Quantize -> Glyph mapping -> Terminal frame`

Formally:

1. `L = f_luma(I)`
2. `D = f_downsample(L, cols, rows, cell_aspect)`
3. `Q = f_quantize(D, ramp_len)`
4. `G = f_map(Q, ramp)`

## 9. Information Loss Channels

ASCII conversion is lossy in multiple stages:

- Spatial loss: many pixels collapse into one cell
- Tonal loss: continuous luminance reduced to ramp cardinality `n`
- Shape loss: glyph basis is discrete and font-dependent
- Temporal loss (video): frame rate and terminal throughput constraints

Loss can be measured with reconstruction error by re-rasterizing ASCII back to pixel space and comparing against downsampled source.

## 10. Deterministic Requirements at Foundation Level

To ensure identical outputs across machines:

- Fix encoding (`ASCII`, no locale transforms)
- Fix font and glyph atlas
- Fix cell geometry
- Use integer arithmetic where possible
- Use stable sort tie-breakers for equal-density glyphs (codepoint order)
- Use canonical newline and buffer write order
