---
name: vcr-ascii-pipeline
description: Build and validate VCR ASCII workflows including stage/capture/library imports and URL-to-overlay outputs. Use when generating terminal-cinema videos, capturing ASCII sources, listing curated ASCII assets, or producing white-on-alpha overlays from ascii.co.uk links.
---

# VCR ASCII Pipeline
Run repeatable ASCII-to-video pipelines using VCR’s ASCII tooling.

## Core Commands
- Library discovery:
  - `vcr ascii library`
  - `vcr ascii sources`
- Stage render (`.vcrtxt` transcript):
  - `vcr ascii stage --in <input>.vcrtxt --out <out>.mp4 --preset <preset> --seed <n>`
- Capture source to MOV:
  - `vcr ascii capture --source <source_id> --out <out>.mov --duration <sec> --fps <fps> --size <cols>x<rows>`
- URL to white-alpha overlay:
  - `scripts/ascii_link_overlay.sh "<ascii_co_uk_url>" -- --width 1920 --height 1080 --fps 24`

## Pipeline Workflow
1. Select source mode (library, stage transcript, remote/source capture, or URL overlay).
2. Generate a short preview output first.
3. Validate frame 0 is non-blank and transparency is correct (if alpha workflow).
4. Produce final render artifact (`.mov`/`.mp4`) and sidecar metadata if applicable.

## Quality Checks
- Expected output file exists and is readable.
- For alpha overlays, foreground is white and background alpha is transparent.
- Timing/fps matches requested settings.
- Generated metadata (if emitted) contains expected source attribution.

## Failure Handling
- Blank leading frames: enable/confirm trimming in URL overlay workflow.
- Source parse issues: switch to curated `library:*` source to isolate parser/input issues.
- Performance or backend issues: reduce duration/size first, then scale up.
