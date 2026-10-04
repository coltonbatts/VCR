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

## Pack First-Look (For Pack-Based Requests)

When a request references `packs/<pack-id>/...`, generate a visual contact sheet before choosing items:

```bash
scripts/pack_contact_sheet.sh \
  --pack packs/y2k-bold-modern \
  --out renders/y2k_pack/contact_sheet.png \
  --index-out renders/y2k_pack/contact_sheet.index.tsv
```

This produces:
- A labeled PNG contact sheet (ID + dimensions on each tile).
- A TSV index for fast ID-driven follow-up prompts like "animate `y2k-26` like this".

## Agent Workflow

Short version in [docs/AGENT_QUICKSTART.md](docs/AGENT_QUICKSTART.md); machine contract in [docs/AGENT_CONTRACT.md](docs/AGENT_CONTRACT.md).

0. `vcr capabilities --json`: what this installed engine can do and what is usable here (do not guess fields, fonts, codecs, time units or hardware).
1. `vcr prompt --json`: normalize. `status: "blocked"` means stop and resolve with the requester.
2. If packs are referenced, run `scripts/pack_contact_sheet.sh` and share item IDs/dimensions.
3. Resolve or explicitly report entries in `unknowns_and_fixes`. Never invent text, brand colors or assets.
4. Author the manifest from `normalized_spec`.
5. Validate in order: `vcr check --json`, `vcr lint --json`, `vcr explain --json` (backend preflight; `ready` must be true).
6. `vcr inspect --json`: sampled frames + contact sheet + bounds/timing diagnostics. Revise by layer `id` or declared param, then inspect again.
7. `vcr render --json` (atomic publish + provenance), then `vcr verify FILE --manifest M [--expect-transparency required] --json`. Verification passing is the definition of delivered.

With `--json`, stdout is exactly one JSON line; read `status`, `diagnostics`, `error.code`; never scrape stderr.

## Output Contract from `vcr prompt`

- `standardized_vcr_prompt`
- `normalized_spec`
- `unknowns_and_fixes`
- `assumptions_applied`
- `acceptance_checks`

## Determinism Scope

Scene settings are always reproducible. Software-backend raster frames are expected identical for the same engine build. Decoded/encoded bytes depend on the ffmpeg build (recorded in `*.provenance.json`). GPU output is not bit-identical across hardware. See the contract doc.

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

## Prompt Patterns

For high-quality results from natural language, use the **A.S.A.P** pattern:

- **A**spect: Define resolution (e.g., 1080p, square, vertical).
- **S**tyle: Use keywords like `dreamcore`, `cinematic`, or `pro_tech`.
- **A**ssets: List fonts (`GeistPixel-Line`) and specific shaders (`neural_sphere`).
- **P**arameters: Specify duration (5s) and frame rate (60fps).

## References

- Agent skill reference: [SKILL.md](SKILL.md)
- Architect prompt + constraints: [AGENT_IDENTITY.md](docs/AGENT_IDENTITY.md)
