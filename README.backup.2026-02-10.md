# VCR (Video Component Renderer)

VCR is a headless, local-first motion graphics renderer.
It compiles declarative scene manifests (`.vcr`) into frames or video using a GPU-first pipeline (`wgpu`) with automatic CPU fallback (`tiny-skia`).

Repository: [https://github.com/coltonbatts/VCR](https://github.com/coltonbatts/VCR)

## Features

- YAML scene manifests with stable IDs and readable animation data.
- GPU-first rendering with deterministic CPU fallback.
- Procedural layers (`solid_color`, `gradient`) and image/asset layers (`image.path` or `source_path`).
- Expression-driven animation (`t`, params, easing/math helpers, deterministic `noise1d`, `env`).
- Global `params`, reusable `modulators`, and inheritable `groups`.
- Timing controls per layer/group: `start_time`, `end_time`, `time_offset`, `time_scale`.
- Fast iteration workflows:
  - `preview` (scaled, short, frame-window)
  - `render-frame` (single PNG)
  - `render-frames` (PNG sequence)
  - `watch` (rebuild on manifest change)
- Diagnostics:
  - `check` for structural validation
  - `lint` for common scene issues
  - `dump` for resolved scene state at a frame/time

## Requirements

- Rust stable toolchain
- FFmpeg on `PATH` for `.mov` output (`build` and default `preview` output)

If FFmpeg is missing, VCR fails with a helpful error message. Use image sequence output (`--image-sequence`) when FFmpeg is unavailable.

## Build

```bash
cargo build
```

## Golden Path

1. Validate your manifest:

```bash
cargo run -- check sanity_check.vcr
```

2. Run a fast preview (half-res, short duration):

```bash
cargo run -- preview sanity_check.vcr --image-sequence -o preview_frames
```

3. Iterate while editing:

```bash
cargo run -- watch sanity_check.vcr --image-sequence -o preview_frames
```

4. Build final video:

```bash
cargo run -- build sanity_check.vcr -o output.mov
```

## CLI

```bash
cargo run -- --help
```

Commands:

- `check <manifest>`: validate and summarize the manifest.
- `lint <manifest>`: flag common scene issues.
- `dump <manifest> [--frame N | --time SECONDS]`: print resolved layer state.
- `build <manifest> -o <output.mov> [--start-frame N] [--frames N | --end-frame N]`.
- `preview <manifest> [--scale 0.5] [--frames N] [--image-sequence]`.
- `render-frame <manifest> --frame N -o frame.png`.
- `render-frames <manifest> --start-frame N --frames N -o frames_dir`.
- `watch <manifest> [preview flags]`: rebuild preview when the manifest changes.

Every render path prints timing breakdowns: parse, layout, render, encode.

## Manifest Notes

- Existing manifests remain valid (defaults are additive).
- New steerability blocks:
  - `params`: named global scalar controls.
  - `modulators`: reusable expression sources.
  - `groups`: transform/timing inheritance.
- Expressions support:
  - `clamp`, `lerp`, `smoothstep`, `easeInOut`
  - `sin`, `cos`, `abs`
  - deterministic `noise1d`
  - envelope helper `env`

See `STEERABILITY.md` and the examples under `examples/`.
For repeatable brief-driven workflows, see `VCR_BRIEF_TEMPLATE.md`.

Image layers can be declared in either form:

```yaml
layers:
  - id: logo
    image:
      path: "assets/logo.png"
```

```yaml
layers:
  - id: logo_legacy
    source_path: "assets/logo.png"
```

## Included Examples

- `examples/global_params_scene.vcr`
- `examples/envelope_scene.vcr`
- `examples/group_wobble_scene.vcr`
- `examples/vcr_title_card.vcr`
- `examples/lower_third_brief_skeleton.vcr`
