# VCR Playground: Steerable Motion Instrument

These scenes are designed to be steered entirely via `--set`, with no file edits.

## Run Everything

```bash
./scripts/run_playground.sh
```

The runner executes all 9 presets (24 frames each), writes `renders/playground/index.json`, and
tries to generate scene contact sheets when `ffmpeg` is available.

## Output Structure

```text
renders/playground/
  index.json
  instrument_typography/
    default/
      frame_000000.png
      ...
      preview.metadata.json
    aggressive/
    minimal/
    contact_sheet.png            # optional (ffmpeg only)
  instrument_grid/
    default/
    aggressive/
    minimal/
    contact_sheet.png            # optional (ffmpeg only)
  instrument_logo_reveal/
    default/
    aggressive/
    minimal/
    contact_sheet.png            # optional (ffmpeg only)
```

`index.json` entries include:
- scene name
- preset name
- output folder
- key params used
- relative path to `preview.metadata.json`

## Presets (Runner Source of Truth)

The presets are defined in `/Users/coltonbatts/Desktop/VCR/scripts/run_playground.sh` in the `run_preset` calls.
Edit those param values to tune the playground.

## 1) Typography Instrument (`examples/instrument_typography.vcr`)

### default
```bash
cargo run --release --bin vcr -- preview examples/instrument_typography.vcr --image-sequence --frames 24 -o renders/playground/instrument_typography/default --set scan_speed=1.0 --set accent_color=#38DCEF --set noise_intensity=0.30
```

### aggressive
```bash
cargo run --release --bin vcr -- preview examples/instrument_typography.vcr --image-sequence --frames 24 -o renders/playground/instrument_typography/aggressive --set scan_speed=2.4 --set accent_color=#FF4C73 --set noise_intensity=1.20
```

### minimal
```bash
cargo run --release --bin vcr -- preview examples/instrument_typography.vcr --image-sequence --frames 24 -o renders/playground/instrument_typography/minimal --set scan_speed=0.55 --set accent_color=#A7F5DA --set noise_intensity=0.08
```

## 2) Grid Instrument (`examples/instrument_grid.vcr`)

### default
```bash
cargo run --release --bin vcr -- preview examples/instrument_grid.vcr --image-sequence --frames 24 -o renders/playground/instrument_grid/default --set grid_scale=1.0 --set jitter=0.30 --set contrast=1.0
```

### aggressive
```bash
cargo run --release --bin vcr -- preview examples/instrument_grid.vcr --image-sequence --frames 24 -o renders/playground/instrument_grid/aggressive --set grid_scale=1.55 --set jitter=1.40 --set contrast=2.0
```

### minimal
```bash
cargo run --release --bin vcr -- preview examples/instrument_grid.vcr --image-sequence --frames 24 -o renders/playground/instrument_grid/minimal --set grid_scale=0.72 --set jitter=0.05 --set contrast=0.65
```

## 3) Logo Reveal Instrument (`examples/instrument_logo_reveal.vcr`)

### default
```bash
cargo run --release --bin vcr -- preview examples/instrument_logo_reveal.vcr --image-sequence --frames 24 -o renders/playground/instrument_logo_reveal/default --set reveal_duration=1.0 --set reveal_bias=0.70 --set accent_color=#F56A3A
```

### aggressive
```bash
cargo run --release --bin vcr -- preview examples/instrument_logo_reveal.vcr --image-sequence --frames 24 -o renders/playground/instrument_logo_reveal/aggressive --set reveal_duration=0.50 --set reveal_bias=0.95 --set accent_color=#FF2C84
```

### minimal
```bash
cargo run --release --bin vcr -- preview examples/instrument_logo_reveal.vcr --image-sequence --frames 24 -o renders/playground/instrument_logo_reveal/minimal --set reveal_duration=1.90 --set reveal_bias=0.25 --set accent_color=#7BE8D4
```

## Friction Log

- `preview --image-sequence` without `-o` always wrote to `renders/preview`, so running multiple scenes silently overwrote frames and metadata.
  Fix: default directory is now manifest-scoped (`renders/<manifest>_preview`).
- `vcr explain` printed full resolved params even when every value matched overrides, so the useful signal was buried.
  Fix: text output now emphasizes non-default overrides and non-default resolved params, plus total param count.
- `vcr build` output was crowded by ffmpeg banner/progress logs, which made output paths harder to scan.
  Fix: ffmpeg now runs with hidden banner and error-only log level.

## Clean Outputs

```bash
rm -rf renders/playground
```
