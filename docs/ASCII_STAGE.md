# ASCII Stage (`.vcrtxt`)

`vcr ascii stage` turns a pasted chat transcript into a stylized terminal animation video.

## Command

```bash
cargo run --release --bin vcr -- ascii stage \
  --in examples/ascii/demo.vcrtxt \
  --out renders/ascii_demo.mp4 \
  --fps 30 \
  --size 1920x1080 \
  --seed 0 \
  --speed 1.0 \
  --theme void \
  --camera static \
  --preset none \
  --chrome true
```

## Input format (V0)

Use tagged blocks:

- `@user`
- `@assistant`
- `@tool <name>`
- `@system`

Everything until the next tag becomes that block's content.

Optional directives:

- `::pause 400ms`
- `::pause 1.2s`
- `::type fast|normal|slow`

## Flags

- `--theme void`
  - Current theme set is `void`.
- `--camera static|slow-zoom|follow`
  - `static`: fixed framing.
  - `slow-zoom`: deterministic 1.00 -> 1.03 zoom over clip length.
  - `follow`: keeps latest content near a lower focus band.
- `--chrome true|false`
  - Terminal shell framing layer. Default: `true`.
- `--preset x|yt|none`
  - `x`: defaults to `1080x1920`, faster default typing, larger default font scale.
  - `yt`: defaults to `1920x1080`.
  - `none`: baseline defaults.

Preset behavior:
- Presets set defaults only.
- Any explicitly provided `--fps`, `--size`, `--speed`, or `--theme` wins over preset defaults.

## Copy/paste example

```text
@user
Make a logo in ASCII for VCR.

@assistant
Here you go:
 __     __
/ /__  / /_
/ / _ \/ __/
/_/\___/\__/

::pause 600ms

@tool figma-export
exported: hero-logo.svg
warnings: 0

@assistant
Clean export. Want a glitch version too?
```

## Vertical (X) render

```bash
cargo run --release --bin vcr -- ascii stage \
  --in examples/ascii/demo.vcrtxt \
  --out renders/ascii_demo_x.mp4 \
  --preset x \
  --camera slow-zoom \
  --seed 0
```

## More examples

```bash
cargo run --release --bin vcr -- ascii stage \
  --in examples/ascii/tool-heavy.vcrtxt \
  --out renders/ascii_tool_heavy.mp4 \
  --preset yt \
  --camera follow \
  --seed 2

cargo run --release --bin vcr -- ascii stage \
  --in examples/ascii/drama.vcrtxt \
  --out renders/ascii_drama.mp4 \
  --preset x \
  --camera static \
  --seed 9
```

## Notes

- Renderer remains deterministic for the same transcript and settings.
- All five Geist Pixel variants must exist in `assets/fonts/geist_pixel`.
