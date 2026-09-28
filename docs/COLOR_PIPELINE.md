# Color Pipeline

VCR has one canonical color and blend pipeline. The GPU (wgpu) and software (CPU)
backends both implement it, and `tests/backend_parity.rs` checks that they agree.
The implementation is shared through `src/color.rs`. Render metadata records the pipeline
revision as `color_pipeline`.

## Stages

| Stage | Definition |
|---|---|
| 1. Inputs | Manifest colors, image files, rasterized text/ascii, and custom shader output are **straight alpha, sRGB-encoded**. A manifest `0.5` or `#808080` means the same thing it means in a browser. |
| 2. Decode | sRGB EOTF (IEC 61966-2-1 piecewise curve) to **linear light**, then **premultiply** by alpha. |
| 3. Layer textures | Premultiplied linear color stored **sRGB-encoded in 8 bits per channel** (exactly what an `Rgba8UnormSrgb` texture holds). Sampling decodes to linear *before* bilinear filtering, so filtering and resampling happen in premultiplied linear light. |
| 4. Procedural shapes | Evaluated per pixel center with the same formulas on both backends (`PROCEDURAL_SHADER` ↔ `procedural_premultiplied`). Gradients interpolate their premultiplied linear endpoints. |
| 5. Compositing | Layer opacity scales all four premultiplied channels. Layers combine with premultiplied **source-over** (`dst = src + dst × (1 − src.a)`) into a float accumulator (GPU `Rgba16Float`, CPU `f32`). |
| 6. Output | **Unpremultiply**, sRGB-encode, and quantize with round-to-nearest to straight-alpha RGBA8. Pixels whose alpha rounds to 0 are written as `(0, 0, 0, 0)`. This is the only encode in the pipeline, and it feeds PNGs and the FFmpeg encoder. |

## Consequences you can see

- A 50%-opacity white layer over black produces sRGB 188, not 128, because it is a linear-light mix.
- Gradients and bilinear-scaled images no longer darken through their midpoints or edges.
- Transparent output (`white_on_alpha`, `instrument_logo_reveal`) carries straight color.
  Previously the GPU wrote premultiplied RGB into straight-alpha PNG/ProRes, which gave
  semi-transparent edges dark fringes.
- Custom WGSL `shade()` functions return straight sRGB color like every other input.
  The preamble converts it.
- Procedural geometry is defined in pixel space: centers, points and sizes are fractions of
  the frame's width and height, and radii and thickness are fractions of the frame
  **width**. A `circle` is therefore a circle at any aspect ratio. The GPU used to draw
  it as an ellipse on non-square frames. Lines have round caps, and polygons put their
  first vertex straight up.

## Coverage rules (both backends)

- A layer is drawn by sampling its texture at each covered output pixel center through
  the inverse layer transform, with bilinear filtering and clamp-to-edge.
- Coverage follows GPU rasterization: quad corners are snapped to a 1/256 px grid and
  pixel centers are tested with the top-left fill rule. Without that emulation, pixels
  lying within a hair of a rotated layer's hard edge would flip between backends.
- There is no edge anti-aliasing yet. Hard shape and layer edges alias identically on both
  backends.

## Parity tolerance

`tests/backend_parity.rs` requires every channel of every pixel to agree within
**3 code values** after weighting color by alpha (`rgb × a / 255`; alpha compared directly).
Alpha weighting matters because straight RGB at near-zero alpha is `premultiplied / alpha`,
which is ill-conditioned: one backend may round such a pixel's alpha to 1/255 and the other to 0.

The budget covers:

- `Rgba16Float` accumulation on the GPU vs `f32` on the CPU (well under 1 code), and
- GPU texture-filter sub-texel precision next to high-contrast texel edges under
  bilinear scaling or rotation (observed up to 3 codes in dark tones).

Observed on Apple Silicon (Metal): max 2 codes on the parity scene. Across every example at
four frames each (about 150M pixels), 5 pixels exceeded 2 codes: four at 3 codes (scaled
text) and one rasterization tie on a rotated edge.

The test skips when no GPU adapter exists. Set `VCR_REQUIRE_GPU=1` in environments
that must have one, so a missing adapter fails instead.

## Known limits

- ASCII cells and overlapping text glyphs are combined inside their rasterizers (in sRGB
  space) before entering the pipeline, the same way an image file's own pixels are baked.
- Custom shader layers only run on the GPU. The software backend renders them
  transparent, prints a warning, and records it in the metadata `warnings` array. The CLI
  never falls back to software for manifests with shader layers; it errors instead.
- The output is 8-bit sRGB per channel.
