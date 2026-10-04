#!/usr/bin/env python3
"""Run the mandatory deterministic contract checks and emit one JSON report.

evidence_kind = deterministic_contract_check. This is NOT agent autonomy evidence.

usage: contract_checks.py [--out renders/agent_bench/contract_report.json] [--skip-reference]
"""

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
ENGINE = REPO / "target" / "release" / "vcr"


def step(name, cmd, cwd=REPO):
    t = time.time()
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    tail = (p.stdout + p.stderr).strip().splitlines()[-3:]
    return {"name": name, "passed": p.returncode == 0, "seconds": round(time.time() - t, 1), "command": " ".join(map(str, cmd)), "tail": tail}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(REPO / "renders" / "agent_bench" / "contract_report.json"))
    ap.add_argument("--skip-reference", action="store_true")
    a = ap.parse_args()
    steps = [
        step("build release engine", ["cargo", "build", "--release"]),
        step("library unit tests (contract, schema table, inspect, capabilities)", ["cargo", "test", "--lib"]),
        step("CLI contract tests (parsed JSON structure and meaning)", ["cargo", "test", "--test", "agent_contract_cli"]),
        step("existing CLI/determinism regressions", ["cargo", "test", "--test", "cli_contract", "--test", "determinism", "--test", "params_reliability", "--test", "agent_error_reporting"]),
        step("MCP adapter tests (selection, timeout cleanup, parity, workflow)", [sys.executable, "-m", "unittest", "discover", "-s", "scripts/vcr-mcp-server/tests"]),
    ]
    if not a.skip_reference:
        steps.append(step("scripted reference solutions + negative controls (gates reject wrong work)", [sys.executable, "scripts/agent_bench/reference_run.py"]))
    identity = subprocess.run([str(ENGINE), "capabilities", "--json"], capture_output=True, text=True)
    engine = json.loads(identity.stdout.strip().splitlines()[-1]).get("engine") if identity.returncode == 0 else None
    report = {
        "evidence_kind": "deterministic_contract_check",
        "claim_scope": "contract and tooling behave as specified; says nothing about agent autonomy",
        "engine": engine,
        "all_passed": all(s["passed"] for s in steps),
        "steps": steps,
    }
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(report, indent=2) + "\n")
    for s in steps:
        print(f"{'PASS' if s['passed'] else 'FAIL'}  {s['seconds']:>6}s  {s['name']}")
    print(f"report: {a.out}")
    return 0 if report["all_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
