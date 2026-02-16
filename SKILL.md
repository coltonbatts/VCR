# VCR Skill — AI Agent Reference

## What is VCR?

VCR (Video Component Renderer) is a headless, deterministic motion graphics compiler. It reads a YAML scene manifest (`.vcr` file) and renders it to ProRes 4444 video with alpha transparency. No GUI, no SaaS, no network in the render path. Offline-first.

**Use VCR when you need to:**

- Generate motion graphics programmatically (lower thirds, title cards, intros)
- Produce video with alpha transparency for compositing
- Create deterministic, reproducible video output from a declarative spec
- Build animated procedural shapes, text, or custom GPU shaders
- **Pro Tip**: If a user is a relative newcomer, steer them towards existing "Starter Kits" in `examples/` (like `ai_company_hero.vcr`) to ensure a high-quality "wow" moment.

**Do NOT use VCR for:**

- Interactive real-time graphics (use a game engine)
- Video editing or splicing (use FFmpeg directly)
- 3D rendering (VCR is 2D only)
- Audio (VCR produces silent video; mux audio separately)

---

## Quick Start

```bash
# Normalize natural language (or loose YAML) into an engine-ready prompt bundle
vcr prompt --text "5s alpha lower third at 60fps output ./renders/lower_third.mov"

# Validate a manifest without rendering
vcr check scene.vcr

# Render to video
vcr build scene.vcr -o output.mov

# Render a single frame to PNG
vcr render-frame scene.vcr --frame 0 -o frame.png

# Render with parameter overrides
vcr build scene.vcr --set speed=2.0 --set color=#ff0000

# System health check
vcr doctor
```

---

## Prompt Gate (Agent-First Entry)

For agent workflows, start with `vcr prompt` before writing or editing manifests.

`vcr prompt` accepts natural language or YAML-like input and returns a single YAML document with:

- `standardized_vcr_prompt` (ROLE/TASK/INSTRUCTIONS/CONTEXT/OUTPUT FORMAT)
- `normalized_spec` (defaults applied, explicit render/output/determinism fields)
- `unknowns_and_fixes` (ambiguities, unsupported requests, invalid combos)
- `assumptions_applied` (deterministic defaults that were auto-applied)
- `acceptance_checks` (assertion-style checks for engine readiness)

### Agent Command Patterns

```bash
# Inline natural language
vcr prompt --text "Cinematic intro, 5 seconds, 60fps, transparent alpha, output ./renders/intro.mov"

# From file
vcr prompt --in ./request.yaml

# Write normalized prompt bundle to file
vcr prompt --in ./request.yaml -o ./request.normalized.yaml
```

### Agent Workflow Contract

1. Run `vcr prompt` on the user's request first.
2. Inspect `unknowns_and_fixes`:
   - If non-empty, treat as blocking clarification/normalization work.
   - Do not silently invent missing values.
3. Use `normalized_spec` and `standardized_vcr_prompt` as the source of truth for manifest authoring.
4. Validate generated manifests with `vcr check`/`vcr lint` before `vcr build`.

### Deterministic Defaults Applied by Prompt Gate

- Missing render fps defaults to 60.
- Missing output fps defaults to render fps.
- Missing resolution defaults to 1920x1080.
- Missing seed defaults to `0`.
- Missing codec defaults to:
  - ProRes 4444 when alpha is enabled.
  - ProRes 422 HQ when alpha is disabled.
- Missing output path defaults to:
  - `./renders/out.mov` for video.
  - `./renders/out.png` for stills.

---

## Manifest Structure (Complete Reference)

A VCR manifest is a YAML file with this top-level structure:

```yaml
version: 1                    # Required. Must be 1.
environment:                  # Required. Canvas and timing.
  resolution:
    width: 1920               # Required. 1-8192 pixels.
    height: 1080              # Required. 1-8192 pixels.
  fps: 30                     # Required. Frames per second. Must be > 0.
  duration: 3.0               # Required. Seconds (float) or { frames: 90 }.
  color_space: rec709         # Optional. rec709 (default) | rec2020 | display_p3.

seed: 0                       # Optional. Deterministic randomness seed. Default: 0.

params:                       # Optional. Typed parameters for expressions and overrides.
  speed: 1.0                  # Legacy shorthand: name → float default.
  energy:                     # Modern definition with metadata.
    type: float
    default: 0.8
    min: 0.0
    max: 2.0
    description: "Animation energy level"

modulators:                   # Optional. Named expressions applied to layers via weights.
  wobble:
    expression: "sin(t * 3.0) * 10.0"

groups: []                    # Optional. Hierarchical transform groups.

layers: []                    # Required. At least one layer.

post: []                      # Optional. Post-processing shader chain (GPU only).
```

