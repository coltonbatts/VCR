#!/usr/bin/env python3
"""Score VCR agent-benchmark tasks (mechanical gates only) and summarize runs.

  score.py task  --task T01_alpha_lower_third --workdir DIR --evidence-kind KIND [--run-meta meta.json] [--out record.json]
  score.py summarize RECORDS_DIR

Evidence kinds (a record is never promoted to a stronger kind than what produced it):
  scripted_reference        a script (not an agent) produced the artifacts; proves the gates, not autonomy
  deterministic_contract_check
  autonomous_agent_run      requires run-meta with harness, model, session_ref and run_id

This scorer measures ARTIFACT CORRECTNESS. Visual quality (rubric) and process metrics
(interventions, repair attempts, elapsed, tool calls, tokens) come from the operator's run-meta and
reviewer scores; they are recorded separately and never inferred here.
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parents[2]
BENCH = REPO / "docs" / "agent-benchmark"
KINDS = ("scripted_reference", "deterministic_contract_check", "autonomous_agent_run")
EXPECT_FLAGS = {
    "width": "--expect-width", "height": "--expect-height", "fps": "--expect-fps", "frames": "--expect-frames",
    "container": "--expect-container", "codec": "--expect-codec", "profile": "--expect-profile",
    "alpha_capable": "--expect-alpha-capable", "transparency": "--expect-transparency",
}


class Ctx:
    def __init__(self, task: dict, workdir: Path, engine: str):
        self.task, self.workdir, self.engine = task, workdir, engine

    def vcr(self, *args: str, timeout: int = 600) -> dict:
        proc = subprocess.run([self.engine, *args], cwd=self.workdir, capture_output=True, text=True, timeout=timeout)
        lines = [l for l in proc.stdout.splitlines() if l.strip()]
        try:
            return json.loads(lines[-1])
        except Exception:
            return {"status": "error", "error": {"code": "scorer.no_json", "message": proc.stderr[-300:]}}

    def globs(self, pattern: str) -> list[Path]:
        """Manifest selector. `@renders/x.mov` = the manifest that PRODUCED that output (from its
        provenance), which is robust against leftover or superseded manifests in the workdir.
        Otherwise `|`-separated globs relative to the workdir."""
        found: list[Path] = []
        self.unresolved = []
        for alt in pattern.split("|"):
            if alt.startswith("@"):
                prov = self.workdir / (alt[1:] + ".provenance.json")
                cand = None
                if prov.is_file():
                    cand = self.workdir / json.loads(prov.read_text())["manifest"]["path"]
                if cand is not None and cand.is_file():
                    found.append(cand)
                else:
                    self.unresolved.append(alt)
                continue
            found += [Path(p) for p in glob.glob(str(self.workdir / alt))]
        return sorted(set(found))

    def fixture(self, name: str) -> Path:
        return BENCH / "fixtures" / self.task["fixtures"] / name


def sha256(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def manifest_text_and_layers(ctx: Ctx, pattern: str):
    texts, kinds, raw = [], [], ""
    for m in ctx.globs(pattern):
        raw += m.read_text()
        try:
            doc = yaml.safe_load(m.read_text()) or {}
        except yaml.YAMLError:
            continue
        for layer in doc.get("layers", []) or []:
            if isinstance(layer, dict):
                if isinstance(layer.get("text"), dict):
                    texts.append(str(layer["text"].get("content", "")))
                for k in ("shader", "wgpu_shader", "video", "lottie"):
                    if k in layer:
                        kinds.append(k)
    return texts, kinds, raw


def gate(ctx: Ctx, g: dict) -> dict:
    t = g["type"]
    out = {"type": t, "passed": False, "detail": ""}

    if t in ("any_of", "all_of"):
        kids = [gate(ctx, c) for c in g["children"]]
        out["children"] = kids
        out["passed"] = any(k["passed"] for k in kids) if t == "any_of" else all(k["passed"] for k in kids)
        return out

    if t == "file_exists":
        out["passed"] = (ctx.workdir / g["path"]).is_file()
        out["detail"] = g["path"]

    elif t == "no_file":
        out["passed"] = not (ctx.workdir / g["path"]).exists()
        out["detail"] = g["path"]

    elif t == "verify":
        path = ctx.workdir / g["path"]
        if not path.is_file():
            out["detail"] = f"{g['path']} missing"
            return out
        args = ["verify", g["path"], "--json"]
        if g.get("manifest"):
            args += ["--manifest", g["manifest"]]
            for k, v in (g.get("set") or {}).items():
                args += ["--set", f"{k}={v}"]
        for k, v in (g.get("expect") or {}).items():
            args += [EXPECT_FLAGS[k], str(v).lower() if isinstance(v, bool) else str(v)]
        if g.get("require_provenance"):
            args.append("--require-provenance")
        doc = ctx.vcr(*args)
        out["passed"] = doc.get("status") == "ok"
        out["detail"] = ",".join(d["code"] for d in doc.get("diagnostics", []) if d["severity"] in ("error", "blocker")) or doc.get("status", "")

    elif t == "inspect_clean":
        manifests = ctx.globs(g["manifest"])
        if ctx.unresolved:
            out["detail"] = f"no producing manifest for {ctx.unresolved}"
            return out
        forbid, phases, ignore = set(g["forbid"]), set(g.get("phases") or []), set(g.get("ignore_layers") or [])
        hits, ran = [], 0
        for m in manifests:
            doc = ctx.vcr("inspect", str(m.relative_to(ctx.workdir)), "--json", "--samples", "8", "-o", "renders/_score_inspect")
            if doc.get("status") == "error":
                hits.append(f"{m.name}: inspect error {doc['error']['code']}")
                continue
            ran += 1
            phase_by_frame = {s["frame"]: s["phase"] for s in doc["result"]["samples"]}
            for d in doc.get("diagnostics", []):
                if d["code"] not in forbid:
                    continue
                if (d.get("location") or {}).get("layer") in ignore:
                    continue
                m_frame = re.search(r"frame (\d+)", d["message"])
                if phases and m_frame and phase_by_frame.get(int(m_frame.group(1))) not in phases:
                    continue
                hits.append(f"{m.name}: {d['code']} {d['message'][:80]}")
        out["passed"] = ran > 0 and not hits
        out["detail"] = "; ".join(hits) if hits else (f"{ran} manifest(s) clean" if ran else "no manifest matched")

    elif t in ("text_preserved", "words_preserved"):
        needles = g["strings"] if t == "text_preserved" else g["words"]
        problems = []
        manifests = ctx.globs(g["manifest"])
        problems += [f"no producing manifest for {u}" for u in ctx.unresolved]
        if not manifests:
            problems.append("no manifest matched")
        for m in manifests:
            texts, _, _ = manifest_text_and_layers(ctx, str(m.relative_to(ctx.workdir)))
            hay = [" ".join(texts)] if t == "words_preserved" else texts
            missing = [n for n in needles if not any(n in h for h in hay)]
            if missing:
                problems.append(f"{m.name} missing {missing}")
        out["passed"], out["detail"] = not problems, "; ".join(problems) or "ok"

    elif t == "manifest_contains":
        _, _, raw = manifest_text_and_layers(ctx, g["manifest"])
        missing = [s for s in g["substrings"] if s not in raw]
        out["passed"] = not missing and bool(raw)
        out["detail"] = f"missing {missing}" if missing else "ok"

    elif t == "manifest_forbids_layer_kinds":
        _, kinds, raw = manifest_text_and_layers(ctx, g["manifest"])
        bad = [k for k in kinds if k in g["kinds"]]
        out["passed"] = bool(raw) and not bad
        out["detail"] = f"found {bad}" if bad else "ok"

    elif t == "manifest_unchanged":
        out["passed"] = (ctx.workdir / g["manifest"]).is_file() and sha256(ctx.workdir / g["manifest"]) == sha256(ctx.fixture(g["manifest"]))
        out["detail"] = g["manifest"]

    elif t == "asset_identity":
        want = sha256(ctx.workdir / g["sha256_of"])
        ok = False
        for m in ctx.globs(g["manifest"]):
            doc = yaml.safe_load(m.read_text()) or {}
            for layer in doc.get("layers", []):
                if layer.get("id") == g["layer"] and isinstance(layer.get("image"), dict):
                    p = (m.parent / layer["image"]["path"])
                    ok = ok or (p.is_file() and sha256(p) == want)
        out["passed"] = ok
        out["detail"] = "layer image bytes match the supplied asset" if ok else "logo layer does not resolve to the supplied asset bytes"

    elif t == "variants_distinct":
        hashes = []
        for p in g["paths"]:
            prov = ctx.workdir / (p + ".provenance.json")
            hashes.append(json.loads(prov.read_text())["hashes"]["decoded_frames_sha256"] if prov.is_file() else None)
        out["passed"] = None not in hashes and len(set(hashes)) == len(hashes)
        out["detail"] = f"{len(set(hashes))} distinct of {len(hashes)}"

    elif t == "answer_json":
        path = ctx.workdir / g["path"]
        try:
            ans = json.loads(path.read_text())
        except Exception as exc:
            out["detail"] = f"answer unreadable: {exc}"
            return out
        ok, notes = True, []
        for k, v in (g.get("equals") or {}).items():
            if ans.get(k) != v:
                ok = False; notes.append(f"{k}={ans.get(k)!r} want {v!r}")
        for k, needles in (g.get("contains_any") or {}).items():
            if not any(n.lower() in str(ans.get(k, "")).lower() for n in needles):
                ok = False; notes.append(f"{k} lacks any of {needles}")
        if g.get("truth_file"):
            truth = json.loads((ctx.workdir.parent / g["truth_file"]).read_text()) if (ctx.workdir.parent / g["truth_file"]).is_file() else json.loads((ctx.workdir / g["truth_file"]).read_text())
            wrong = [f for f, want in truth.items() if ans.get(f, {}).get("acceptable") != want["acceptable"] or ans.get(f, {}).get("problem") != want["problem"]]
            if wrong:
                ok = False; notes.append(f"wrong: {wrong}")
            out["accuracy"] = 1 - len(wrong) / len(truth)
        out["passed"], out["detail"] = ok, "; ".join(notes) or "ok"

    else:
        out["detail"] = f"unknown gate type {t}"
    return out


def applicable(ctx: Ctx) -> tuple[bool, str]:
    cond = ctx.task.get("applicable_if")
    if not cond:
        return True, ""
    doc = ctx.vcr("capabilities", "--json")
    cur = doc.get("result", {})
    for part in cond["capability"].split("."):
        cur = cur.get(part) if isinstance(cur, dict) else None
    ok = cur == cond["equals"]
    return ok, f"{cond['capability']}={cur!r}"


def cmd_task(a) -> int:
    suite = yaml.safe_load((BENCH / "tasks.yaml").read_text())
    task = next((t for t in suite["tasks"] if t["id"] == a.task), None)
    if not task:
        sys.exit(f"unknown task {a.task}")
    meta = json.loads(Path(a.run_meta).read_text()) if a.run_meta else {}
    if a.evidence_kind == "autonomous_agent_run":
        missing = [k for k in ("harness", "model", "session_ref", "run_id") if not meta.get(k)]
        if missing:
            sys.exit(f"refusing to label as autonomous_agent_run: run-meta lacks {missing}")
    ctx = Ctx(task, Path(a.workdir).resolve(), a.engine)
    ok_app, why = applicable(ctx)
    record = {
        "suite_version": suite["suite_version"], "task_id": task["id"], "evidence_kind": a.evidence_kind,
        "engine": ctx.vcr("capabilities", "--json").get("engine"), "applicable": ok_app, "applicability": why,
        "process": {k: meta.get(k) for k in ("operational_interventions", "repair_attempts", "elapsed_seconds", "tool_calls", "tokens")},
        "attestation": {k: meta.get(k) for k in ("harness", "model", "session_ref", "run_id")},
        "visual_quality": meta.get("visual_quality"),
    }
    if not ok_app:
        record.update(gates=[], artifact_correctness=None, task_completion=None, passed=None, status="not_applicable")
    else:
        gates = [gate(ctx, g) for g in task["gates"]]
        frac = sum(g["passed"] for g in gates) / len(gates)
        record.update(gates=gates, artifact_correctness=frac, task_completion=frac == 1.0)
        interventions = meta.get("operational_interventions")
        record["passed"] = bool(frac == 1.0 and interventions == 0) if a.evidence_kind == "autonomous_agent_run" else None
        record["status"] = "scored"
    text = json.dumps(record, indent=2)
    if a.out:
        Path(a.out).parent.mkdir(parents=True, exist_ok=True)
        Path(a.out).write_text(text + "\n")
    print(text)
    return 0 if record.get("task_completion") in (True, None) else 1


def cmd_summarize(a) -> int:
    recs = [json.loads(p.read_text()) for p in sorted(Path(a.records).glob("*.json"))]
    agent = [r for r in recs if r["evidence_kind"] == "autonomous_agent_run"]
    other = {k: sum(1 for r in recs if r["evidence_kind"] == k) for k in KINDS if k != "autonomous_agent_run"}
    summary = {"records": len(recs), "non_autonomous_records": other, "autonomous_records": len(agent), "harnesses": {}}
    by = {}
    for r in agent:
        by.setdefault(r["attestation"]["harness"], {}).setdefault(r["attestation"]["run_id"], []).append(r)
    for harness, runs in by.items():
        per_run = []
        for run_id, rs in runs.items():
            tasks = {}
            for r in rs:
                tid = r["task_id"][:3] if r["task_id"].startswith("T08") else r["task_id"][:3]
                tasks.setdefault(tid, []).append(r)
            passed = sum(1 for rr in tasks.values() if all(x["passed"] for x in rr if x["status"] == "scored") and any(x["status"] == "scored" for x in rr))
            per_run.append({"run_id": run_id, "tasks_scored": len(tasks), "passed_without_operational_intervention": passed})
        meets = len(per_run) >= 3 and all(p["tasks_scored"] == 10 and p["passed_without_operational_intervention"] >= 9 for p in per_run)
        summary["harnesses"][harness] = {"runs": per_run, "meets_proposed_autonomy_target": meets,
                                         "note": "needs >= 3 complete runs of 10 tasks, each with >= 9 passes; visual rubric reviewed separately"}
    if not agent:
        summary["verdict"] = "NO AUTONOMOUS-AGENT EVIDENCE: nothing here supports an autonomy claim"
    print(json.dumps(summary, indent=2))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    t = sub.add_parser("task")
    t.add_argument("--task", required=True)
    t.add_argument("--workdir", required=True)
    t.add_argument("--evidence-kind", required=True, choices=KINDS)
    t.add_argument("--run-meta")
    t.add_argument("--engine", default=str(REPO / "target" / "release" / "vcr"))
    t.add_argument("--out")
    s = sub.add_parser("summarize")
    s.add_argument("records")
    a = ap.parse_args()
    return cmd_task(a) if a.cmd == "task" else cmd_summarize(a)


if __name__ == "__main__":
    sys.exit(main())
