# Goth CRT Pass Notes (Relic Hand Orb / Oracle Globe / Sigil Compass)

## What Worked
- Palette direction landed: deep black/charcoal base with restrained crimson accents reads goth without looking neon.
- Motion discipline on `sigil_compass` improved after limiting yaw so the shape stayed readable instead of collapsing edge-on.
- Contact-sheet truth loop caught readability regressions quickly and prevented shipping an unusable angle profile.
- Core thematic variety worked: hand relic, scrying globe, and sigil instrument feel like distinct motifs in one family.

## What Didn’t Work
- First goth pass was underexposed across all three assets; detail disappeared on dark displays.
- CRT treatment was initially too heavy relative to scene luminance, reducing legibility instead of adding texture.
- `relic_hand_orb` silhouette reads more symbolic than anatomical; hand form needs a dedicated refinement pass.
- `oracle_globe` and `sigil_compass` still have narrow dynamic separation between primary body and accent structures in the darkest phases.

## Concrete Fixes Applied In This Round
- Lifted overall output gain while preserving black floor and muted red accents.
- Softened CRT scan modulation amplitude and retained subtle temporal rolloff.
- Kept non-neon accent discipline: red used as punctuation, not fill-light.

## Next Iteration Targets
- Add a low, controlled key fill just above black point for `relic_hand_orb` finger readability.
- Increase local contrast around accent geometry on `oracle_globe` rings and `sigil_compass` ticks.
- Run a dedicated anatomy/stylization pass on `relic_hand_orb` (thumb/palm profile and finger taper hierarchy).