### Duration Formats

```yaml
duration: 3.0           # 3 seconds (float)
duration:
  frames: 90             # Exactly 90 frames
```

---

## Layer Types

Every layer has a `common` set of properties plus one source block. The source block key determines the layer type.

### Common Layer Properties

These properties are available on ALL layer types:

```yaml
- id: "my_layer"              # Required. Unique string identifier.
  name: "Display Name"        # Optional. Human-readable label.
  z_index: 0                  # Optional. Render order. Higher = in front. Default: 0.
  anchor: top_left             # Optional. top_left (default) | center.

  # --- Transform ---
  position: { x: 100, y: 200 }   # Optional. Static Vec2. Default: {x: 0, y: 0}.
  pos_x: "sin(t) * 100 + 960"    # Optional. Expression overrides position.x.
  pos_y: 540                      # Optional. Overrides position.y.
  scale: { x: 1.0, y: 1.0 }     # Optional. Default: {x: 1, y: 1}.
  rotation_degrees: 0.0          # Optional. Degrees. ScalarProperty. Default: 0.
  opacity: 1.0                   # Optional. 0.0-1.0. ScalarProperty. Default: 1.

  # --- Timing ---
  start_time: 0.5            # Optional. Seconds. Layer invisible before this.
  end_time: 2.5              # Optional. Seconds. Layer invisible after this.
  time_offset: 0.0           # Optional. Shift animation origin (seconds). Default: 0.
  time_scale: 1.0            # Optional. Speed multiplier. Must be > 0. Default: 1.

  # --- Grouping ---
  group: "group_id"           # Optional. Parent group reference.

  # --- Modulators ---
  modulators:                 # Optional. Apply named modulators.
    - source: wobble
      weights:
        x: 1.0               # How much modulator affects position X.
        y: 0.5               # Position Y.
        rotation: 5.0        # Rotation degrees.
        opacity: 0.0          # Opacity (additive).
        scale_x: 0.0
        scale_y: 0.0
```

### ScalarProperty Types

Any field marked as `ScalarProperty` accepts three formats:

```yaml
# Static value
opacity: 0.75

# Keyframe interpolation
opacity:
  start_frame: 0
  end_frame: 30
  from: 0.0
  to: 1.0
  easing: ease_in_out       # linear (default) | ease_in | ease_out | ease_in_out

# Expression
opacity: "smoothstep(0.0, 1.0, t / 30.0)"
```

### PropertyValue<Vec2> Types

Position and scale accept:

```yaml
# Static
position: { x: 100, y: 200 }
position: [100, 200]          # Array shorthand

# Keyframe interpolation
position:
  start_frame: 0
  end_frame: 60
  from: { x: 0, y: 0 }
  to: { x: 1920, y: 1080 }
  easing: ease_out
```

---

### 1. Procedural Layer

Renders GPU-accelerated shapes. Eight primitive types available.

```yaml
- id: "bg"
  procedural:
    kind: solid_color
    color: { r: 0.1, g: 0.1, b: 0.12, a: 1.0 }
```

#### Procedural Kinds

**solid_color** — Fill entire layer with one color.

```yaml
procedural:
  kind: solid_color
  color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
```

**gradient** — Two-color gradient.

```yaml
procedural:
  kind: gradient
  start_color: { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
  end_color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
  direction: horizontal    # horizontal (default) | vertical
```

**circle** — Filled circle.

```yaml
procedural:
  kind: circle
  center: { x: 960, y: 540 }
  radius: 100              # ScalarProperty. Can be animated.
  color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
```

**rounded_rect** — Rounded rectangle.

```yaml
procedural:
  kind: rounded_rect
  center: { x: 960, y: 540 }
  size: { x: 400, y: 200 }
  corner_radius: 16        # ScalarProperty.
  color: { r: 0.2, g: 0.2, b: 0.25, a: 1.0 }
```

**ring** — Donut/annulus shape.

```yaml
procedural:
  kind: ring
  center: { x: 960, y: 540 }
  outer_radius: 200        # ScalarProperty.
  inner_radius: 150        # ScalarProperty.
  color: { r: 0.0, g: 1.0, b: 0.5, a: 1.0 }
```

**line** — Thick line segment.

```yaml
procedural:
  kind: line
  start: { x: 100, y: 540 }
  end: { x: 1820, y: 540 }
  thickness: 4             # ScalarProperty.
  color: { r: 1.0, g: 1.0, b: 1.0, a: 0.5 }
```

**triangle** — Three-point triangle.

```yaml
procedural:
  kind: triangle
  p0: { x: 960, y: 400 }
  p1: { x: 860, y: 600 }
  p2: { x: 1060, y: 600 }
  color: { r: 1.0, g: 0.5, b: 0.0, a: 1.0 }
```

