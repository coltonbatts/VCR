# Chat Render (`.vcrchat`)

`vcr chat render` turns a tagged plain-text conversation into an animated terminal video.

## Command

```bash
cargo run --release --bin vcr -- chat render \
  --in examples/chat/demo.vcrchat \
  --out renders/chat_demo.mp4 \
  --theme geist-pixel \
  --fps 30 \
  --speed 1.0 \
  --seed 0
```

## Input format (V0)

Use tagged blocks:

- `@user`
- `@assistant`
- `@tool <name>`
- `@system`

Everything until the next tag/directive becomes that block's content.

Optional directives:

- `::pause 400ms`
- `::wait 1.2s`
- `::type fast|normal|slow`

## Copy/paste example

```text
@user
Write a haiku about FFmpeg.

@assistant
Sure. Here you go:
The frames fall in line
Code hums under midnight screens
Video wakes, precise

@tool ffmpeg
command: ffmpeg -i input.mov -vf scale=1920:1080 output.mp4

::pause 600ms

@user
Yes, 9:16.
```

## Notes

- The renderer is software-driven and deterministic for the same `--seed`, script content, and flags.
- Tool blocks render as `[tool: <name>]` plus a boxed body style.
- If Geist Pixel fonts are missing, run `vcr doctor` to confirm local dependencies.
