# VCR Params Semantics

This document defines the exact behavior for typed manifest params and `--set` overrides.

## Precedence

1. Manifest defaults (`params.<name>.default` or legacy numeric `params.<name>`) initialize values.
2. CLI overrides (`--set name=value`) are applied next.
3. Effective values are the resolved params used for render/evaluation.

CLI overrides always win over manifest defaults.

## Param Types

Supported types:

- `float`
- `int`
- `color`
- `vec2`
- `bool`

## Override Parsing (`--set`)

Format: `--set name=value`

- `name` must be a valid identifier (`[A-Za-z_][A-Za-z0-9_]*`).
- `value` is parsed strictly by declared param type.
- Duplicate `--set` for the same param is rejected with an error.

Type parsing:

- `float`: finite numeric literal (example: `1.25`, `-0.5`)
- `int`: strict integer literal only (example: `3`, `-7`)
- `bool`: `true`, `false`, `1`, `0`
- `vec2`: `x,y` (comma-delimited; whitespace around values allowed)
- `color`:
  - `#RRGGBB`
  - `#RRGGBBAA`
  - `r,g,b[,a]` (numeric channels)

Notes:

- Shell quoting is handled by the shell. VCR receives already-tokenized strings and does not implement shell parsing.
- Bounds (`min`, `max`) apply to numeric param types (`float`, `int`, and expression-scalar bool/int/float forms).

## Time Model (manifest `version`)

The manifest `version` selects what "time" means. New manifests should use `version: 2`.

| | `version: 1` (default, legacy) | `version: 2` |
|---|---|---|
| expression `t` | layer-local **frame number** | layer-local **seconds** |
| expression `frame` | layer-local frame number | layer-local frame number |
| expression `fps` | `environment.fps` | `environment.fps` |
| `env(t)` default attack / decay | 12 / 24 (frames) | 0.5 / 1.0 (seconds) |
| procedural colors/radii, shader uniforms, ascii reveal | evaluated at the **global** frame | evaluated at **layer-local** time |
| params named `frame` / `fps` | allowed (param value wins) | rejected (reserved) |

Why it matters: in `version: 1`, `pos_x: "100 + t * 50"` moves 50 px *per frame*, so the
same manifest plays 2.5x faster in wall-clock time at 60fps than at 24fps. In `version: 2`
it moves 50 px per second at any frame rate. `version: 1` manifests render byte-identically
to earlier VCR releases; nothing changes unless you opt in.

"Layer-local" means after group and layer timing controls: `local = (global + time_offset) * time_scale`,
applied from the outermost group inward. `start_time` / `end_time` are visibility windows
in global seconds and do not shift local time.

Tips for `version: 2`:

- Per-frame randomness: `random(frame)` (not `random(t)`, which changes once per second).
- `step(0.5, fract(t * 2))` blinks at 2 Hz. In `version: 1` it is always 0 because `t` is an integer frame.
- Keyframe times: use `time:` (seconds) or `frame:` (frames); see "Keyframes" below.

Migrating a `version: 1` manifest: add `version: 2`, then divide the time constants in
expressions by `fps` (for example `smoothstep(24, 48, t)` becomes `smoothstep(1, 2, t)` at 24fps).
`start_frame`/`end_frame` keyframes keep working unchanged in both versions because they
are explicit frame numbers.

## Keyframes

Animatable properties: `position`, `scale`, `rotation_degrees`, `opacity`, `pos_x`/`pos_y`,
procedural colors (`color`, `start_color`, `end_color`), numeric procedural fields
(`radius`, `corner_radius`, `thickness`, ...) and shader `uniforms`. Each accepts:

1. A static value: `opacity: 0.8`, `position: [100, 200]`.
2. An expression string (scalars only): `opacity: "0.5 + 0.5 * sin(t * 3)"`.
3. A keyframe track:

```yaml
opacity:
  keyframes:
    - { time: 0.0, value: 0.0, easing: ease_out }            # seconds
    - { time: 0.6, value: 1.0, easing: hold }                # stays 1.0 until the next key
    - { time: 2.0, value: 1.0, easing: [0.42, 0, 0.58, 1] }  # CSS cubic-bezier
    - { time: 2.5, value: 0.0 }
position:
  keyframes:
    - { frame: 0, value: [-200, 540] }                       # explicit frame numbers
    - { frame: 36, value: [960, 540], easing: ease_in_out }
color:                                                        # whole-color track
  keyframes:
    - { time: 0, value: { r: 1, g: 0.2, b: 0.1 } }
    - { time: 1, value: { r: 0.1, g: 0.4, b: 1, a: 0.5 } }
```

