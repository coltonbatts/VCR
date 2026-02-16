# Trailer Render Commands

All commands were run from:

```bash
cd /Users/coltonbatts/Desktop/VCR
```

## 1) Prompt-Gate Normalization (`vcr prompt`)

```bash
cargo run -- prompt --text "Vertical 1080x1920 trailer title card. Text VCR in Geist Pixel only. White text only on transparent alpha background. 24fps. 6 seconds. Character-by-character reveal then hard hold. No color no gradients no textures." -o manifests/trailer/prompt/title_card.normalized.yaml
cargo run -- prompt --text "Vertical 1080x1920 trailer subtitle card. Text Video Component Renderer. White only on transparent alpha background. 24fps. 6 seconds. Single-line wipe in from left with linear motion and hold. Geist Pixel only." -o manifests/trailer/prompt/subtitle_card.normalized.yaml
cargo run -- prompt --text "Vertical 1080x1920 feature callout clip. Text Deterministic. White on transparent alpha only. 24fps. 5 seconds. Snap/step motion in then hold. No easing curves. Geist Pixel only." -o manifests/trailer/prompt/feature_deterministic.normalized.yaml
cargo run -- prompt --text "Vertical 1080x1920 feature callout clip. Text CLI-native. White on transparent alpha only. 24fps. 5 seconds. Snap/step motion in then hold. No easing curves. Geist Pixel only." -o manifests/trailer/prompt/feature_cli_native.normalized.yaml
cargo run -- prompt --text "Vertical 1080x1920 feature callout clip. Text ProRes on alpha. White on transparent alpha only. 24fps. 5 seconds. Snap/step motion in then hold. No easing curves. Geist Pixel only." -o manifests/trailer/prompt/feature_prores_on_alpha.normalized.yaml
cargo run -- prompt --text "Vertical 1080x1920 ASCII grid animation. Monospaced cell grid resolves into a solid white block then cuts to text VCR. White only on transparent alpha background. 24fps. 7 seconds. Terminal-native, geometric, no blur/glow/texture." -o manifests/trailer/prompt/ascii_grid.normalized.yaml
```

## 2) Manifest Validation (`check` + `lint`)

```bash
for f in manifests/trailer/*.vcr; do
  cargo run -- check "$f"
  cargo run -- lint "$f"
done
```

## 3) Preview Renders (PNG Sequences, Software Backend)

```bash
for f in manifests/trailer/*.vcr; do
  name=$(basename "$f" .vcr)
  cargo run -- render-frames "$f" --backend software --start-frame 0 --frames 9999 -o "renders/trailer/previews/$name"
done
```

## 4) Determinism Hashes (Software Backend)

```bash
for f in manifests/trailer/*.vcr; do
  name=$(basename "$f" .vcr)
  cargo run -- determinism-report "$f" --backend software --frame 0 --json > "renders/trailer/validation/determinism/${name}.json"
done
```

Representative active-frame hashes:

```bash
for f in manifests/trailer/*.vcr; do
  name=$(basename "$f" .vcr)
  frame=24
  case "$name" in
    ascii_grid_resolve_to_vcr) frame=96 ;;
    subtitle_card_wipe) frame=60 ;;
    title_card_vcr) frame=40 ;;
    feature_*) frame=24 ;;
  esac
  cargo run -- determinism-report "$f" --backend software --frame "$frame" --json > "renders/trailer/validation/determinism_active/${name}.json"
done
```

## 5) Final ProRes 4444 MOV Renders

```bash
for f in manifests/trailer/*.vcr; do
  name=$(basename "$f" .vcr)
  cargo run -- build "$f" --backend software -o "renders/trailer/final/${name}.mov"
done
```

A transient repo compile issue in `src/library.rs` interrupted one `cargo run` batch. Remaining renders were completed with the already-built CLI binary:

```bash
target/debug/vcr build manifests/trailer/subtitle_card_wipe.vcr --backend software -o renders/trailer/final/subtitle_card_wipe.mov
target/debug/vcr build manifests/trailer/title_card_vcr.vcr --backend software -o renders/trailer/final/title_card_vcr.mov
```

## 6) Final Output Verification (`ffprobe`)

```bash
for f in renders/trailer/final/*.mov; do
  ffprobe -v error -select_streams v:0 \
    -show_entries stream=codec_name,profile,pix_fmt,width,height,r_frame_rate,avg_frame_rate \
    -of default=noprint_wrappers=1 "$f"
done
```

Final MOV outputs:

- `renders/trailer/final/title_card_vcr.mov`
- `renders/trailer/final/subtitle_card_wipe.mov`
- `renders/trailer/final/feature_deterministic.mov`
- `renders/trailer/final/feature_cli_native.mov`
- `renders/trailer/final/feature_prores_on_alpha.mov`
- `renders/trailer/final/ascii_grid_resolve_to_vcr.mov`
