# VCR agent-first handoff (2026-10-04)

> **Studio Ops was not reachable from this session.** The task ran in an isolated cloud container;
> `/Users/coltonbatts/Documents/Studio Ops` does not exist there, so `READ-ME-FIRST.md`, `now.md`,
> `projects.md`, `tasks/vcr-001-production-baseline.md`, `handoffs/vcr.md` and
> `plans/vcr-agent-first-2026-10-04.md` were **not read, and nothing there was updated.** Everything
> below is self-contained so it can be pasted into `handoffs/vcr.md` and the task file. The two
> Mac-side facts I could not verify are marked ⚠.

## State

| | |
|---|---|
| Repo | `coltonbatts/VCR` (cloud checkout; the Mac checkout was not touched) |
| Branch | `claude/epic-rubin-iv5jii` |
| Base | `ea2dead` (= `origin/main`), then fast-forwarded to `85e6e26` (`origin/codex/explain-preflight-readme`, "Add explain preflight JSON") because the brief says to extend that preflight |
| Head | code revision `707bc72`; this handoff + evidence are in the commit that follows it (`git log -1`) |
| Local changes at start | none in this checkout (⚠ the Mac's uncommitted `tests/golden_manifest_stability.rs` edit is not visible here) |
| Binary identity | `target/release/vcr` = 0.1.2 (`707bc72`, release). ⚠ `~/.local/bin/vcr` (0.1.2 `ea2dead` per the earlier assessment) is **not on this container** and is **older than the contract**; the MCP now refuses it (`engine.incompatible`) instead of silently using it |
| Writing ownership | sole writer in an isolated clone; no other agents started |
| Not merged | `origin/fix/agent-error-contract` and `origin/foundations/time-keyframes-color-encode` have **unrelated histories** (no merge base) and were left alone. The latter changes manifest time units (v2 seconds) and encoding; this work targets manifest v1 |

## The eight assessed findings, verified against live code

| # | Finding | Verified | Action |
|---|---|---|---|
| 1 | Prompt gate vs MCP planner disagree (60 fps vs 24 fps/5 s, alpha default) | **Confirmed** (planner defaulted 24 fps, 5.0 s, alpha=false; no gate) | MCP planner now calls `vcr prompt --json`; adapter has no defaults; parity tested |
| 2 | Structured interfaces incomplete; no schema discovery; MCP concatenates stdout/stderr | **Confirmed** | `--json` on check/lint/dump/doctor/prompt/build/render-frame (+inspect, capabilities); `capabilities --schema`; MCP forwards documents |
| 3 | Multiple error shapes | **Confirmed** (CodedError, AgentErrorReport, clap) | one envelope; legacy keys kept; clap errors structured under `--json` |
| 4 | Binary selection can hide version mismatches | **Confirmed** (`which vcr` first) | explicit → `VCR_BIN` → checkout build → PATH; probed with `capabilities`; identity + mismatch warnings in every response |
| 5 | `verify` only hashes | **Confirmed** | full media/provenance verification (below) |
| 6 | Last-frame-only layer metadata | **Confirmed** | `vcr inspect` (motion-aware samples, exact bounds) |
| 7 | Doc contradictions | **Partly stale**: at `85e6e26` the engine *already rejects* shader layers on software (README right; SKILL.md "falls back to transparent" wrong). `post:`/`ascii_post` were **silently ignored** on software (a real gap) | SKILL.md corrected; software now rejects `post`/`ascii_post` (`UNSUPPORTED_SOFTWARE_FEATURES`); determinism claims split into levels |
| 8 | Installer deletes previous install; example clipping; fresh-install / editor import | Installer **confirmed** (`rm -rf` before clone). Example clipping and editor import: ⚠ not re-checked | installer fixed and tested; **fresh-install on the Mac and editor-import checks remain open** |

### Additional defects found and fixed while doing the work

- `vcr preview --scale` rendered layers at original pixel coordinates into a smaller canvas (a cropped
  view, not a scaled composition).
- Manifests with image/video/sequence assets failed validation when invoked by **bare filename**
  (`vcr check scene.vcr`): `fs::canonicalize("")`. Regression test added.
- Software backend silently ignored `post:` and enabled `ascii_post:`.
- Unknown `font_family` silently substitutes GeistPixel-Line (now flagged by preflight).
- A render killed by timeout left a hidden partial file (now removed by the adapter and swept by the engine).
- MCP passed absolute output paths that the engine rejects (execute_plan could never succeed on `build`).
- Authoring traps now discoverable/documented: procedural geometry is normalized 0–1; expressions use frames, layer times use seconds; `start_time` does not shift keyframes; no string params; text does not wrap.

## What now works for an agent that previously could not

Discover → normalize → author → validate/preflight → inspect → revise → render → verify, entirely
through structured output (CLI `--json` or MCP): see `docs/AGENT_QUICKSTART.md` and
`docs/AGENT_CONTRACT.md`. Concretely: determine hardware/codec/font/time-unit support without
guessing; get blockers instead of invented defaults; find a clipped/mistimed/blank layer from
rendered evidence; revise one layer or one param; deliver only atomically-published, verified
files; tell stale, modified, truncated, wrong-codec, wrong-duration and opaque-instead-of-
transparent files apart.

## Files and interfaces

New library modules: `agent_contract`, `capabilities`, `preflight`, `inspect`, `media_verify`,
`provenance`. New bin module `src/agent_cli.rs`. Changed: `main.rs`, `manifest.rs` (bare-filename
fix), `schema.rs` (JSON Schema derivation via `schemars`, `Layer::kind`, expression-function table
+ drift test), `prompt_gate.rs` (`defaults_applied`), `font_assets.rs`, `renderer.rs` (shared font
alias table). MCP: `scripts/vcr-mcp-server/{engine,server}.py` + tests. Docs: AGENT_QUICKSTART,
AGENT_CONTRACT, AGENTS.md, SKILL.md, README, ARCHITECTURE, EXIT_CODES, DETERMINISM_SPEC,
VCR_VERIFICATION, CHANGELOG. Installer: `scripts/install.sh`. CI: MCP test step.
Benchmark: `docs/agent-benchmark/`, `scripts/agent_bench/`. Proof: `examples/agent/`,
`docs/agent-first/proof/`. New dependency: `schemars 1`.

## Compatibility and migration

See `docs/AGENT_CONTRACT.md` → "Compatibility and migration". Behavior changes to be aware of:
`vcr verify` now fails (exit 3) on mismatch; software renders of `post:` fail; `preview --scale`
output geometry changed (correctly); `doctor` requires ffprobe; new exit code 6 (`prompt --strict`
only); `vcr_render_plan` MCP signature lost its override params; legacy `VCR_AGENT_MODE` stderr
output is now the envelope plus the old keys (pretty-printed as before).

## Tests and evidence

All run on this Linux container (no GPU adapter; ffmpeg/ffprobe 6.1.1). Engine build `707bc72` (release).

| Check | Result |
|---|---|
| `cargo test --no-fail-fast` | everything passes **except** `golden_manifest_stability` (encoded-file hash only; the raster `frame_hash` assertion passes). It fails identically on untouched `ea2dead` and `85e6e26`, i.e. pre-existing and ffmpeg-build-specific, not caused by this work |
| new unit tests (`--lib`) | 173 pass (contract classifier, schema/expression-table drift guard, inspect, capabilities) |
| `tests/agent_contract_cli.rs` | 22 pass: parsed JSON structure/meaning for success and every failure class, preflight, atomic publish, verify negatives (frames/fps/resolution/codec/profile/stale/modified/truncated/opaque-vs-alpha-capable), determinism at three hash levels, clipping + mistimed detection with param revisions, preview geometry, bare-filename regression (fail-then-pass shown), legacy agent-mode envelope |
| existing `cli_contract`, `determinism`, `params_reliability`, `agent_error_reporting` | pass |
| MCP adapter (`python3 -m unittest discover -s scripts/vcr-mcp-server/tests`) | 13 pass: selection/precedence/mismatch warnings, incompatible-engine rejection, protocol violation, **real process-group kill on timeout** (grandchild dead), CLI↔MCP normalization parity incl. blockers, no adapter defaults, path confinement, end-to-end inspect→render→verify, timeout publishes nothing |
| installer (`scripts/install.sh`, fake local repos in a sandbox HOME) | clone failure → old install intact; build failure → old install restored; success → one `.previous` generation kept |
| `cargo check --all-features`, `cargo fmt --check` | clean; clippy has no warnings in the new modules (pre-existing warnings elsewhere untouched) |
| `scripts/agent_bench/contract_checks.py` | all 6 steps pass (`evidence/contract_report.json`) |
| benchmark reference run (`reference_run.py`, **scripted, not agent**) | 19/19 as expected: 11 reference cases (the 10 tasks; T08 has two cases) pass all gates; 8 negative controls (opaque instead of transparent, un-reflowed aspect, unfixed overflow, ignored `--set`, invented logo, claimed delivery of a blocked task, "everything is fine" artifact triage…) fail for the right reasons |
| lower-third baseline (real 1080p60 render, software, ~34 s) | status ok; 300 frames; `verify --expect-transparency required`: 9/9 checks pass, **300/300 decoded frames contain alpha < 255, min alpha 0**; producer == verifier == `707bc72` |
| reproducibility (same engine+ffmpeg, different filename) | raster, decoded and encoded hashes identical; scene metadata byte-identical |
| 12-variant matrix (`run_variants.py`, scripted) | 12/12 pass check/lint/preflight/inspect/render/verify; text inside panel (exact bounds, after an automatic inspect→measure→refit loop on the long-text variants: 611 and 1322 px panel right edge); 12 distinct decoded hashes; repeat render matches at all three levels |

## Artifact paths (container-local, `renders/` is gitignored; regenerate with the commands in `docs/agent-first/proof/README.md`)

- `renders/lower_third/lt_v1_baseline.mov` (24,842,258 B, sha256 `363e93c11fc83bb16a89f4de557cbdf34169f931ca624fae618862f550411f20`) + `.metadata.json` + `.provenance.json`
- `renders/lt_v1_inspect/contact_sheet.png`, `sample_*.png`, `inspection.json`
- `renders/lower_third_variants/*.mov` (12) + `variant_report.json`
- `renders/agent_bench/reference/` (scored reference workdirs and records)

Committed evidence (small): `docs/agent-first/proof/evidence/`.

## Remaining work and unverified claims

1. **No autonomous-agent run has been performed in any harness.** The benchmark is defined and the
   scorer/reference runs prove the gates; they do not measure agent success. The 9/10 target is a
   proposal. Needed: choose harnesses, authorize runs/cost, run ≥ 3 repeats, review visual rubrics.
2. **No comparative evidence** against other video tools exists. Do not claim "best available".
3. **GPU backend untested** here (no adapter). Shader/video/lottie/post paths, `T09` applicability,
   and GPU determinism are unverified.
4. **Cross-machine encoded bytes:** `tests/golden_manifest_stability.rs` fails on this machine's
   ffmpeg 6.1.1 (file hash only; raster hash matches). Pre-existing, ffmpeg-build specific; left
   unchanged. Decision needed: pin ffmpeg in CI, or relax that test to the raster hash plus a
   recorded-toolchain check.
5. **Creative approval:** copy, palette and layout in the lower-third proof are fixture placeholders.
6. **Recovery behavior with real agents** (missing asset, unsupported backend) is specified and
   scripted, not demonstrated autonomously.
7. ⚠ **Fresh-install proof and editor-import (Premiere/AE/Resolve) checks** from the production
   baseline task are not done; the installer fix was tested only with fake local repos.
8. ⚠ Mac/Metal path, `prores_videotoolbox`, and the installed `~/.local/bin/vcr` were not exercised.
9. Approximate diagnostics (`text.small`, `timing.hold_short`) are heuristics; safe-area is a policy
   check. Layer bounds are exact for pixels drawn, not for layout intent. Inspection cannot judge
   design quality.
10. `render-frames`, `preview`, `watch`, `play`, ASCII/chat/pack/tape commands have no `--json`.

## Next action

Authorize and run the first harness pass of the benchmark (3 repeats × 10 tasks) using
`docs/agent-benchmark/README.md`, with a human reviewer scoring the visual rubric from
`inspect`'s contact sheets; in parallel, on the Mac: build this branch, run
`python3 scripts/agent_bench/contract_checks.py`, and complete the fresh-install and editor-import
checks.

## Paste-ready Studio Ops update

> VCR — branch `claude/epic-rubin-iv5jii` @ 707bc72+docs (base 85e6e26). Agent contract `vcr.agent/1`
> shipped: `--json` + structured errors, `capabilities`/schema, extended preflight, `inspect`,
> media/provenance `verify`, atomic publish, MCP rewritten as thin typed wrappers with explicit
> engine selection. Baseline lower-third (alpha) rendered and verified on decoded pixels; 12-variant
> matrix passed 12/12 by script (verify, decoded alpha, distinct hashes, reproducibility). Benchmark suite v1 defined; scripted reference + negative controls pass;
> **no autonomous-agent runs yet; no comparative evidence**. Open: harness runs, fresh-install and
> editor-import on the Mac, golden file-hash vs ffmpeg build, creative approval of fixture content.