**polygon** — Regular N-sided polygon.

```yaml
procedural:
  kind: polygon
  center: { x: 960, y: 540 }
  radius: 100              # ScalarProperty. Circumscribed radius.
  sides: 6                 # Integer. Number of sides.
  color: { r: 0.5, g: 0.0, b: 1.0, a: 1.0 }
```

#### AnimatableColor

All procedural `color` fields support per-channel expressions:

```yaml
# Static color
color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }

# Animated color (channels are expressions)
color:
  r: "abs(sin(t * 0.5))"
  g: 0.0
  b: "abs(cos(t * 0.5))"
  a: 1.0
```

Each channel (r, g, b, a) is a ScalarProperty. Alpha defaults to 1.0 if omitted.

---

### 2. Text Layer

Renders text using built-in Geist Pixel bitmap fonts.

```yaml
- id: "title"
  position: { x: 960, y: 540 }
  anchor: center
  text:
    content: "HELLO WORLD"     # Required. Non-empty string.
    font_family: "GeistPixel-Line"  # Optional. Default: "GeistPixel-Line".
    font_size: 48              # Optional. Default: 48.
    letter_spacing: 0          # Optional. Default: 0.
    color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }  # Optional. Default: white.
```

**Available fonts:**

- `GeistPixel-Line` (default, clean lines)
- `GeistPixel-Square` (blocky)
- `GeistPixel-Grid` (grid pattern)
- `GeistPixel-Circle` (circular dots)
- `GeistPixel-Triangle` (triangular)

---

### 3. Image Layer

Loads an external image file (PNG, JPEG, WebP).

```yaml
- id: "logo"
  position: { x: 100, y: 100 }
  image:
    path: "assets/logo.png"   # Required. Relative to manifest directory.
```

Path must be relative (no absolute paths). File must exist under the manifest's directory.

---

### 4. Shader Layer (GPU Only)

Custom WGSL fragment shader. You write a `shade()` function; VCR provides the vertex shader and uniform bindings.

```yaml
- id: "custom_effect"
  shader:
    fragment: |
      fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> {
        let r = sin(uv.x * u.custom[0].x + u.time) * 0.5 + 0.5;
        let g = cos(uv.y * u.custom[0].y + u.time) * 0.5 + 0.5;
        return vec4<f32>(r, g, 0.3, 1.0);
      }
    uniforms:
      freq_x: 10.0             # Maps to u.custom[0].x
      freq_y: 8.0              # Maps to u.custom[0].y
```

Or load from file:

```yaml
  shader:
    path: "shaders/my_effect.wgsl"
    uniforms:
      intensity: "sin(t) * 0.5 + 0.5"
```

**ShaderUniforms available in `shade()`:**

```wgsl
struct ShaderUniforms {
  time: f32,                    // Current time in frames
  frame: u32,                   // Current frame index
  resolution: vec2<f32>,        // Canvas resolution
  custom: array<vec4<f32>, 2>,  // Up to 8 user uniforms packed into 2 vec4s
}
```

Uniform packing: uniforms map to `custom[0].x`, `custom[0].y`, `custom[0].z`, `custom[0].w`, `custom[1].x`, etc. in declaration order. Maximum 8 uniforms.

**Falls back to transparent on software backend.**

---

### 5. Asset Layer (Legacy)

Older image loading syntax. Prefer `image:` for new manifests.

```yaml
- id: "old_image"
  source_path: "assets/photo.png"
```

---

### 6. ASCII Layer

Grid-based ASCII art rendering with per-cell control.

```yaml
- id: "ascii_art"
  ascii:
    grid: { rows: 5, columns: 20 }
    cell: { width: 16, height: 24 }
    font_variant: geist_pixel_regular
    foreground: { r: 0.0, g: 1.0, b: 0.4, a: 1.0 }
    background: { r: 0.0, g: 0.0, b: 0.0, a: 0.8 }
    inline:                    # Provide content inline...
      - "  ____  ____  ____  "
      - " |    ||    ||    | "
      - " | VCR||    ||    | "
      - " |____||____||____| "
      - "____________________"
    # OR from file:
    # path: "art/banner.txt"
    cells:                     # Optional per-cell overrides
      - row: 2
        column: 3
        foreground: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
    reveal:                    # Optional reveal animation
      kind: row_major
      start_frame: 0
      frames_per_cell: 1
      direction: forward       # forward (default) | reverse
```

---

## Groups

Groups provide hierarchical transforms. Layers reference a group; group transforms cascade.

