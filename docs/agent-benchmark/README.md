# VCR Agent Benchmark (suite v1)

A reusable, pre-registered suite for measuring whether agents can use VCR to make video without a
human operating the software. **Tasks, gates and scoring were fixed before any run** (`tasks.yaml`).
If a task needs to change after results exist, create suite v2; do not edit v1.

## What counts as evidence

| `evidence_kind` | Produced by | What it supports |
|---|---|---|
| `deterministic_contract_check` | `scripts/agent_bench/contract_checks.py` | The contract and tools behave as specified |
| `scripted_reference` | `scripts/agent_bench/reference_run.py` | Every gate is satisfiable through the public CLI, and rejects wrong work (negative controls) |
| `autonomous_agent_run` | an agent in a real harness | **The only kind that supports an autonomy claim.** The scorer refuses to label a record this way without `harness`, `model`, `session_ref`, `run_id` |

A scripted workflow is never labelled agent autonomy. `score.py summarize` prints
`NO AUTONOMOUS-AGENT EVIDENCE` when no `autonomous_agent_run` records exist.

## Status

- Deterministic checks and the scripted reference run exist and pass (see the handoff for the run).
- **No autonomous-agent run has been performed in any harness.** Nothing here measures agent
  success rates, and no comparison against other video tools has been made. The release target
  below is a proposal, not a result.

## The ten tasks

T01 alpha lower third · T02 title sequence · T03 branded intro (supplied logo) · T04 supplied-image
scene · T05 aspect-ratio adaptation · T06 long-text/clipping repair · T07 parameterized variants ·
T08 missing-asset recovery (T08a recoverable from the pool, T08b unavailable: correct behavior is to
report the blocker; one task, passes only if both pass) · T09 unsupported-backend recovery
(applicable only where no GPU is usable) · T10 encoded-artifact verification (seven exports: one
good, six wrong in different ways).

Supplied assets: `assets/logo_mark.png` (512×512 RGBA), `assets/photo_placeholder.png` (1280×720).
Fixtures: `fixtures/<task>/`. Reference solutions: `reference/` (scripted, not agent output).

## Metrics (recorded separately, never blended)

task completion · operational human interventions · repair attempts · elapsed time · tool calls ·
tokens (`null` when the harness cannot observe them; never estimated) · visual quality (rubric
0-4 per criterion, by a reviewer, from `inspect`'s contact sheet) · artifact correctness (fraction
of mechanical gates passed).

**Allowed intervention.** Creative: a human may answer direction questions, approve a substitution,
or supply creative inputs the brief leaves open. Operational: nothing. Installing tools, fixing
paths/permissions, retrying, debugging the engine, editing manifests or interpreting errors for the
agent each count as an **operational intervention**, and the task then does not count as autonomous.

## Running a harness

1. Fresh checkout, engine built (`cargo build --release`), `vcr capabilities --json` recorded.
2. Per task: create `workdir/`, copy `fixtures/<task>/*` into it and `assets/` to
   `workdir/assets_pool/`. For T10 run `scripts/agent_bench/make_bad_artifacts.sh OUT` and give the
   agent only `OUT/artifacts` (keep `artifacts_truth.json` out of its sight). Give the agent only
   the task `brief` plus repo docs. Count interventions, repair attempts, tool calls, time.
3. After the run: write `run-meta.json` (`harness, model, session_ref, run_id,
   operational_interventions, repair_attempts, elapsed_seconds, tool_calls, tokens,
   visual_quality`) and score:
   ```bash
   python3 scripts/agent_bench/score.py task --task T01_alpha_lower_third --workdir WORKDIR \
       --evidence-kind autonomous_agent_run --run-meta run-meta.json --out records/T01.json
   python3 scripts/agent_bench/score.py summarize records/
   ```
4. Repeat for at least 3 runs per harness.

## Proposed release target (PROPOSED)

All mandatory deterministic contract checks pass; at least 9 of the 10 tasks pass (all gates and
zero operational interventions) in **each** tested harness in **each** of ≥ 3 repeated runs; every
delivered clip scores ≥ 3 on every criterion of its visual rubric. `summarize` evaluates the
autonomy part and reports `meets_proposed_autonomy_target` per harness; the visual rubric is
reviewed separately.

## Comparative claims

"Best available solution" requires running the same briefs through other tools/pipelines under the
same rules and scoring them with the same gates and rubric. That has not been done. Do not claim it.
