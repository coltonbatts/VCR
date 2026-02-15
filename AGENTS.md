# VCR Agents Entry

This file is the agent-first entrypoint for automated coding/workflow tools.

## Primary Rule: Prompt Gate First

Before generating or editing a `.vcr` manifest, run `vcr prompt` on the user request.

```bash
# Natural language input
vcr prompt --text "5s alpha lower third at 60fps output ./renders/lower_third.mov"

# YAML or mixed request file
vcr prompt --in ./request.yaml -o ./request.normalized.yaml
```

Treat `unknowns_and_fixes` as blocking normalization work. Do not silently invent missing values.

## Agent Workflow

1. Run `vcr prompt`.
2. Resolve or explicitly report entries in `unknowns_and_fixes`.
3. Author manifest from `normalized_spec` and `standardized_vcr_prompt`.
4. Validate with `vcr check` and `vcr lint`.
5. Render with `vcr build`.

## Output Contract from `vcr prompt`

- `standardized_vcr_prompt`
- `normalized_spec`
- `unknowns_and_fixes`
- `assumptions_applied`
- `acceptance_checks`

## Determinism Defaults

- Missing `render.fps` defaults to `60`.
- Missing output fps defaults to render fps.
- Missing resolution defaults to `1920x1080`.
- Missing seed defaults to `0`.
- Missing codec defaults to:
  - ProRes 4444 when alpha is enabled.
  - ProRes 422 HQ when alpha is disabled.
- Missing output path defaults to:
  - `./renders/out.mov` for video.
  - `./renders/out.png` for stills.

## References

- Agent skill reference: `SKILL.md`
- Architect prompt + constraints: `docs/AGENT_IDENTITY.md`
