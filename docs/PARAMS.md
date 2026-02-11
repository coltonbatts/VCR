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