```yaml
groups:
  - id: "panel"
    position: { x: 100, y: 800 }
    opacity:
      start_frame: 0
      end_frame: 20
      from: 0.0
      to: 1.0
      easing: ease_out

  - id: "panel_content"
    parent: "panel"            # Inherits panel's transforms.
    position: { x: 20, y: 10 }

layers:
  - id: "bg"
    group: "panel"
    procedural:
      kind: rounded_rect
      center: { x: 400, y: 50 }
      size: { x: 800, y: 100 }
      corner_radius: 8
      color: { r: 0.1, g: 0.1, b: 0.12, a: 0.9 }

  - id: "label"
    group: "panel_content"
    text:
      content: "LOWER THIRD"
      font_size: 32
```

Group properties: `position`, `pos_x`, `pos_y`, `scale`, `rotation_degrees`, `opacity`, `start_time`, `end_time`, `time_offset`, `time_scale`, `modulators`.

---

## Modulators

Named expressions that can be applied to multiple layers with per-property weights.

```yaml
modulators:
  breathe:
    expression: "sin(t * 2.0) * 0.02"
  jitter:
    expression: "noise1d(t * 5.0) * 3.0"

layers:
  - id: "box"
    procedural:
      kind: rounded_rect
      center: { x: 960, y: 540 }
      size: { x: 200, y: 200 }
      corner_radius: 8
      color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
    modulators:
      - source: breathe
        weights:
          scale_x: 1.0
          scale_y: 1.0
      - source: jitter
        weights:
          x: 1.0
          y: 0.5
```

Weight values are multiplied by the modulator's evaluated result, then added to the property.

---

## Post-Processing Pipeline

Array of texture-to-texture shader passes. GPU only. Applied after all layers are composited.

```yaml
post:
  - shader: levels
    gamma: 1.2
    lift: 0.02
    gain: 0.95

  - shader: sobel
    strength: 0.5

  - shader: passthrough    # No-op pass (useful for debugging).
```

### Available Post Effects

| Shader | Parameters | Defaults |
|--------|-----------|----------|
| `passthrough` | (none) | |
| `levels` | `gamma`, `lift`, `gain` | 1.0, 0.0, 1.0 |
| `sobel` | `strength` | 1.0 |

### ASCII Post-Processing

Quantizes rendered output to ASCII character grid:

```yaml
ascii_post:
  enabled: true
  cols: 120
  rows: 45
  ramp: " .:-=+*#%@"       # Character luminance ramp (dark to bright).
```

### 7. Animation Engine (Frame Packs)

Renders frame-by-frame ASCII animations from specialized asset directories.

```yaml
- id: "overlay_anim"
  animation_engine:
    clip_name: "demo_wave"
    fps: 12
    colors:
      foreground: [255, 255, 255, 255]
      background: [0, 0, 0, 0]
```

---

## High-Level Workflows

VCR includes specialized tools for common motion tasks.

### URL to ASCII Overlay (ascii.co.uk)

Use this workflow to import an animated ASCII art page from `ascii.co.uk` and render it as a white-on-alpha ProRes MOV.

**Single or multiple URLs:**

```bash
/Users/coltonbatts/Desktop/VCR/scripts/ascii_link_overlay.sh \
  "https://www.ascii.co.uk/animated-art/milk-water-droplet-animated-ascii-art.html" \
  -- --width 1920 --height 1080 --fps 24
```

**Key Features:**

- **Auto-trimming**: leading blank frames are automatically detected and skipped.
- **Checker Preview**: generates a `*_checker.mp4` with a dark background to verify transparency.
- **White-on-Alpha**: outputs pure white glyphs on a transparent background, perfect for compositing.

---

## Expression Language

Expressions are strings evaluated per-frame. Available wherever a `ScalarProperty` is accepted.

### Variables

| Variable | Description |
|----------|-------------|
| `t` | Current frame number (float). At 30fps, frame 15 = `t` is 15.0. |
| `${param}` | Any manifest parameter by name. |

### Operators

`+`, `-`, `*`, `/`, `%` (modulo), `^` (power), unary `-`.

### Functions

| Function | Args | Description |
|----------|------|-------------|
| `sin(x)` | 1 | Sine |
| `cos(x)` | 1 | Cosine |
| `abs(x)` | 1 | Absolute value |
| `floor(x)` | 1 | Floor |
| `ceil(x)` | 1 | Ceiling |
| `round(x)` | 1 | Round to nearest |
| `fract(x)` | 1 | Fractional part |
| `clamp(x, min, max)` | 3 | Clamp to range |
| `lerp(a, b, t)` | 3 | Linear interpolation |
| `smoothstep(e0, e1, x)` | 3 | Smooth Hermite interpolation |
| `step(edge, x)` | 2 | 0 if x < edge, else 1 |
| `easeinout(t)` | 1 | Ease-in-out on 0-1 range |
| `saw(t, freq?)` | 1-2 | Sawtooth wave 0-1. Default freq=1. |
| `tri(t, freq?)` | 1-2 | Triangle wave 0-2. Default freq=1. |
| `random(x)` | 1 | Deterministic hash-based random 0-1 |
| `noise1d(x, seed?)` | 1-2 | Perlin-like noise -1 to 1 |
| `glitch(t, intensity?)` | 1-2 | Glitch effect using noise |
| `env(time, attack?, decay?)` | 1-3 | Envelope: ramp up then decay. Defaults: attack=12, decay=24. |

