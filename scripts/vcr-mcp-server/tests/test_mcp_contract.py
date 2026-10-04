"""MCP adapter tests: engine selection, timeout cleanup, protocol errors, CLI parity, workflow.

Run: python3 -m unittest discover -s scripts/vcr-mcp-server/tests -v
Needs a built engine (cargo build) and ffmpeg for the workflow tests; they skip otherwise.
"""

import json
import os
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import engine as eng  # noqa: E402
import server  # noqa: E402

REPO = HERE.parent.parent.parent


def built_engine():
    for profile in ("release", "debug"):
        p = REPO / "target" / profile / "vcr"
        if p.is_file():
            return str(p)
    return None


ENGINE = built_engine()
HAVE_FFMPEG = bool(shutil.which("ffmpeg") and shutil.which("ffprobe"))

SMALL = """version: 1
environment:
  resolution: { width: 64, height: 36 }
  fps: 10
  duration: { frames: 6 }
  encoding: { prores_profile: prores4444 }
layers:
  - id: dot
    procedural:
      kind: circle
      center: { x: 0.5, y: 0.5 }
      radius: 0.2
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
"""


def fake_engine(path: Path, body: str):
    path.write_text("#!/bin/sh\n" + textwrap.dedent(body))
    path.chmod(path.stat().st_mode | stat.S_IEXEC)
    return str(path)


CAPS_JSON = lambda git: json.dumps({"contract": "vcr.agent/1", "operation": "capabilities", "ok": True, "status": "ok",
                                    "engine": {"name": "vcr", "version": "9.9.9", "git_hash": git}, "result": {}})


