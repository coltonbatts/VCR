# ascii.co.uk Import Guide

Import frame-by-frame ASCII art from [ascii.co.uk](https://www.ascii.co.uk/animated-art/) and render white-on-alpha ProRes 4444 MOV files using Geist Pixel.

**Output:** White glyphs on transparent alpha (ProRes 4444). Re-import any clips that previously rendered as solid white—those used the wrong codec.

## Quick Start

```bash
./scripts/ascii_link_overlay.sh "https://www.ascii.co.uk/animated-art/<slug>-animated-ascii-art.html" -- --width 1920 --height 1080 --fps 24
```

Output: `renders/ascii_co_uk_<slug>_white_alpha.mov` (+ checker preview + PNG still)

## What It Does

1. **Fetches** the ascii.co.uk page and parses the JavaScript frame data
2. **Imports** frames to `assets/animations/ascii_co_uk_<slug>/`
3. **Renders** white glyphs on transparent alpha using Geist Pixel (ProRes 4444)
4. **Generates** checker preview MP4 (dark background) for quick review

## Recently Imported (Release-Ready)

| Animation | URL Slug | Rendered Output |
|-----------|----------|-----------------|
| Ballet | `ballet-animated-ascii-art` | `renders/ascii_co_uk_ballet_animated_ascii_art_white_alpha.mov` |
| Milk Water Droplet | `milk-water-droplet-animated-ascii-art` | `renders/ascii_co_uk_milk_water_droplet_animated_ascii_art_white_alpha.mov` |
| 3D Tunnel | `3d-tunnel-animated-ascii-art` | `renders/ascii_co_uk_3d_tunnel_animated_ascii_art_white_alpha.mov` |
| Launch | `launch-animated-ascii-art` | `renders/ascii_co_uk_launch_animated_ascii_art_white_alpha.mov` |
| Geometric | `geometric-animated-ascii-art-by-rorysimms` | `renders/ascii_co_uk_geometric_animated_ascii_art_by_rorysimms_white_alpha.mov` |
| Daft Punk Bass | `daft-punk-bass-animated-ascii-art` | `renders/ascii_co_uk_daft_punk_bass_animated_ascii_art_white_alpha.mov` |
| Disney Frozen | `disney-frozen-animated-ascii-art` | `renders/ascii_co_uk_disney_frozen_animated_ascii_art_white_alpha.mov` |

## Batch Import

```bash
./scripts/ascii_link_overlay.sh \
  "https://www.ascii.co.uk/animated-art/milk-water-droplet-animated-ascii-art.html" \
  "https://www.ascii.co.uk/animated-art/3d-tunnel-animated-ascii-art.html" \
  "https://www.ascii.co.uk/animated-art/launch-animated-ascii-art.html" \
  -- --width 1920 --height 1080 --fps 24
```

## Options (after `--`)

| Flag | Default | Description |
|------|---------|-------------|
| `--width` | (auto) | Output width |
| `--height` | (auto) | Output height |
| `--fps` | 24 | Frame rate |
| `--cell-width` | 16 | Geist Pixel cell width |
| `--cell-height` | 31 | Geist Pixel cell height |
| `--font` | geist-pixel-line | Font variant (line, square, grid, circle, triangle) |

## Browse ascii.co.uk

Browse animations at: https://www.ascii.co.uk/animated/

URL pattern: `https://www.ascii.co.uk/animated-art/<slug>-animated-ascii-art.html`

## Requirements

- `--features workflow` (script includes this; reqwest for HTTP)
- FFmpeg in PATH
- Geist Pixel fonts bundled in VCR
