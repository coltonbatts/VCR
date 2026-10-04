"""Engine access for the VCR MCP server.

This module is deliberately free of MCP imports so it can be unit-tested and reused. Its one job
is to run the *installed engine* (the `vcr` CLI) and return its structured `--json` documents
unchanged. It never normalizes, validates, plans, or invents scene settings: that behaviour lives in
the engine, so CLI and MCP cannot disagree.

Contract: docs/AGENT_CONTRACT.md (`vcr.agent/1`).
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

ADAPTER_NAME = "vcr-mcp"
ADAPTER_VERSION = "0.2.0"
SUPPORTED_CONTRACT_MAJOR = 1
CONTRACT_PREFIX = "vcr.agent/"

DEFAULT_PROJECT_ROOT = Path(
    os.environ.get("VCR_PROJECT_ROOT", Path(__file__).resolve().parent.parent.parent)
).resolve()


# ── engine selection ─────────────────────────────────────────────────────────


@dataclass
class Engine:
    path: str
    reason: str
    identity: dict[str, Any]
    capabilities_contract: str
    warnings: list[str] = field(default_factory=list)
    alternatives: list[dict[str, Any]] = field(default_factory=list)

    def adapter_block(self) -> dict[str, Any]:
        return {
            "name": ADAPTER_NAME,
            "version": ADAPTER_VERSION,
            "engine_path": self.path,
            "selection": self.reason,
            "engine_identity": self.identity,
            "warnings": self.warnings,
            "alternatives": self.alternatives,
        }


class EngineUnavailable(Exception):
    """No usable engine. `.envelope` is a contract-shaped error document."""

    def __init__(self, envelope: dict[str, Any]):
        super().__init__(envelope["error"]["message"])
        self.envelope = envelope


def error_envelope(
    operation: str,
    code: str,
    category: str,
    message: str,
    *,
    recovery: list[str] | None = None,
    retryable: bool = False,
    exit_code: int = 5,
    observed: Any = None,
    expected: Any = None,
    adapter: dict[str, Any] | None = None,
    details: Any = None,
) -> dict[str, Any]:
    body: dict[str, Any] = {
        "code": code,
        "category": category,
        "message": message,
        "summary": message.splitlines()[0] if message else code,
        "operation": operation,
        "recovery": recovery or [],
        "retryable": retryable,
        "exit_code": exit_code,
    }
    if observed is not None:
        body["observed"] = observed
    if expected is not None:
        body["expected"] = expected
    if details is not None:
        body["details"] = details
    doc: dict[str, Any] = {
        "contract": f"{CONTRACT_PREFIX}{SUPPORTED_CONTRACT_MAJOR}",
        "operation": operation,
        "ok": False,
        "status": "error",
        "error": body,
        "source": "adapter",
    }
    if adapter:
        doc["adapter"] = adapter
    return doc


def _candidates(project_root: Path, explicit: str | None) -> list[tuple[str, str]]:
    """Ordered (path, reason). Explicit choices win; the checkout build precedes PATH so an old
    installed binary can never silently become the engine for this checkout."""
    found: list[tuple[str, str]] = []
    if explicit:
        found.append((explicit, "explicit engine_path argument"))
    env_bin = os.environ.get("VCR_BIN")
    if env_bin:
        found.append((env_bin, "VCR_BIN environment variable"))
    built = [
        p
        for p in (
            project_root / "target" / "release" / "vcr",
            project_root / "target" / "debug" / "vcr",
        )
        if p.is_file()
    ]
    built.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    for p in built:
        found.append((str(p), "checkout build"))
    on_path = shutil.which("vcr")
    if on_path:
        found.append((on_path, "PATH"))
    # De-duplicate by real path, keeping first reason.
    seen: set[str] = set()
    unique = []
    for path, reason in found:
        real = os.path.realpath(path)
        if real not in seen:
            seen.add(real)
            unique.append((path, reason))
    return unique


_IDENTITY_CACHE: dict[tuple[str, float], dict[str, Any] | None] = {}


def _probe(path: str, timeout: float = 20.0) -> dict[str, Any]:
    """Run `capabilities --json` on a candidate. Returns {'ok':bool, 'identity':..., 'why':...}."""
    try:
        mtime = os.stat(path).st_mtime
    except OSError as exc:
        return {"ok": False, "why": f"cannot stat: {exc}"}
    key = (path, mtime)
    if key in _IDENTITY_CACHE and _IDENTITY_CACHE[key] is not None:
        return _IDENTITY_CACHE[key]  # type: ignore[return-value]
    proc = _spawn([path, "capabilities", "--json"], cwd=None, timeout=timeout)
    if proc["timed_out"]:
        return {"ok": False, "why": "capabilities probe timed out"}
    if proc["returncode"] is None:
        return {"ok": False, "why": proc.get("spawn_error", "failed to start")}
    try:
        doc = json.loads(proc["stdout"].strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {
            "ok": False,
            "why": "engine does not implement the agent contract (no JSON from `capabilities --json`); it predates vcr.agent/1",
            "stderr": proc["stderr"][-300:],
        }
    contract = str(doc.get("contract", ""))
    if not contract.startswith(CONTRACT_PREFIX):
        return {"ok": False, "why": f"unrecognized contract '{contract}'"}
    major = int(contract[len(CONTRACT_PREFIX):].split(".")[0] or 0)
    result = {
        "ok": major == SUPPORTED_CONTRACT_MAJOR,
        "identity": doc.get("engine", {}),
        "contract": contract,
        "why": None if major == SUPPORTED_CONTRACT_MAJOR else f"contract major {major} unsupported (adapter supports {SUPPORTED_CONTRACT_MAJOR})",
    }
    _IDENTITY_CACHE[key] = result
    return result


def select_engine(project_root: Path | None = None, explicit: str | None = None) -> Engine:
    """Choose, identify and compatibility-check the engine. Raises EngineUnavailable."""
    root = (project_root or DEFAULT_PROJECT_ROOT).resolve()
    candidates = _candidates(root, explicit)
    if not candidates:
        raise EngineUnavailable(
            error_envelope(
                "select_engine",
                "dependency.engine_missing",
                "dependency",
                "no vcr executable found (no explicit path, VCR_BIN, checkout build, or PATH entry)",
                recovery=[f"Run `cargo build --release` in {root}", "or pass engine_path / set VCR_BIN"],
                exit_code=4,
            )
        )
    probes = [(path, reason, _probe(path)) for path, reason in candidates]
    first_ok = next(((p, r, pr) for p, r, pr in probes if pr["ok"]), None)
    if first_ok is None:
        raise EngineUnavailable(
            error_envelope(
                "select_engine",
                "engine.incompatible",
                "dependency",
                "no candidate engine speaks the agent contract (vcr.agent/%d)" % SUPPORTED_CONTRACT_MAJOR,
                recovery=["Rebuild: `cargo build --release` in the VCR checkout, then retry.", "Or pass engine_path to a current build."],
                exit_code=4,
                observed=[{"path": p, "reason": r, "why": pr["why"]} for p, r, pr in probes],
            )
        )
    path, reason, probe = first_ok
    identity = probe["identity"]
    warnings: list[str] = []
    alternatives: list[dict[str, Any]] = []
    for p, r, pr in probes:
        if p == path:
            continue
        alt_id = pr.get("identity") or {}
        alternatives.append({"path": p, "reason": r, "usable": pr["ok"], "git_hash": alt_id.get("git_hash"), "version": alt_id.get("version"), "why_not": pr["why"]})
        if pr["ok"] and alt_id.get("git_hash") != identity.get("git_hash"):
            warnings.append(
                f"another engine on this machine ({r}: {p}) is build {alt_id.get('git_hash')} but build {identity.get('git_hash')} was selected ({reason}); results come from the selected engine only"
            )
        elif not pr["ok"]:
            warnings.append(f"ignored {r} engine {p}: {pr['why']}")
    return Engine(path=path, reason=reason, identity=identity, capabilities_contract=probe["contract"], warnings=warnings, alternatives=alternatives)


# ── process execution ────────────────────────────────────────────────────────


def _spawn(cmd: list[str], cwd: str | Path | None, timeout: float) -> dict[str, Any]:
    """Run in its own session so a timeout can kill the whole process tree (vcr -> ffmpeg)."""
    try:
        proc = subprocess.Popen(
            cmd,
            cwd=str(cwd) if cwd else None,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            start_new_session=True,
        )
    except OSError as exc:
        return {"returncode": None, "stdout": "", "stderr": "", "timed_out": False, "spawn_error": str(exc)}
    try:
        out, err = proc.communicate(timeout=timeout)
        return {"returncode": proc.returncode, "stdout": out, "stderr": err, "timed_out": False, "pid": proc.pid}
    except subprocess.TimeoutExpired:
        _kill_group(proc)
        try:
            out, err = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:  # pragma: no cover - last resort
            out, err = "", ""
        return {"returncode": None, "stdout": out or "", "stderr": err or "", "timed_out": True, "pid": proc.pid}


def _remove_partials(root: Path, pid: int) -> list[str]:
    """A killed engine cannot run its cleanup guard; remove the hidden `.<stem>.partial-<pid>.<ext>`
    file it was writing. (The engine also sweeps stale partials on the next render.)"""
    removed: list[str] = []
    needle = f".partial-{pid}."
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in {"target", ".git", "node_modules"}]
        for name in filenames:
            if name.startswith(".") and needle in name:
                try:
                    os.unlink(os.path.join(dirpath, name))
                    removed.append(os.path.join(dirpath, name))
                except OSError:
                    pass
    return removed


def _kill_group(proc: subprocess.Popen) -> None:
    try:
        pgid = os.getpgid(proc.pid)
    except ProcessLookupError:
        return
    for sig, grace in ((signal.SIGTERM, 2.0), (signal.SIGKILL, 0.0)):
        try:
            os.killpg(pgid, sig)
        except ProcessLookupError:
            return
        deadline = time.time() + grace
        while time.time() < deadline:
            if proc.poll() is not None:
                return
            time.sleep(0.05)


def run_engine(
    engine: Engine,
    operation: str,
    args: list[str],
    *,
    cwd: Path | None = None,
    timeout: float = 120.0,
) -> dict[str, Any]:
    """Run `vcr <args> --json` and return the engine's document untouched, plus an `adapter` block.

    Failures in the adapter's own domain (timeout, spawn failure, protocol violation) are returned
    as contract-shaped error documents with `source: "adapter"`. The engine's own failures are the
    engine's own error documents.
    """
    cmd = [engine.path, *args]
    if "--json" not in cmd:
        cmd.append("--json")
    started = time.time()
    run = _spawn(cmd, cwd=cwd or DEFAULT_PROJECT_ROOT, timeout=timeout)
    elapsed_ms = int((time.time() - started) * 1000)
    adapter = engine.adapter_block()
    adapter["elapsed_ms"] = elapsed_ms
    adapter["argv"] = cmd
    if run["timed_out"]:
        removed = _remove_partials(Path(cwd or DEFAULT_PROJECT_ROOT), run["pid"])
        return error_envelope(
            operation,
            "timeout.exceeded",
            "timeout",
            f"`vcr {operation}` exceeded {timeout:.0f}s and its process group was terminated",
            recovery=[
                "Re-run with a larger timeout_seconds, or reduce frames/resolution first (use inspect to check the scene).",
                "Any partially written output was not published: the engine renders to a hidden partial file and renames on success.",
            ],
            retryable=True,
            exit_code=5,
            adapter=adapter,
            observed={"timeout_seconds": timeout, "stderr_tail": run["stderr"][-400:], "removed_partial_files": removed},
        )
    if run["returncode"] is None:
        return error_envelope(
            operation, "dependency.engine_missing", "dependency",
            f"failed to start engine: {run.get('spawn_error')}", exit_code=4, adapter=adapter,
            recovery=["Check the engine path and permissions; run vcr_capabilities."],
        )
    lines = [ln for ln in run["stdout"].splitlines() if ln.strip()]
    try:
        doc = json.loads(lines[-1]) if lines else None
    except ValueError:
        doc = None
    if not isinstance(doc, dict) or not str(doc.get("contract", "")).startswith(CONTRACT_PREFIX):
        return error_envelope(
            operation, "engine.protocol_error", "internal",
            "engine did not return a contract document on stdout",
            exit_code=5, adapter=adapter,
            observed={"returncode": run["returncode"], "stdout_tail": run["stdout"][-300:], "stderr_tail": run["stderr"][-300:]},
            recovery=["Run vcr_capabilities to confirm the engine version; rebuild the engine if it is older than this adapter."],
        )
    if len(lines) != 1:
        adapter.setdefault("warnings", []).append("engine wrote more than one line to stdout; used the last JSON line")
    doc["adapter"] = adapter
    doc["process"] = {"returncode": run["returncode"], "stderr_tail": run["stderr"][-600:] if run["returncode"] else ""}
    return doc


# ── path handling (explicit project/manifest/output context) ────────────────


class PathRefused(ValueError):
    pass


def resolve_in_project(project_root: Path, rel: str, *, must_exist: bool = False, kind: str = "path") -> Path:
    """Resolve `rel` inside the project root; refuse escapes. The engine independently refuses
    absolute/`..` output paths, so outputs stay relative to the project root."""
    root = project_root.resolve()
    candidate = (root / rel).resolve() if not os.path.isabs(rel) else Path(rel).resolve()
    try:
        candidate.relative_to(root)
    except ValueError as exc:
        raise PathRefused(f"{kind} '{rel}' resolves outside the project root {root}") from exc
    if must_exist and not candidate.exists():
        raise PathRefused(f"{kind} '{rel}' does not exist under {root}")
    return candidate


def set_args(overrides: dict[str, Any] | None) -> list[str]:
    """`--set NAME=VALUE` pairs. Colors are `#RRGGBBAA` strings; bools `true`/`false`."""
    args: list[str] = []
    for name, value in (overrides or {}).items():
        if isinstance(value, bool):
            rendered = "true" if value else "false"
        else:
            rendered = str(value)
        args += ["--set", f"{name}={rendered}"]
    return args
