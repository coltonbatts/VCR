#!/usr/bin/env python3
"""Scripted reference solutions + negative controls for the agent benchmark.

THIS IS NOT AGENT EVIDENCE. It shows that (a) every gate can be satisfied through the public CLI
surface and (b) the gates reject wrong solutions. Records are labelled `scripted_reference`.

usage: reference_run.py [--engine PATH] [--root DIR] [--only T01 T06 ...]
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BENCH = REPO / "docs" / "agent-benchmark"
SCORE = REPO / "scripts" / "agent_bench" / "score.py"


class Run:
    def __init__(self, engine: str, root: Path):
        self.engine, self.root = engine, root
        self.rows: list[dict] = []

    def workdir(self, name: str, fixtures: str | None = None, pool: bool = True) -> Path:
        wd = self.root / name / "workdir"
        shutil.rmtree(wd.parent, ignore_errors=True)
        wd.mkdir(parents=True)
        if fixtures:
            for f in (BENCH / "fixtures" / fixtures).iterdir():
                (shutil.copytree if f.is_dir() else shutil.copy)(f, wd / f.name)
        if pool:
            shutil.copytree(BENCH / "assets", wd / "assets_pool")
        return wd

    def vcr(self, wd: Path, *args: str) -> dict:
        p = subprocess.run([self.engine, *args], cwd=wd, capture_output=True, text=True)
        lines = [l for l in p.stdout.splitlines() if l.strip()]
        return json.loads(lines[-1]) if lines else {"status": "error"}

    def author(self, wd: Path, ref: str, dest: str) -> str:
        shutil.copy(BENCH / "reference" / ref, wd / dest)
        return dest

    def pipeline(self, wd: Path, manifest: str, out: str, **render_kw) -> None:
        """The workflow an agent follows: validate -> lint -> preflight -> inspect -> render."""
        for stage in (["check", manifest], ["lint", manifest], ["explain", manifest]):
            d = self.vcr(wd, "--backend", "software", *stage, "--json")
            assert d["status"] in ("ok",), (stage, d.get("error") or d.get("diagnostics"))
        pre = self.vcr(wd, "--backend", "software", "explain", manifest, "--json")
        assert pre["result"]["backend_preflight"]["ready"], pre
        ins = self.vcr(wd, "--backend", "software", "inspect", manifest, "--json", "-o", f"renders/{Path(out).stem}_inspect")
        assert ins["status"] in ("ok", "failed"), ins
        r = self.vcr(wd, "--backend", "software", "render", manifest, "-o", out, "--json")
        assert r["status"] == "ok", r

    def score(self, task: str, wd: Path, label: str, expect_pass: bool) -> None:
        rec = self.root / label / "record.json"
        p = subprocess.run(
            [sys.executable, str(SCORE), "task", "--task", task, "--workdir", str(wd), "--evidence-kind", "scripted_reference", "--engine", self.engine, "--out", str(rec)],
            capture_output=True, text=True,
        )
        doc = json.loads(rec.read_text())
        completed = doc.get("task_completion")
        if doc["status"] == "not_applicable":
            verdict, ok = "n/a", True
        else:
            verdict = "PASS" if completed else "FAIL"
            ok = completed == expect_pass
        failing = [f"{g['type']}:{g['detail']}"[:90] for g in doc["gates"] if not g["passed"]]
        self.rows.append({"label": label, "task": task, "verdict": verdict, "expected": "pass" if expect_pass else "fail (negative control)", "as_expected": ok, "failing_gates": failing})


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", default=str(REPO / "target" / "release" / "vcr"))
    ap.add_argument("--root", default=str(REPO / "renders" / "agent_bench" / "reference"))
    ap.add_argument("--only", nargs="*")
    a = ap.parse_args()
    run = Run(a.engine, Path(a.root))
    want = lambda t: not a.only or any(t.startswith(x) for x in a.only)

    if want("T01"):
        wd = run.workdir("T01")
        m = run.author(wd, "t01_lower_third.vcr", "t01_lower_third.vcr")
        run.pipeline(wd, m, "renders/t01_lower_third.mov")
        run.score("T01_alpha_lower_third", wd, "T01", True)
        # negative control: an opaque background kills the transparency requirement
        wd = run.workdir("T01_neg_opaque")
        txt = (BENCH / "reference" / "t01_lower_third.vcr").read_text().replace("layers:\n", "layers:\n  - id: backdrop\n    z_index: 0\n    procedural: { kind: solid_color, color: { r: 0.1, g: 0.1, b: 0.1, a: 1.0 } }\n", 1)
        (wd / "t01_lower_third.vcr").write_text(txt)
        run.vcr(wd, "--backend", "software", "render", "t01_lower_third.vcr", "-o", "renders/t01_lower_third.mov", "--json")
        run.score("T01_alpha_lower_third", wd, "T01_neg_opaque", False)

    if want("T02"):
        wd = run.workdir("T02")
        m = run.author(wd, "t02_titles.vcr", "t02_titles.vcr")
        run.pipeline(wd, m, "renders/t02_titles.mov")
        run.score("T02_title_sequence", wd, "T02", True)

    if want("T03"):
        wd = run.workdir("T03")
        m = run.author(wd, "t03_intro.vcr", "t03_intro.vcr")
        run.pipeline(wd, m, "renders/t03_intro.mov")
        run.score("T03_branded_intro", wd, "T03", True)

    if want("T04"):
        wd = run.workdir("T04")
        m = run.author(wd, "t04_photo.vcr", "t04_photo.vcr")
        run.pipeline(wd, m, "renders/t04_photo.mov")
        run.score("T04_supplied_image_scene", wd, "T04", True)

    if want("T05"):
        wd = run.workdir("T05", "aspect_adaptation")
        for name in ("t05_vertical", "t05_square"):
            m = run.author(wd, f"{name}.vcr", f"{name}.vcr")
            run.pipeline(wd, m, f"renders/{name}.mov")
        run.score("T05_aspect_adaptation", wd, "T05", True)
        # negative control: the 16:9 source rendered at 720x1280 without reflow clips
        wd = run.workdir("T05_neg", "aspect_adaptation")
        src = (wd / "source_16x9.vcr").read_text().replace("width: 1280, height: 720", "width: 720, height: 1280")
        (wd / "t05_vertical.vcr").write_text(src)
        (wd / "t05_square.vcr").write_text(src.replace("width: 720, height: 1280", "width: 720, height: 720"))
        for n in ("t05_vertical", "t05_square"):
            run.vcr(wd, "--backend", "software", "render", f"{n}.vcr", "-o", f"renders/{n}.mov", "--json")
        run.score("T05_aspect_adaptation", wd, "T05_neg", False)

    if want("T06"):
        wd = run.workdir("T06", "long_text_repair")
        # the workflow: inspect finds the defect first
        before = run.vcr(wd, "--backend", "software", "inspect", "overflow.vcr", "--json", "-o", "renders/before")
        assert any(d["code"] == "layout.touches_canvas_edge" for d in before["diagnostics"]), "defect must be detected before repair"
        m = run.author(wd, "t06_fixed.vcr", "t06_fixed.vcr")
        run.pipeline(wd, m, "renders/t06_fixed.mov")
        run.score("T06_long_text_repair", wd, "T06", True)
        # negative control: submitting the broken fixture, rendered, must fail
        wd = run.workdir("T06_neg", "long_text_repair")
        run.vcr(wd, "--backend", "software", "render", "overflow.vcr", "-o", "renders/t06_fixed.mov", "--json")
        run.score("T06_long_text_repair", wd, "T06_neg", False)

    if want("T07"):
        wd = run.workdir("T07", "parameterized_variants")
        for suffix, accent, width in (("a", "#FF5533", "0.30"), ("b", "#33C3FF", "0.50"), ("c", "#FFD633", "0.70")):
            r = run.vcr(wd, "--backend", "software", "render", "base.vcr", "-o", f"renders/t07_{suffix}.mov", "--set", f"accent={accent}", "--set", f"bar_width={width}", "--json")
            assert r["status"] == "ok", r
        run.score("T07_parameterized_variants", wd, "T07", True)
        # negative control: ignoring the overrides gives three identical files
        wd = run.workdir("T07_neg", "parameterized_variants")
        for s in "abc":
            run.vcr(wd, "--backend", "software", "render", "base.vcr", "-o", f"renders/t07_{s}.mov", "--json")
        run.score("T07_parameterized_variants", wd, "T07_neg", False)

    if want("T08"):
        wd = run.workdir("T08a", "missing_asset")
        chk = run.vcr(wd, "check", "manifest.vcr", "--json")
        assert chk["error"]["code"] == "asset.missing" and chk["error"]["observed"] == "assets/logo_mark.png", chk
        (wd / "assets").mkdir()
        shutil.copy(wd / "assets_pool" / "logo_mark.png", wd / "assets" / "logo_mark.png")
        run.pipeline(wd, "manifest.vcr", "renders/t08a.mov")
        run.score("T08a_missing_asset_recoverable", wd, "T08a", True)
        # negative control: an invented replacement logo
        wd = run.workdir("T08a_neg", "missing_asset")
        (wd / "assets").mkdir()
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=512x512", "-frames:v", "1", str(wd / "assets" / "logo_mark.png")], check=True)
        run.vcr(wd, "--backend", "software", "render", "manifest.vcr", "-o", "renders/t08a.mov", "--json")
        run.score("T08a_missing_asset_recoverable", wd, "T08a_neg", False)

        wd = run.workdir("T08b", "missing_asset_unavailable", pool=False)
        chk = run.vcr(wd, "check", "manifest.vcr", "--json")
        assert chk["error"]["code"] == "asset.missing", chk
        (wd / "answer.json").write_text(json.dumps({"status": "blocked", "reason": f"asset missing: {chk['error']['observed']} (client_logo) is not available", "needs": "the client logo file"}))
        run.score("T08b_missing_asset_unavailable", wd, "T08b", True)
        wd = run.workdir("T08b_neg", "missing_asset_unavailable", pool=False)
        (wd / "assets").mkdir()
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=blue:s=512x512", "-frames:v", "1", str(wd / "assets" / "client_logo.png")], check=True)
        run.vcr(wd, "--backend", "software", "render", "manifest.vcr", "-o", "renders/t08b.mov", "--json")
        (wd / "answer.json").write_text(json.dumps({"status": "delivered", "reason": "made a logo"}))
        run.score("T08b_missing_asset_unavailable", wd, "T08b_neg", False)

    if want("T09"):
        wd = run.workdir("T09")
        shader = "version: 1\nenvironment:\n  resolution: { width: 1280, height: 720 }\n  fps: 30\n  duration: 2.0\nlayers:\n  - id: fx\n    shader:\n      fragment: |\n        fn shade(uv: vec2<f32>, u: ShaderUniforms) -> vec4<f32> { return vec4<f32>(uv.x, uv.y, 0.5, 1.0); }\n"
        (wd / "t09.vcr").write_text(shader)
        pre = run.vcr(wd, "--backend", "auto", "explain", "t09.vcr", "--json")
        b = pre["result"]["backend_preflight"]
        if b["ready"]:
            run.rows.append({"label": "T09", "task": "T09_unsupported_backend_recovery", "verdict": "n/a", "expected": "n/a (GPU usable here)", "as_expected": True, "failing_gates": []})
        else:
            codes = [c["code"] for c in b["checks"]]
            (wd / "answer.json").write_text(json.dumps({"status": "blocked", "reason": f"gpu unavailable; shader layer cannot render on the software backend ({', '.join(codes)})", "requires_approval": True}))
            run.score("T09_unsupported_backend_recovery", wd, "T09", True)
            # negative control: silently claiming delivery with no output
            wd = run.workdir("T09_neg")
            (wd / "answer.json").write_text(json.dumps({"status": "delivered", "reason": "done"}))
            run.score("T09_unsupported_backend_recovery", wd, "T09_neg", False)

    if want("T10"):
        root = run.root / "T10"
        shutil.rmtree(root, ignore_errors=True)
        subprocess.run([str(REPO / "scripts/agent_bench/make_bad_artifacts.sh"), str(root), a.engine], check=True, capture_output=True)
        wd = root / "workdir"
        wd.mkdir()
        shutil.copytree(root / "artifacts", wd / "artifacts")
        codes_map = {"verify.unreadable_media": "unreadable", "verify.modified_since_render": "modified", "verify.stale_artifact": "stale",
                     "verify.codec_mismatch": "wrong_codec", "verify.frame_count_mismatch": "wrong_duration", "verify.duration_seconds_mismatch": "wrong_duration",
                     "verify.transparency_mismatch": "no_transparency"}
        answer = {}
        for f in sorted(p.name for p in (wd / "artifacts").glob("*.mov")):
            d = run.vcr(wd, "verify", f"artifacts/{f}", "--manifest", "artifacts/scene.vcr", "--expect-transparency", "required", "--json")
            found = [codes_map[x["code"]] for x in d.get("diagnostics", []) if x["code"] in codes_map]
            order = ["unreadable", "modified", "stale", "wrong_codec", "wrong_duration", "no_transparency"]
            problem = next((o for o in order if o in found), "ok")
            answer[f] = {"acceptable": problem == "ok", "problem": problem}
        (wd / "answer.json").write_text(json.dumps(answer))
        shutil.copy(root / "artifacts_truth.json", wd / "artifacts_truth.json")
        run.score("T10_encoded_artifact_verification", wd, "T10", True)
        # negative control: judging by file extension / pix_fmt alone ("everything is fine")
        naive = {f: {"acceptable": True, "problem": "ok"} for f in answer}
        (wd / "answer.json").write_text(json.dumps(naive))
        run.score("T10_encoded_artifact_verification", wd, "T10_neg", False)

    print(json.dumps(run.rows, indent=2))
    bad = [r for r in run.rows if not r["as_expected"]]
    print(f"\n{len(run.rows)} scored: {len(run.rows) - len(bad)} as expected, {len(bad)} NOT as expected", file=sys.stderr)
    print("evidence_kind=scripted_reference: this does not measure agent autonomy", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
