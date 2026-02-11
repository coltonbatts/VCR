# ASCII History

## 1. Teletypes and Mechanical Constraints

Early text terminals and teleprinters transmitted character codes over serial links with constrained bandwidth and fixed-width print heads.
Rendering was mechanical or electromechanical:

- Character generator hardware selected one symbol at a time
- No arbitrary pixel addressing
- Layout was row-major line feed and carriage return

ASCII art emerged from this constraint model: shape approximation by character placement.

## 2. Line Printers and Overstrike Techniques

Before raster displays were common, operators used overstriking (printing multiple characters at the same position with carriage returns) to increase apparent darkness.
This was an early density-composition method: multiple strokes approximated grayscale through cumulative ink coverage.

## 3. BBS Era and CP437 / Extended Glyph Sets

Bulletin Board Systems popularized text graphics on terminal emulators.
Although strict ASCII is 7-bit, many systems used code page 437 extensions:

- Box drawing characters
- Block elements
- Pseudographic symbols

Mechanism: remote systems sent byte streams; terminal interpreted bytes using local code page mapping.
Output consistency depended on matching code page assumptions.

## 4. ANSI Escape Codes and ANSI Art

ANSI X3.64 / ECMA-48 escape sequences introduced cursor control and color attributes.
Core pattern:

- `ESC[...m` for color/style attributes
- `ESC[row;colH` for cursor placement
- Incremental writes over a persistent text buffer

ANSI art used these controls to place colored characters at absolute coordinates. This was effectively a 2D retained-mode text framebuffer with attribute state.

## 5. ASCII Warez Intros and Scripted Text Effects

DOS-era intros used text mode (`80x25`, hardware character cells in video memory).
Technical pattern:

- Direct writes to VGA text memory (`B800:0000`)
- Per-cell tuple: character byte + attribute byte
- Timing loops for fades, scrolls, and reveals

This was deterministic on fixed hardware clocks but diverged across CPU speeds without calibrated timing.

## 6. mIRC and IRC Client Art Culture

IRC clients rendered monospaced text with mIRC control codes for colors/styles.
Mechanism:

- Inline control bytes switched foreground/background color indices
- Rendering remained cell-based and stream-oriented
- Animation used repeated message updates or pseudo-screen rewrites

Consistency depended on client parsing compatibility and flood limits.

## 7. Demoscene Text Mode Experiments

Demoscene productions treated text mode as constrained raster:

- Custom character sets
- Per-frame character/attribute updates
- Synchronization to music timers

Optimization strategy: update only changed cells (dirty rectangles) to fit bandwidth and cycle budgets.

## 8. AAlib: Structured ASCII Rasterization

AAlib (late 1990s) formalized image/video-to-ASCII conversion.
Implementation pattern:

- Convert source image to luminance
- Downsample to character grid
- Map luminance + edge features to character tables
- Emit terminal output with optional dithering

AAlib separated sampling and display backends, enabling deterministic software conversion independent of GPU APIs.

## 9. libcaca: Color + Dithering + Multiple Backends

libcaca expanded text graphics with color-aware dithering and output drivers.
Typical pipeline:

- Decode source image
- Dither to terminal color palette
- Choose glyphs from density/edge tables
- Render through ncurses/X11/Win32/etc.

Implementation emphasis: backend abstraction and optimized diff rendering (minimize terminal writes).

## 10. FFmpeg ASCII Filters and CLI Video Workflows

FFmpeg pipelines enabled practical ASCII video conversion in scripted environments.
Common mechanics:

- Decode frames with deterministic codec parameters
- Scale to character-grid-compatible dimensions
- Convert to grayscale/luma
- Apply ASCII mapping filter or external post-process
- Encode output as text stream or rasterized video

Stability depends on fixed FFmpeg build options, fixed scaling algorithms, and fixed color-range handling.

## 11. Modern Terminal Animation

Contemporary terminal animation combines:

- UTF-8 transport
- ANSI color / truecolor escape sequences
- Alternate screen buffers
- Cursor addressing for in-place updates

Primary engineering issue is not capability but determinism under variable terminal emulators, fonts, and refresh behavior.

## 12. Historical Implementation Patterns Relevant to VCR

Recurring mechanisms across eras:

- Fixed cell grids as the atomic display primitive
- Density ranking of symbols as grayscale approximation
- State-based screen updates rather than full reprints when possible
- Deterministic software rasterization preferred over device-dependent rendering
- Tight control over encoding, font assumptions, and timing for reproducibility