### Expression Examples

```yaml
# Smooth fade in over first 30 frames
opacity: "smoothstep(0.0, 30.0, t)"

# Oscillating X position (centered at 960, amplitude 200)
pos_x: "960 + sin(t * 0.1) * 200"

# Pulse opacity using env()
opacity: "env(t, 10, 60)"

# Parameterized speed
pos_x: "100 + t * speed"

# Step-based visibility (appear at frame 30)
opacity: "step(30, t)"

# Blinking cursor (on for 15 frames, off for 15)
opacity: "step(0.5, fract(t / 30.0))"
```

---

## Parameters

Parameters let you make manifests reusable. Define defaults in the manifest, override from CLI.

### Definition Formats

```yaml
# Legacy shorthand (float only)
params:
  speed: 1.5

# Modern typed definition
params:
  speed:
    type: float
    default: 1.5
    min: 0.0
    max: 10.0
    description: "Animation speed multiplier"
  visible:
    type: bool
    default: true
  accent:
    type: color
    default: { r: 1.0, g: 0.0, b: 0.5, a: 1.0 }
  offset:
    type: vec2
    default: { x: 0, y: 0 }
```

### Supported Types

| Type | CLI Format | Example |
|------|-----------|---------|
| `float` | Number | `--set speed=2.5` |
| `int` | Integer | `--set count=10` |
| `bool` | true/false/1/0 | `--set visible=true` |
| `vec2` | x,y | `--set offset=100,-50` |
| `color` | #RRGGBB or r,g,b,a | `--set accent=#ff0066` |

### Substitution

Use `${param_name}` for whole-string substitution in YAML values:

```yaml
params:
  bg_opacity: 0.9

layers:
  - id: "bg"
    opacity: "${bg_opacity}"     # Resolves to 0.9
```

**Rules:**

- Only whole-string replacement. `"prefix_${name}"` is rejected.
- Use `$${name}` to produce literal `${name}`.
- Params are also available as variables in expressions: `"sin(t) * speed"`.

---

## ASCII Curated Library

VCR- Engine docs: `docs/ANIMATION_ENGINE.md`

- Boilerplate example: `examples/ascii_animation_boilerplate.rs`
- Curated Library: `assets/animations/library/` (browse with `vcr ascii library`)
 and organized by category.

### Discovery

```bash
vcr ascii library          # List all curated assets
vcr ascii library --json   # Machine-readable list
```

### Categories

| Category | Description |
|----------|-------------|
| `geometric` | Abstract tunnels, grids, and math shapes |
| `humanoid` | People, characters, and movement |
| `nature` | Fluid dynamics, animals, and natural phenomena |
| `demo` | Technical samples and engine tests |

### Usage in Manifest

Reference a library asset by its path relative to the library root:

```yaml
animation_engine:
  clip_name: "library/humanoid/ballet"
```

---

## CLI Commands

### Core Rendering

```bash
# Full render to video
vcr build scene.vcr -o output.mov
vcr build scene.vcr --start-frame 0 --frames 90

# Single frame
vcr render-frame scene.vcr --frame 42 -o frame_42.png

# Frame range as PNG sequence
vcr render-frames scene.vcr --start-frame 0 --frames 30 -o renders/seq/

# Quick preview (half resolution, first 3 seconds)
vcr preview scene.vcr -o preview.mov --scale 0.5

# Live reload on file changes
vcr watch scene.vcr -o preview.mov --scale 0.5
```

### Validation & Inspection

```bash
# Validate manifest
vcr check scene.vcr

# Deep lint (finds unreachable layers)
vcr lint scene.vcr

# Show layer states at specific frame
vcr dump scene.vcr --frame 30

# List parameters
vcr params scene.vcr --json

# Show resolved manifest state
vcr explain scene.vcr --set speed=2.0

# Determinism hash for frame
vcr determinism-report scene.vcr --frame 0 --json

# Discovery
vcr ascii library      # List curated animations
vcr ascii library --json
```

### Global Flags

