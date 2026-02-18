# VCR Standard Operating Procedure (SOP)

This document outlines the mandatory procedures for developing and verifying VCR elements (shaders, manifests, and assets) to ensure high-fidelity, centered, and non-clipping renders.

## 1. Visual Verification Loop

**CRITICAL**: Every new VCR element MUST pass a Visual Verification check before a final render is performed or delivered.

### Step 1: Snapshot Preview

Render a short clip and extract a keyframe snapshot (Frame 0).

```bash
cargo run --release -- render manifests/[id].vcr --output renders/preview.mov
ffmpeg -y -i renders/preview.mov -vf "select=eq(n\,0)" -vframes 1 renders/preview_snapshot.png
```

**Check**: Is the object centered? Is it touching any edges?

### Step 2: Contact Sheet Verification

Generate a 3x3 contact sheet to inspect the animation at start, middle, and end.

```bash
./.tmp_venv/bin/python3 scripts/vcr_contact_sheet.py renders/preview.mov renders/contact_sheet.png
```

**Check**: Does the object remain in frame and stable throughout the entire 10-second duration?

## 2. Framing Requirements (9:16 Vertical)

- **Target Aspect**: 1080x1920 (9:16).
- **Safe Zones**: Maintain at least a 10% margin from all edges.
- **Raymarching Tips**:
  - Use `ro.z` distances of `-15.0` to `-25.0` for safe framing.
  - Adjust `focal_length` (rd calculation) to compensate for camera distance.
  - Prefer "Capsule Chain" or robust global mapping over high-frequency analytic SDFs.

## 3. Tooling Reference

- **`vcr_contact_sheet.py`**: Python helper for mosaic generation.
- **`ffprobe`**: Use signalstats to verify alpha channel transparency.
