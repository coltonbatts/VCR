---
description: how to verify VCR render framing using PNG snapshots
---

To ensure a VCR element is correctly framed and not clipped:

1. **Render a short clip** of the manifest (e.g., 1 second):

   ```bash
   cargo run --release -- render manifests/[manifest].vcr --output renders/preview.mov
   ```

2. **Extract a PNG snapshot** from a key frame (e.g., frame 0 or frame 150) using ffmpeg:

   ```bash
   ffmpeg -i renders/preview.mov -vf "select=eq(n\,0)" -vframes 1 renders/preview_snapshot.png
   ```

3. **Generate a Contact Sheet** to inspect the entire animation (start, middle, and end):

   ```bash
   ./.tmp_venv/bin/python3 scripts/vcr_contact_sheet.py renders/preview.mov renders/contact_sheet.png
   ```

4. **Inspect the results** using the `view_file` tool:
   - **Snapshot**: Check for initial centeredness and clipping.
   - **Contact Sheet**: Verify the geometry remains solid, centered, and within the 9:16 safe zones throughout the entire duration.

5. **Iterate** on the shader's camera and scale parameters until both the snapshot and contact sheet look perfect.
