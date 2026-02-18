# VCR Visual Verification Suite

VCR is built on the philosophy that **code is the source, but the image is the truth.** This document details the tools and workflows used to guarantee high-fidelity motion graphics.

## The Verification Hierarchy

### 1. Snapshot (The Framing Check)

The snapshot is your first line of defense. It captures Frame 0 and confirms your camera and geometry are correctly initialized.

```bash
# Render preview (1s)
cargo run --release -- render manifests/my.vcr -o renders/preview.mov --duration 1.0
# Extract Frame 0
ffmpeg -y -i renders/preview.mov -vf "select=eq(n\,0)" -vframes 1 renders/snapshot.png
```

### 2. Contact Sheet (The Continuity Check)

A 3x3 mosaic capturing frames at 0%, 12.5%, 25%, 37.5%, 50%, 62.5%, 75%, 87.5%, and 100% of the animation. This catches flickering, clipping, or geometric breakdown that only happens mid-animation.

```bash
# Generate Sheet
./.tmp_venv/bin/python3 scripts/vcr_contact_sheet.py renders/preview.mov renders/contact_sheet.png
```

## Best Practices

- **Vertical Safety**: When rendering for mobile (9:16), always ensure your subject has a 10% vertical gutter. Use the Contact Sheet to verify the object doesn't clip when rotating.
- **Stability**: If you see "blobs" or flickering in the Contact Sheet, your SDF sampling density is too low or your math is unstable. Switch to `Capsule Chain` logic for guaranteed 100% continuity.
- **Alpha Integrity**: Use `ffprobe` with signalstats (as defined in `VCR_SOP.md`) to verify your transparency is clean.

## Mandatory for AI Agents

Any agent modification to a manifest or shader **MUST** include a Contact Sheet as proof of work. halluncinations in geometry are unacceptable.