```bash
--quiet              # Suppress non-essential logs
--backend auto       # auto (default) | software | gpu
--set NAME=VALUE     # Parameter override (repeatable)
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Usage/argument error |
| 3 | Manifest validation error |
| 4 | Missing dependency (e.g., FFmpeg) |
| 5 | I/O error |

---

## Graduated Examples

### Example 1: Static Colored Rectangle

A single solid-color background. Simplest possible manifest.

```yaml
version: 1
environment:
  resolution: { width: 1920, height: 1080 }
  fps: 30
  duration: 1.0
layers:
  - id: "bg"
    procedural:
      kind: solid_color
      color: { r: 0.05, g: 0.05, b: 0.07, a: 1.0 }
```

### Example 2: Animated Circle

A circle that moves across the screen with eased opacity.

```yaml
version: 1
environment:
  resolution: { width: 1280, height: 720 }
  fps: 30
  duration: 3.0
layers:
  - id: "bg"
    procedural:
      kind: solid_color
      color: { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }

  - id: "dot"
    z_index: 1
    opacity:
      start_frame: 0
      end_frame: 20
      from: 0.0
      to: 1.0
      easing: ease_out
    procedural:
      kind: circle
      center: { x: 640, y: 360 }
      radius: "40 + sin(t * 0.15) * 10"
      color:
        r: "abs(sin(t * 0.05))"
        g: 0.4
        b: 0.9
        a: 1.0
```

### Example 3: Lower Third with Groups

A professional lower third with entrance animation.

```yaml
version: 1
environment:
  resolution: { width: 1920, height: 1080 }
  fps: 30
  duration: 4.0

params:
  entrance_delay:
    type: float
    default: 0.0
    min: 0.0
    max: 2.0
    description: "Delay before entrance (seconds)"
  name_text:
    type: float
    default: 1.0

groups:
  - id: "lower_third"
    position:
      start_frame: 0
      end_frame: 20
      from: { x: 0, y: 40 }
      to: { x: 0, y: 0 }
      easing: ease_out
    opacity:
      start_frame: 0
      end_frame: 15
      from: 0.0
      to: 1.0
      easing: ease_out

layers:
  - id: "bg_bar"
    group: "lower_third"
    position: { x: 80, y: 900 }
    procedural:
      kind: rounded_rect
      center: { x: 300, y: 40 }
      size: { x: 600, y: 80 }
      corner_radius: 6
      color: { r: 0.08, g: 0.08, b: 0.1, a: 0.92 }

  - id: "accent_line"
    group: "lower_third"
    position: { x: 80, y: 895 }
    z_index: 1
    procedural:
      kind: line
      start: { x: 0, y: 0 }
      end: { x: 600, y: 0 }
      thickness: 3
      color: { r: 0.0, g: 0.8, b: 1.0, a: 1.0 }

  - id: "name"
    group: "lower_third"
    position: { x: 100, y: 910 }
    z_index: 2
    text:
      content: "JANE DOE"
      font_family: "GeistPixel-Square"
      font_size: 36
      color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }

  - id: "title"
    group: "lower_third"
    position: { x: 100, y: 948 }
    z_index: 2
    opacity: 0.7
    text:
      content: "Senior Engineer"
      font_family: "GeistPixel-Line"
      font_size: 24
      color: { r: 0.7, g: 0.7, b: 0.75, a: 1.0 }
```

### Example 4: Multi-Layer Composition with Post-Processing

Complex scene with modulators, expressions, and post-processing.

```yaml
version: 1
environment:
  resolution: { width: 1920, height: 1080 }
  fps: 30
  duration: 5.0
seed: 42

params:
  energy:
    type: float
    default: 1.0
    min: 0.0
    max: 3.0

modulators:
  pulse:
    expression: "sin(t * 3.0 * energy) * 0.03"
  drift:
    expression: "noise1d(t * 0.5) * 8.0"

groups:
  - id: "center_group"
    position: { x: 960, y: 540 }
    modulators:
      - source: drift
        weights:
          x: 1.0
          y: 0.6

