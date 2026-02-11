# ASCII Capture Milestone (Earth Overlay)

Status: working and usable for editor overlays.

## What Works

- `vcr ascii capture` captures `ascii-live:earth` and writes ProRes MOV.
- Output is centered with configurable safe margins (`--fit-padding`).
- Symbol remap reduces overuse of heavy symbols (default: `equalize`).
- Output codec/pixel format is editor-friendly ProRes 422 10-bit (`yuv422p10le`).

## Current Quality

- Good enough for compositing and creative overlays.
- Known issue: frame jitter can appear because source stream updates are best-effort parsed from terminal ANSI output.
- This path is intentionally non-canonical; external tools/streams affect output stability.

## Recommended Command

```bash
cargo run --quiet --bin vcr -- ascii capture \
  --source ascii-live:earth \
  --out renders/earth_v3.mov \
  --duration 8 --fps 30 --size 120x45 \
  --fit-padding 0.22 \
  --symbol-remap equalize \
  --symbol-ramp ".,:;irsXA253hMHGS#9B&@"
```

## Integration Notes

- For overlays in NLEs, set blend mode with white-on-black source or key the black background.
- If more border is needed for title-safe regions, increase `--fit-padding` (e.g. `0.28`).
- If density feels too heavy/light, adjust `--symbol-ramp`.

## Next Iteration (Optional)

- Add temporal smoothing in capture parser output to reduce jitter.
- Add optional alpha output mode for direct compositing.