4. The legacy single-segment mapping, which is shorthand for a two-key track:
   `{ start_frame: 0, end_frame: 24, from: 0, to: 1, easing: ease_in }`, or in seconds
   `{ start_time: 0, end_time: 1, from: 0, to: 1 }`. Use one pair or the other, not a mix.

Rules:

- Each key sets exactly one of `time` (seconds) or `frame`. All keys in one track use the
  same unit, and times must be strictly increasing. `frame:` keys are fps-dependent by
  definition, while `time:` keys are not.
- Before the first key the first value holds; after the last key the last value holds.
- A key's `easing` shapes the segment **from that key to the next** (the last key's
  easing is unused). Options: `linear` (default), `ease_in`, `ease_out`, `ease_in_out`,
  `hold` (alias `step`: keep this key's value until the next key, then jump),
  or a cubic bezier `[x1, y1, x2, y2]` / `{ cubic_bezier: [x1, y1, x2, y2] }` with the same
  meaning as CSS `cubic-bezier()` (`x1`, `x2` must be in `[0, 1]`; `y` may overshoot).
- Colors interpolate per channel on the authored values. Per-channel animation also works:
  `color: { r: { keyframes: [...] }, g: 0.2, b: "sin(t)", a: 1 }`.
- Keyframes are sampled at the layer-local frame, so `time_offset`/`time_scale` and group
  timing apply to them just like they apply to expressions. (Exception kept for
  compatibility: in `version: 1`, procedural color/shape fields use the global frame.)
- Text layer `text.color` and ascii colors are static. Text is rasterized once, so animate
  a text layer with `opacity`/transform keyframes instead.

## Substitution (`${param_name}`)

Substitution is intentionally strict and deterministic.

Rules:

- Only whole-string scalar tokens are substituted.
  - Valid: `"${speed}"`
  - Invalid: `"speed=${speed}"` (rejected)
- Missing references are hard errors.
- Escaping literal `${...}` is done with `$${...}`.
  - Example: `"$${speed}"` resolves to literal string `"${speed}"`.

### Substitution Depth

- Maximum substitution depth is 1.
- Param defaults cannot reference other params.
  - This prevents recursive chains such as `A -> ${B}` and `B -> ${A}`.

## Determinism and Hashing

VCR computes deterministic hashes with stable ordering.

- Resolved manifest hash includes:
  - raw manifest content
  - resolved params
  - applied overrides
- Sidecar metadata `manifest_hash` additionally binds frame window:
  - start frame
  - frame count
  - end frame

Override ordering does not change the resulting hash when the effective resolved values are identical.

## Metadata Stability

Metadata JSON is deterministic:

- stable field ordering (struct + `BTreeMap` ordering)
- no timestamps
- no machine-specific filesystem paths

## Quiet Mode

`--quiet` suppresses non-essential param dumps, watch diff chatter, and progress logs while preserving errors and command failures.

Render success output paths are still printed (for example `Wrote render.mov` and sidecar paths).

## Error Message Contract

Override type errors include:

- param name
- expected type
- received value
- valid example

Examples:

```bash
vcr build scene.vcr --set speed=fast
# invalid --set for param 'speed': expected float, got 'fast'. Example: --set speed=1.25

vcr build scene.vcr --set drift=10
# invalid --set for param 'drift': expected vec2, got '10'. Example: --set drift=120,-45
```

## Troubleshooting Params

- `invalid --set ... expected NAME=VALUE`
  - Fix: pass each override as `--set name=value` and repeat the flag for multiple values.
- `invalid --set for param 'name': expected vec2, got '1 2'`
  - Fix: vec2 must be comma-delimited: `--set name=1,2`.
- `invalid substitution string 'speed=${speed}'`
  - Fix: only whole-string tokens are substituted. Use `"${speed}"` or escape with `"$${speed}"` when you want a literal.
- `duplicate --set override for param 'name'`
  - Fix: provide each param at most once; duplicates are rejected deterministically.
