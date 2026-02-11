# ASCII Capture (`vcr ascii capture`)

`vcr ascii capture` is a non-canonical capture path that turns animated ASCII sources into a ProRes MOV.

- Existing canonical path (`vcr ascii render`) is unchanged and remains deterministic.
- Capture mode is best-effort when upstream tools/streams are unstable or version-dependent.

## Command

```bash
vcr ascii capture --source <source> --out <output.mov> [options]
```

## Sources

1. Remote stream source:

```bash
--source ascii-live:earth
```

Uses:

```bash
curl -L --no-buffer https://ascii.live/earth
```

2. Local media via Chafa:

```bash
--source chafa:/path/to/input.gif
```

Uses `chafa` with fixed size and monochrome output (`--colors=none`), plus optional stable flags when available (`--symbols=ascii`, `--animate=on`, `--clear`).

## ProRes Encoder

Frames are rasterized to RGBA in software, then encoded with FFmpeg `prores_ks` using ProRes 422 and 10-bit 4:2:2 output:

```bash
ffmpeg -f rawvideo -pix_fmt rgba -s:v <WxH> -r <fps> -i - \
  -an -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le <output.mov>
```

## Options

- `--fps` (default `30`)
- `--duration` seconds (default `5`)
- `--frames` max frame count (overrides duration-derived count)
- `--size` grid size `COLSxROWS` (default `80x40`)
- `--font-path` path to `.ttf` (default Geist Pixel Line font in repo assets)
- `--font-size` (default `16`)
- `--tmp-dir` optional working directory for ffmpeg
- `--debug-txt-dir` optional directory for normalized `.txt` frame dumps
- `--symbol-remap` symbol remap mode: `none`, `density`, `equalize` (default `equalize`)
- `--symbol-ramp` output symbol ramp (default `.:-=+*#%@`)
- `--dry-run` print planned capture/encode pipeline without running

## Examples

```bash
vcr ascii capture --source ascii-live:earth --out earth.mov --duration 8 --fps 30 --size 120x45
```

```bash
vcr ascii capture --source chafa:./assets/statue.gif --out statue.mov --duration 6 --fps 24 --size 120x45
```

Golden manual command:

```bash
vcr ascii capture --source chafa:./assets/welcome_terminal_scene.gif --out renders/manual_ascii_capture.mov --frames 180 --fps 30 --size 120x45 --font-size 16
```

## Parser & Capture Notes

- Capture loop samples at fixed FPS and stores exactly the requested number of frames.
- ANSI parsing is best-effort:
  - frame boundaries are inferred from clear-screen and cursor-home sequences.
  - if boundaries are ambiguous, the latest complete screen snapshot is reused at sample ticks.
- Output frames are normalized to fixed `COLSxROWS` with stable `\n` line joining.

## Determinism Stance

- `vcr ascii render` canonical behavior (quantization/dither/hysteresis/hash pipeline) is unchanged.
- `vcr ascii capture` can vary across external tool versions, stream behavior, and font files unless all inputs/tools are pinned.

## Safety / Licensing

- Do not embed third-party copyrighted media without rights.
- Prefer user-owned, licensed, or public-domain GIF/video inputs for `chafa:` workflows.