class EngineSelection(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        eng._IDENTITY_CACHE.clear()

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_old_engine_without_contract_is_rejected_not_used(self):
        old = fake_engine(self.tmp / "vcr", 'echo "error: unrecognized subcommand capabilities" >&2; exit 2\n')
        with self.assertRaises(eng.EngineUnavailable) as ctx:
            eng.select_engine(self.tmp, explicit=old)
        err = ctx.exception.envelope["error"]
        self.assertEqual(err["code"], "engine.incompatible")
        self.assertIn("predates", json.dumps(err["observed"]))

    def test_explicit_beats_everything_and_mismatch_is_reported(self):
        a = fake_engine(self.tmp / "a", f"echo '{CAPS_JSON('aaaaaaa')}'\n")
        b = fake_engine(self.tmp / "b", f"echo '{CAPS_JSON('bbbbbbb')}'\n")
        old_env = os.environ.get("VCR_BIN")
        os.environ["VCR_BIN"] = b
        try:
            engine = eng.select_engine(self.tmp, explicit=a)
        finally:
            os.environ.pop("VCR_BIN", None)
            if old_env:
                os.environ["VCR_BIN"] = old_env
        self.assertEqual(engine.path, a)
        self.assertEqual(engine.reason, "explicit engine_path argument")
        self.assertEqual(engine.identity["git_hash"], "aaaaaaa")
        self.assertTrue(any("bbbbbbb" in w and "aaaaaaa" in w for w in engine.warnings), engine.warnings)

    def test_checkout_build_precedes_path(self):
        root = self.tmp / "checkout"
        (root / "target" / "debug").mkdir(parents=True)
        fake_engine(root / "target" / "debug" / "vcr", f"echo '{CAPS_JSON('checkout')}'\n")
        bindir = self.tmp / "bin"
        bindir.mkdir()
        fake_engine(bindir / "vcr", f"echo '{CAPS_JSON('pathold')}'\n")
        old_path = os.environ["PATH"]
        os.environ["PATH"] = f"{bindir}:{old_path}"
        os.environ.pop("VCR_BIN", None)
        try:
            engine = eng.select_engine(root)
        finally:
            os.environ["PATH"] = old_path
        self.assertEqual(engine.identity["git_hash"], "checkout")
        self.assertEqual(engine.reason, "checkout build")
        self.assertTrue(any("pathold" in w for w in engine.warnings))

    def test_protocol_violation_is_an_error_document(self):
        e = fake_engine(self.tmp / "e", f"""
            case "$*" in *capabilities*) echo '{CAPS_JSON('x')}';; *) echo "not json at all";; esac
        """)
        engine = eng.select_engine(self.tmp, explicit=e)
        doc = eng.run_engine(engine, "check", ["check", "m.vcr"], cwd=self.tmp)
        self.assertEqual(doc["error"]["code"], "engine.protocol_error")
        self.assertEqual(doc["status"], "error")

    def test_timeout_kills_the_whole_process_group(self):
        pidfile = self.tmp / "child.pid"
        e = fake_engine(self.tmp / "slow", f"""
            case "$*" in *capabilities*) echo '{CAPS_JSON('x')}'; exit 0;; esac
            sleep 60 &
            echo $! > {pidfile}
            wait
        """)
        engine = eng.select_engine(self.tmp, explicit=e)
        started = time.time()
        doc = eng.run_engine(engine, "render", ["render", "m.vcr"], cwd=self.tmp, timeout=1.0)
        self.assertLess(time.time() - started, 15)
        self.assertEqual(doc["error"]["code"], "timeout.exceeded")
        self.assertTrue(doc["error"]["retryable"])
        child = int(pidfile.read_text().strip())
        time.sleep(0.3)
        try:
            state = Path(f"/proc/{child}/stat").read_text().rsplit(")", 1)[1].split()[0]
        except FileNotFoundError:
            state = "gone"
        self.assertIn(state, ("gone", "Z"), "grandchild must be dead (a zombie awaiting reaping counts), not running")

    def test_path_escape_is_refused(self):
        with self.assertRaises(eng.PathRefused):
            eng.resolve_in_project(self.tmp, "../outside.vcr")


@unittest.skipUnless(ENGINE, "build the engine first: cargo build")
class Workflow(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.kw = dict(project_root=str(self.root), engine_path=ENGINE)

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def cli(self, *args):
        out = subprocess.run([ENGINE, *args], capture_output=True, text=True, cwd=self.root)
        return json.loads(out.stdout.strip().splitlines()[-1]), out.returncode

    def test_capabilities_reports_selected_engine(self):
        doc = server.vcr_capabilities(**self.kw)
        self.assertEqual(doc["status"], "ok")
        self.assertEqual(doc["adapter"]["engine_path"], ENGINE)
        self.assertEqual(doc["adapter"]["engine_identity"]["git_hash"], doc["engine"]["git_hash"])
        self.assertTrue(any(l["kind"] == "text" for l in doc["result"]["layers"]))

    def test_normalization_parity_with_cli_including_blockers(self):
        for brief in ("5s alpha lower third 1280x720 at 24fps", "3s title card", "make a lower third"):
            cli, _ = self.cli("prompt", "--json", "--text", brief)
            mcp_doc = server.vcr_normalize_brief(brief_text=brief, **self.kw)
            self.assertEqual(mcp_doc["status"], cli["status"], brief)
            self.assertEqual(mcp_doc["result"]["normalized_spec"], cli["result"]["normalized_spec"], brief)
            self.assertEqual(mcp_doc["result"]["unknowns_and_fixes"], cli["result"]["unknowns_and_fixes"], brief)
            self.assertEqual(mcp_doc["result"]["defaults_applied"], cli["result"]["defaults_applied"], brief)
        blocked = server.vcr_normalize_brief(brief_text="make a lower third", **self.kw)
        self.assertEqual(blocked["status"], "blocked")
        self.assertFalse(blocked["ok"])

    def test_render_plan_has_no_adapter_side_defaults(self):
        plan = server.vcr_render_plan(brief_text="3s title card", **self.kw)
        self.assertEqual(plan["result"]["normalized_spec"]["render"]["fps"], 60)  # engine default, not 24
        self.assertEqual(plan["result"]["normalized_spec"]["render"]["frames"], 180)
        blocked = server.vcr_render_plan(brief_text="make a lower third", **self.kw)
        self.assertEqual(blocked["status"], "blocked")
        self.assertNotIn("plan", blocked["result"])

    def test_validate_stages_and_first_failure(self):
        good = server.vcr_validate(manifest_yaml=SMALL, **self.kw)
        self.assertTrue(good["ready_to_render"], json.dumps(good)[:600])
        self.assertEqual(good["first_failing_stage"], None)
        bad = server.vcr_validate(manifest_yaml=SMALL.replace("id: dot", "id: dot\n    nope: 1"), **self.kw)
        self.assertEqual(bad["first_failing_stage"], "check")
        self.assertEqual(bad["stages"]["check"]["error"]["code"], "manifest.schema_invalid")
        self.assertEqual(bad["stages"]["lint"]["status"], "skipped")
        shader = SMALL.replace("    procedural:\n      kind: circle\n      center: { x: 0.5, y: 0.5 }\n      radius: 0.2\n      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }", "    shader:\n      fragment: \"fn main() {}\"")
        unsup = server.vcr_validate(manifest_yaml=shader, backend="software", **self.kw)
        self.assertEqual(unsup["first_failing_stage"], "preflight")
        codes = [c["code"] for c in unsup["stages"]["preflight"]["result"]["backend_preflight"]["checks"]]
        self.assertIn("backend.software_unsupported_layer", codes)

    def test_paths_are_confined_to_the_project(self):
        out = server.vcr_render(manifest_path="a.vcr", output="/etc/x.mov", **self.kw)
        self.assertEqual(out["error"]["code"], "usage.invalid_argument")
        out = server.vcr_validate(manifest_path="../x.vcr", **self.kw)
        self.assertEqual(out["error"]["code"], "usage.invalid_argument")

    @unittest.skipUnless(HAVE_FFMPEG, "ffmpeg/ffprobe required")
    def test_inspect_render_verify_end_to_end(self):
        (self.root / "a.vcr").write_text(SMALL)
        ins = server.vcr_inspect(manifest_path="a.vcr", samples=4, **self.kw)
        self.assertEqual(ins["status"], "ok", json.dumps(ins)[:400])
        self.assertTrue((self.root / ins["result"]["contact_sheet"]["path"]).exists())
        frames = [s["frame"] for s in ins["result"]["samples"]]
        self.assertEqual((frames[0], frames[-1]), (0, 5))

        done = server.vcr_render(manifest_path="a.vcr", output="renders/a.mov", expect_transparency="required", **self.kw)
        self.assertEqual(done["status"], "ok", json.dumps(done)[:600])
        self.assertEqual(done["render"]["adapter"]["engine_path"], ENGINE)
        self.assertEqual(done["verify"]["result"]["media"]["probe"]["packet_count"], 6)
        self.assertEqual(done["verify"]["result"]["freshness"]["fresh"], True)
        self.assertTrue((self.root / "renders/a.mov.provenance.json").exists())

        wrong = server.vcr_verify(output_path="renders/a.mov", expect_frames=7, **self.kw)
        self.assertEqual(wrong["status"], "failed")
        self.assertIn("verify.frame_count_mismatch", [d["code"] for d in wrong["diagnostics"]])

    @unittest.skipUnless(HAVE_FFMPEG, "ffmpeg/ffprobe required")
    def test_render_timeout_publishes_nothing(self):
        big = SMALL.replace("{ width: 64, height: 36 }", "{ width: 1920, height: 1080 }").replace("{ frames: 6 }", "{ frames: 3000 }")
        (self.root / "big.vcr").write_text(big)
        doc = server.vcr_render(manifest_path="big.vcr", output="renders/big.mov", backend="software", timeout_seconds=1.0, **self.kw)
        self.assertEqual(doc["status"], "error")
        self.assertEqual(doc["render"]["error"]["code"], "timeout.exceeded")
        time.sleep(0.5)
        leftovers = [p.name for p in self.root.rglob("*") if p.is_file() and (".partial-" in p.name or p.name == "big.mov")]
        self.assertEqual(leftovers, [])
        # no orphaned encoder
        ps = subprocess.run(["pgrep", "-f", f"{self.root}"], capture_output=True, text=True)
        self.assertEqual(ps.stdout.strip(), "")


if __name__ == "__main__":
    unittest.main()
