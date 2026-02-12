# ASCII Animation Engine

This module adds a modular frame-by-frame ASCII animation system for VCR.

## Asset Layout

Use:

```
assets/animations/<animation-name>/
  metadata.json
  0001.txt
  0002.txt
  ...
```

- Frames are loaded from `.txt` and `.ans`
- Numeric filename prefixes control playback order

## Core API

- `AnimationManager`: imports and stores clips
- `AnimationLayer`: playback, fit, blend mode, color, and filter settings
- `Renderer::render_frame_rgba_with_animation_layer(...)`: renders VCR frame + overlays one animation layer

## Injecting Into the Render Pipeline

```rust
let mut renderer = Renderer::new_software(&environment, &manifest.layers, scene)?;

let mut manager = AnimationManager::new();
manager.load_from_assets_root(
    "assets/animations",
    "demo_wave",
    AnimationImportOptions {
        source_fps: 12,
        strip_ansi_escape_codes: true,
    },
)?;

let layer = AnimationLayer::new("demo_wave");
let rgba = renderer.render_frame_rgba_with_animation_layer(frame_index, &manager, &layer)?;
```

This is equivalent to injecting an ASCII layer after scene composition and before encoding.

## Boutique Filter

`BoutiqueFilter` applies deterministic analog-style effects:

- `drop_frame_probability`: random frame drop (flicker)
- `brightness_jitter`: random brightness modulation
- `horizontal_shift_px`: random horizontal jitter

All effects are deterministic from `seed` + `frame_index`.

## Credits Strategy

Attribution is stored per animation in `metadata.json`.

Recommended fields:

- `artist`
- `source_url`
- `license`
- `tags`
- `credit` (optional override string)

At runtime, use `AnimationManager::credits_manifest()` to emit a normalized credits report.

## High-Level Workflows

### URL -> White Alpha Overlay (ascii.co.uk)

VCR provides a formalized workflow for importing and rendering animated art from `ascii.co.uk`.

**Command:**

```bash
./scripts/ascii_link_overlay.sh <URL> -- --width 1920 --height 1080
```

**What it does:**

1. **Imports**: Fetches the URL, parses the JavaScript frames, and writes them to `assets/animations/ascii_co_uk_<slug>/`.
2. **Trims**: Automatically skips leading blank frames (source often has 30-60 empty frames).
3. **Renders**: Produces a ProRes 4444 `.mov` with white text and alpha transparency.
4. **Validates**: Generates a `.png` still and a `*_checker.mp4` (composite over dark grey) for quick review.