layers:
  - id: "bg"
    procedural:
      kind: gradient
      start_color: { r: 0.02, g: 0.02, b: 0.05, a: 1.0 }
      end_color: { r: 0.08, g: 0.05, b: 0.12, a: 1.0 }
      direction: vertical

  - id: "ring_outer"
    group: "center_group"
    z_index: 1
    anchor: center
    procedural:
      kind: ring
      center: { x: 0, y: 0 }
      outer_radius: "200 + sin(t * 0.1) * 20"
      inner_radius: "180 + sin(t * 0.1) * 20"
      color:
        r: 0.0
        g: "0.6 + sin(t * 0.2) * 0.2"
        b: 1.0
        a: "0.6 + sin(t * 0.15) * 0.3"
    modulators:
      - source: pulse
        weights:
          scale_x: 1.0
          scale_y: 1.0

  - id: "hex"
    group: "center_group"
    z_index: 2
    anchor: center
    rotation_degrees: "t * 0.5"
    procedural:
      kind: polygon
      center: { x: 0, y: 0 }
      radius: 120
      sides: 6
      color: { r: 0.0, g: 0.7, b: 0.9, a: 0.4 }

  - id: "label"
    group: "center_group"
    z_index: 3
    anchor: center
    position: { x: 0, y: -10 }
    text:
      content: "VCR"
      font_family: "GeistPixel-Square"
      font_size: 72
      color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }

  - id: "scanline"
    z_index: 10
    procedural:
      kind: line
      start: { x: 0, y: 0 }
      end: { x: 1920, y: 0 }
      thickness: 2
      color: { r: 0.0, g: 1.0, b: 0.8, a: 0.15 }
    pos_y: "fract(t / 120.0) * 1080"

post:
  - shader: levels
    gamma: 1.1
    lift: 0.01
    gain: 0.98
```

---

## Common Patterns

### Fade In

```yaml
opacity:
  start_frame: 0
  end_frame: 30
  from: 0.0
  to: 1.0
  easing: ease_out
```

### Fade Out

```yaml
opacity:
  start_frame: 60
  end_frame: 90
  from: 1.0
  to: 0.0
  easing: ease_in
```

### Slide In from Left

```yaml
position:
  start_frame: 0
  end_frame: 20
  from: { x: -200, y: 540 }
  to: { x: 100, y: 540 }
  easing: ease_out
```

### Timed Layer Visibility

```yaml
start_time: 1.0    # Appear at 1 second
end_time: 4.0      # Disappear at 4 seconds
```

### Blinking / Pulsing

```yaml
# Blink every 30 frames
opacity: "step(0.5, fract(t / 30.0))"

# Smooth pulse
opacity: "0.5 + sin(t * 0.2) * 0.3"
```

### Continuous Rotation

```yaml
rotation_degrees: "t * 2.0"    # 2 degrees per frame
```

### Typewriter Reveal (Per-Character)

Use `step()` with staggered thresholds:

```yaml
# Character 1 visible from frame 0
opacity: "step(0, t)"
# Character 2 visible from frame 3
opacity: "step(3, t)"
# Character 3 visible from frame 6
opacity: "step(6, t)"
```

### Staggered Layer Entrance

Use `time_offset` on groups or layers:

```yaml
groups:
  - id: "item_1"
    time_offset: 0.0
  - id: "item_2"
    time_offset: 0.2
  - id: "item_3"
    time_offset: 0.4
```

### Organic Motion with Noise

```yaml
modulators:
  organic:
    expression: "noise1d(t * 2.0) * 5.0"

layers:
  - id: "element"
    modulators:
      - source: organic
        weights:
          x: 1.0
          y: 0.8
          rotation: 2.0
```

---

## Validation Checklist

Before rendering, verify:

1. **version** is `1`.
2. **environment** has `resolution`, `fps`, `duration` all set and valid.
3. **At least one layer** exists.
4. **All layer IDs are unique** strings. Not empty.
5. **z_index** determines render order — higher is in front.
6. **Image paths** are relative, not absolute. File must exist under manifest directory.
7. **ScalarProperty expressions** use valid function names and reference defined params.
8. **Modulator sources** in layer bindings must reference modulators defined at the top level.
9. **Group parents** must reference defined group IDs. No cycles.
10. **Post-processing** is GPU-only. Use `--backend gpu` or `auto`.
11. **Shader layers** are GPU-only. Software backend renders them transparent.
12. **Duration in frames** must not exceed 100,000.
13. **Resolution** per dimension must not exceed 8192.

---

## Common Gotchas

### 1. Expression Variable is `t` (Frame Number), NOT Seconds

`t` is the frame number as a float. At 30fps, 1 second = `t` of 30. To convert: `t / fps`.

### 2. `deny_unknown_fields` is Active

Any typo in a YAML key will cause a parse error. `colour` instead of `color` will fail.

### 3. Shader Layers are GPU-Only

Custom shader layers render as transparent on the software backend. Always use `--backend gpu` or `auto` for shader content.

### 4. Post-Processing is GPU-Only

The `post:` pipeline requires the GPU backend. It will be skipped on software.

### 5. Image Paths Must Be Relative

Absolute paths (e.g., `/Users/...`) are rejected for security. Always use paths relative to the manifest file.

### 6. AnimatableColor Alpha Defaults to 1.0

If you omit `a:` in a color, it defaults to 1.0 (fully opaque). This is correct for most cases.

### 7. `pos_x` / `pos_y` Override `position`

If you set both `position: {x: 100, y: 200}` and `pos_x: "sin(t)"`, the X component will be driven by the expression. The Y from `position` still applies unless `pos_y` is also set.

### 8. Modulators are Additive

Modulator values are _added_ to properties (multiplied by weight). A weight of `1.0` on `x` adds the modulator's value directly to position.x.

### 9. Easing Curves are Limited

Only four easing curves: `linear`, `ease_in`, `ease_out`, `ease_in_out`. For more complex easing, use expressions with `smoothstep()` or `easeinout()`.

### 10. `${}` Substitution is Whole-String Only

`"speed is ${speed}"` will fail. Use `"${speed}"` alone, or reference params directly in expressions: `"t * speed"`.

### 11. Procedural Shapes Fill the Layer Area

Procedural primitives are rendered to a texture the size of the full canvas. Position, scale, and rotation on the layer transform the entire texture.

### 12. Time Variables

- `start_time` / `end_time` are in **seconds**
- `time_offset` is in **seconds**
- `start_frame` / `end_frame` in KeyValue are in **frames**
- `t` in expressions is in **frames**

---

## File Structure Conventions

```
project/
  scene.vcr              # Manifest
  assets/                 # Images, videos
    logo.png
    background.mov
  shaders/                # Custom WGSL shaders
    effect.wgsl
  renders/                # Output (auto-created)
    scene.mov
    scene_meta.json
