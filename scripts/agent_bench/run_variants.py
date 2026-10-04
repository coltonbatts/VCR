#!/usr/bin/env python3
"""Generate and exercise the 12-variant lower-third matrix through the full engine workflow.

evidence_kind = scripted_reference (a script, not an agent). It proves the scene system and the
verification chain hold across the matrix; it does not measure agent autonomy.

usage: run_variants.py [--engine PATH] [--out DIR] [--only ID ...] [--repeat-check]
"""

import argparse
import itertools
import json
import re
import subprocess
import sys
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parents[2]
MATRIX = REPO / "docs" / "agent-first" / "proof" / "variant_matrix.yaml"
CHAR_W = 0.56  # measured GeistPixel advance in em (approximate; exact bounds are checked after render)


def vcr(engine, cwd, *args):
    p = subprocess.run([str(engine), *args], cwd=cwd, capture_output=True, text=True)
    lines = [l for l in p.stdout.splitlines() if l.strip()]
    return json.loads(lines[-1])


def make_manifest(base: str, text: dict, right_override: float | None = None) -> str:
    """Substitute copy and size the panel to the text (geometry is static in the manifest)."""
    name, title = text["name"], text["title"]
    need = 132 + max(len(name) * CHAR_W * 64, len(title) * CHAR_W * 36) + 48
    right = right_override or max(936.0, round(need))
    width = right - 96
    cx, sx = (96 + right) / 2 / 1920, width / 1920
    out = base.replace('content: "SAMPLE NAME"', f'content: "{name}"').replace('content: "SAMPLE TITLE"', f'content: "{title}"')
    out, n = re.subn(r"center: \{ x: 0\.26875, y: 0\.836111 \}", f"center: {{ x: {cx:.6f}, y: 0.836111 }}", out)
    out2, m = re.subn(r"size: \{ x: 0\.4375, y: 0\.153704 \}", f"size: {{ x: {sx:.6f}, y: 0.153704 }}", out)
    assert n == 1 and m == 1, "panel geometry anchors not found in template"
    return out2


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", default=str(REPO / "target" / "release" / "vcr"))
    ap.add_argument("--out", default=str(REPO / "renders" / "lower_third_variants"))
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--repeat-check", action="store_true")
    a = ap.parse_args()
    spec = yaml.safe_load(MATRIX.read_text())
    base = (REPO / spec["scene"]).read_text()
    out = Path(a.out)
    mdir = out / "manifests"
    mdir.mkdir(parents=True, exist_ok=True)
    rows, decoded, kept = [], {}, {}
    for palette, motion, text in itertools.product(spec["axes"]["palette"], spec["axes"]["motion"], spec["axes"]["text"]):
        vid = f"lt_v1_{palette}_{motion}_{text}"
        if a.only and not any(vid.startswith(o) or o in vid for o in a.only):
            continue
        man = mdir / f"{vid}.vcr"
        man.write_text(make_manifest(base, spec["axes"]["text"][text]))
        sets = []
        for k, v in {**spec["axes"]["palette"][palette], **spec["axes"]["motion"][motion]}.items():
            sets += ["--set", f"{k}={v}"]
        rel = str(man.relative_to(REPO))
        kept[vid] = (rel, sets)
        row = {"id": vid, "stages": {}}
        try:
            for stage in ("check", "lint"):
                d = vcr(a.engine, REPO, "--backend", "software", stage, rel, *sets, "--json")
                row["stages"][stage] = d["status"]
                assert d["status"] == "ok", (stage, d.get("error") or d.get("diagnostics"))
            pre = vcr(a.engine, REPO, "--backend", "software", "explain", rel, *sets, "--json")
            assert pre["result"]["backend_preflight"]["ready"], pre
            row["stages"]["preflight"] = "ok"
            right = None
            for attempt in range(3):  # inspect -> measure -> revise the one element that is wrong
                man.write_text(make_manifest(base, spec["axes"]["text"][text], right))
                ins = vcr(a.engine, REPO, "--backend", "software", "inspect", rel, *sets, "--samples", "8", "--json", "-o", f"{out.relative_to(REPO)}/{vid}_inspect")
                hold_b = [{x["id"]: x["bbox"] for x in smp["layer_bounds"]} for smp in ins["result"]["samples"] if smp["phase"] == "hold"]
                widest = max((b[t]["x1"] for b in hold_b for t in ("name_text", "title_text") if b.get(t)), default=0)
                panel_right = max((b["panel"]["x1"] for b in hold_b if b.get("panel")), default=0)
                if widest + 40 <= panel_right + 1 and widest + 56 >= panel_right:
                    break
                right = widest + 48 + 1  # panel right edge: text end + padding
                row.setdefault("refits", []).append(round(right))
            bad = [d["code"] for d in ins.get("diagnostics", []) if d["code"] in ("layout.touches_canvas_edge", "layer.renders_nothing", "timing.nothing_visible", "timing.motion_on_last_frame") and d["severity"] != "info"]
            hold = [s for s in ins["result"]["samples"] if s["phase"] == "hold"]
            assert hold, "no hold sample"
            for s in hold:
                b = {x["id"]: x["bbox"] for x in s["layer_bounds"]}
                panel = b["panel"]
                for t in ("name_text", "title_text"):
                    tb = b[t]
                    assert tb and panel["x0"] <= tb["x0"] and tb["x1"] <= panel["x1"] and panel["y0"] <= tb["y0"] and tb["y1"] <= panel["y1"], f"{t} not inside panel at frame {s['frame']}: {tb} vs {panel}"
            assert not bad, bad
            row["stages"]["inspect"] = "ok"
            mov = f"{out.relative_to(REPO)}/{vid}.mov"
            r = vcr(a.engine, REPO, "--backend", "software", "render", rel, "-o", mov, *sets, "--json")
            assert r["status"] == "ok", r.get("error")
            v = vcr(a.engine, REPO, "verify", mov, "--manifest", rel, *sets, "--expect-width", "1920", "--expect-height", "1080", "--expect-fps", "60", "--expect-frames", "300", "--expect-transparency", "required", "--require-provenance", "--json")
            assert v["status"] == "ok", [d["code"] for d in v.get("diagnostics", [])]
            t = v["result"]["media"]["transparency"]
            assert t["min_alpha"] == 0 and t["frames_with_transparency"] > 0
            row["stages"]["render+verify"] = "ok"
            prov = json.loads((REPO / (mov + ".provenance.json")).read_text())
            decoded[vid] = prov["hashes"]["decoded_frames_sha256"]
            row["output_sha256"] = prov["hashes"]["output_file_sha256"][:16]
            row["passed"] = True
        except AssertionError as exc:
            row["passed"], row["failure"] = False, str(exc)[:300]
        rows.append(row)
        print(("PASS " if row["passed"] else "FAIL ") + vid + ("" if row["passed"] else "  " + row["failure"]), flush=True)
    distinct = len(set(decoded.values())) == len(decoded)
    repro = None
    if a.repeat_check and decoded:
        vid = next(iter(decoded))
        man, sets = kept[vid]
        mov2 = f"{out.relative_to(REPO)}/{vid}_repeat.mov"
        vcr(a.engine, REPO, "--backend", "software", "render", man, "-o", mov2, *sets, "--json")
        first = json.loads((REPO / f"{out.relative_to(REPO)}/{vid}.mov.provenance.json").read_text())
        second = json.loads((REPO / (mov2 + ".provenance.json")).read_text())
        repro = {lvl: first["hashes"][lvl] == second["hashes"][lvl] for lvl in ("raster_frames_sha256", "decoded_frames_sha256", "output_file_sha256")}
    report = {"evidence_kind": "scripted_reference", "variants": rows, "all_passed": all(r["passed"] for r in rows), "decoded_hashes_distinct": distinct, "reproducibility_recheck": repro}
    (out / "variant_report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k != "variants"}, indent=2))
    return 0 if report["all_passed"] and distinct and (repro is None or all(repro.values())) else 1


if __name__ == "__main__":
    sys.exit(main())
