# VCR Motion Graphics Brief Template

Use this brief format to generate repeatable `.vcr` skeletons fast.

## Prompt Structure

```text
VCR BRIEF:

[TITLE/NAME]
Lower Third: "Welcome to First Principles"

[AESTHETIC]
Black background, white text, Geist Pixel, pixels visible. Terminal Core (no neon).

[CONTENT]
- Text: "Welcome to First Principles"
- Font: Geist Pixel
- Additional elements: [graphics, shapes, accents]

[MOTION]
- Text enters from left side
- Drops to bottom of frame
- Motion feel: slow Bezier, organic easing
- Duration: 5 seconds

[RESOLUTION & FORMAT]
- 2560x1440, 24fps
- Duration: 120 frames
- Output: ProRes 4444 with alpha

[NOTES]
- [timing/color/iteration notes]
```

## Manifest Skeleton (Current Engine-Compatible)

```yaml
environment:
  resolution:
    width: 2560
    height: 1440
  fps: 24
  duration:
    frames: 120

params:
  entrance_delay: 0
  text_speed: 1.0
  drop_distance: 560

layers:
  - id: lower_third_text
    z_index: 2
    pos_x: "-1200 + smoothstep(8 + entrance_delay, 56 + entrance_delay, t * text_speed) * 1400"
    pos_y: "420 + smoothstep(20 + entrance_delay, 76 + entrance_delay, t) * drop_distance"
    opacity: "smoothstep(8, 28, t) * (1.0 - smoothstep(108, 120, t))"
    image:
      path: "assets/lower_third_text.png"
```

## Workflow

1. Write the brief.
2. Generate a `.vcr` skeleton.
3. Render to project outputs:

```bash
cargo run --release -- build examples/[name].vcr -o exports/[name].mov
```

4. Tweak expressions/params and re-render.

For quick iteration:

```bash
cargo run --release -- watch examples/[name].vcr --image-sequence -o /tmp/[name]_watch
```

## Common Adjustments

- Faster/slower motion: lower/higher frame windows in `smoothstep(...)`.
- Softer/harder fades: adjust opacity `smoothstep` ranges.
- Staggered timing: add offsets (`+ 8`, `+ 16`) per layer.
- Limited palette: keep 1 background, 1 accent, 1 text color.

## Engine Notes (Important)

- If you want true transparency in output, do **not** add an opaque background layer.
- Current VCR supports `image.path` and legacy `source_path` for bitmap layers.
- Procedural layers are full-frame sources transformed by position/scale/rotation; they do not currently take per-layer `width`/`height` fields.