```

---

## Determinism Contract

Same manifest + same params + same seed + same backend = identical frame bytes.

- **Software backend**: Bitwise identical on the same machine and toolchain.
- **GPU backend**: Not guaranteed bit-exact across different hardware/drivers.
- Use `vcr determinism-report scene.vcr --frame 0` to get a frame hash for verification.

---

## Dependencies

VCR requires:

- **FFmpeg** in PATH (for video encoding)
- **Rust stable toolchain** (for building from source)
- **GPU** (macOS Metal) for GPU backend, shader layers, and post-processing

Run `vcr doctor` to verify all dependencies.

---

## Troubleshooting (Error → Fix)

These are real error messages VCR produces, with exact fixes:

### `unknown variant 'solid_colour', expected one of 'solid_color', ...`

**Cause**: Typo in procedural `kind`.
**Fix**: Use exact kind names: `solid_color`, `gradient`, `triangle`, `circle`, `rounded_rect`, `ring`, `line`, `polygon`.

### `unknown field 'extra_field', expected 'color'`

**Cause**: Extra or misspelled key in a layer/procedural block. Schema uses `deny_unknown_fields`.
**Fix**: Remove the unknown key. Check spelling against this reference.

### `invalid expression 'bad_func(t)': unsupported function 'bad_func'`

**Cause**: Expression references a function that doesn't exist.
**Fix**: Use only supported functions: `sin`, `cos`, `abs`, `floor`, `ceil`, `round`, `fract`, `clamp`, `lerp`, `smoothstep`, `step`, `easeinout`, `saw`, `tri`, `random`, `noise1d`, `glitch`, `env`.

### `manifest must define at least one layer`

**Cause**: `layers: []` or `layers` key missing.
**Fix**: Add at least one layer.

### `missing field 'duration'`

**Cause**: `environment` block is missing a required field.
**Fix**: Ensure `environment` has all three: `resolution`, `fps`, `duration`.

### `Absolute output paths are restricted for security`

**Cause**: Output path starts with `/` (e.g., `-o /tmp/out.mov`).
**Fix**: Use a relative path (e.g., `-o renders/out.mov`).

### `invalid --set for param 'speed': expected float, got 'fast'`

**Cause**: CLI `--set` value doesn't match the param's declared type.
**Fix**: Pass a value matching the type. For float: `--set speed=1.5`.

### `invalid substitution string 'text=${name}'`

**Cause**: `${}` used inside a longer string. Only whole-string substitution is supported.
**Fix**: Use `"${name}"` alone, not embedded in other text.

### `layer 'bg': custom shader layers require GPU backend`

**Cause**: Shader layer rendered on software backend.
**Fix**: Use `--backend gpu` or `--backend auto` (default).

---

## Agent Workflow

Recommended workflow for an AI agent generating VCR content:

```
1. Write manifest YAML to a .vcr file
2. Run `vcr check <file>` to validate (exit code 0 = valid)
3. Run `vcr render-frame <file> --frame 0 -o frame.png` to test visually
4. If satisfied, run `vcr build <file> -o output.mov` for full video
5. Use `vcr params <file> --json` to discover overridable parameters
6. Use `--set key=value` to customize without editing the manifest
```

Always validate with `check` before rendering. It catches schema errors, expression errors, and missing references instantly without the cost of GPU initialization.
