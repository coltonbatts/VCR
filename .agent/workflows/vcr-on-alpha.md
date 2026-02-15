---
description: how to create and verify VCR renders with alpha transparency
---

1. Design the manifest with a transparent background (no solid color layers).
2. Implement chromatic aberration using a 3-layer offset stack (Cyan +2px, Red -2px).
3. Add flickering logic via `glitch(t)` modulators.
4. If using custom shaders, mask the effect area to prevent background alpha bleed.
5. Set `prores_profile: prores4444` and `vendor: apl0` in the encoding block.
6. Verify transparency with:

```bash
ffprobe -v error -f lavfi -i "movie={path}.mov,alphaextract,signalstats" -show_entries frame_tags=lavfi.signalstats.YMIN,lavfi.signalstats.YMAX -of default=nw=1:nk=1 -read_intervals "%+1"
```

7. Check that YMIN is 256 and YMAX is 3760 (ProRes 4444 limited range standards).
