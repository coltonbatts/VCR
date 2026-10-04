#!/usr/bin/env python3
"""VCR MCP server: typed access to the installed VCR engine, for agents.

Design rules (see docs/AGENT_CONTRACT.md):
- Every tool forwards the engine's own `--json` document (contract `vcr.agent/1`). Normalization,
  validation, preflight, rendering and verification are the engine's; this adapter adds only an
  `adapter` block (selected executable, identity, warnings, argv, elapsed time).
- No tool invents scene settings. Missing duration etc. comes back as a `blocked` document.
- The calling agent authors and revises manifests itself. The optional LLM synthesis tools at the
  bottom are conveniences that obey the same normalization/validation gates.
- Project, manifest and output context are explicit; outputs are relative to the project root.
"""

from __future__ import annotations

import hashlib
import json
import logging
import os
import re
import sqlite3
from pathlib import Path
from typing import Any

import httpx
from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

import engine as eng

log = logging.getLogger("vcr-mcp")

READ_ONLY = ToolAnnotations(readOnlyHint=True)
WRITES = ToolAnnotations(readOnlyHint=False, destructiveHint=False)

mcp = FastMCP("vcr")

VCR_HOME = Path.home() / ".vcr"
BRAIN_DB = VCR_HOME / "brain.db"
VCR_LLM_ENDPOINT = os.environ.get("VCR_LLM_ENDPOINT", "http://127.0.0.1:1234/v1").rstrip("/")
VCR_LLM_MODEL = os.environ.get("VCR_LLM_MODEL", "")
VCR_LLM_API_KEY = os.environ.get("VCR_LLM_API_KEY", "")

Doc = dict[str, Any]
BACKENDS = ("auto", "software", "gpu")


# ── shared plumbing ──────────────────────────────────────────────────────────


def _root(project_root: str | None) -> Path:
    return Path(project_root).resolve() if project_root else eng.DEFAULT_PROJECT_ROOT


def _engine(project_root: Path, engine_path: str | None, operation: str) -> tuple[eng.Engine | None, Doc | None]:
    try:
        return eng.select_engine(project_root, engine_path), None
    except eng.EngineUnavailable as exc:
        exc.envelope["operation"] = operation
        return None, exc.envelope


def _bad(operation: str, code: str, message: str, **kw: Any) -> Doc:
    return eng.error_envelope(operation, code, kw.pop("category", "usage"), message, exit_code=kw.pop("exit_code", 2), **kw)


def _backend_args(backend: str) -> list[str] | None:
    return ["--backend", backend] if backend in BACKENDS else None


def _call(
    operation: str,
    args: list[str],
    *,
    project_root: str | None,
    engine_path: str | None,
    backend: str | None = None,
    timeout: float = 120.0,
) -> Doc:
    root = _root(project_root)
    engine, failure = _engine(root, engine_path, operation)
    if failure:
        return failure
    assert engine is not None
    global_args: list[str] = []
    if backend is not None:
        if backend not in BACKENDS:
            return _bad(operation, "usage.invalid_argument", f"backend must be one of {BACKENDS}, got '{backend}'", adapter=engine.adapter_block())
        global_args = ["--backend", backend]
    return eng.run_engine(engine, operation, [*global_args, *args], cwd=root, timeout=timeout)


def _manifest_arg(
    operation: str,
    project_root: Path,
    manifest_path: str | None,
    manifest_yaml: str | None,
    save_as: str | None,
) -> tuple[str | None, Doc | None]:
    """Resolve to a project-relative manifest path. Inline YAML is saved to a real, stable path (not
    a temp file) because asset paths resolve relative to the manifest's directory."""
    try:
        if manifest_yaml is not None:
            if manifest_path:
                return None, _bad(operation, "usage.invalid_argument", "pass manifest_path or manifest_yaml, not both")
            rel = save_as or f".vcr_mcp/{hashlib.sha256(manifest_yaml.encode()).hexdigest()[:12]}.vcr"
            target = eng.resolve_in_project(project_root, rel, kind="save_as")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(manifest_yaml)
            return str(target.relative_to(project_root)), None
        if not manifest_path:
            return None, _bad(operation, "usage.invalid_argument", "manifest_path or manifest_yaml is required")
        target = eng.resolve_in_project(project_root, manifest_path, must_exist=True, kind="manifest_path")
        return str(target.relative_to(project_root)), None
    except eng.PathRefused as exc:
        return None, _bad(operation, "usage.invalid_argument", str(exc), recovery=["Use a path inside project_root."])


def _output_arg(operation: str, output: str | None, default_name: str) -> tuple[str | None, Doc | None]:
    rel = output or f"renders/{default_name}"
    if os.path.isabs(rel) or ".." in Path(rel).parts:
        return None, _bad(operation, "usage.invalid_argument", f"output '{rel}' must be relative to project_root and must not contain '..'")
    return rel, None


# ── discovery ────────────────────────────────────────────────────────────────


@mcp.tool(annotations=READ_ONLY)
def vcr_capabilities(
    include_schema: bool = False,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Discover what the installed engine can do and what is usable on this machine.

    Returns engine/build identity, contract and manifest versions, supported layer types and
    backend requirements, codecs/profiles, fonts, expression functions, time units (expressions and
    keyframes use FRAMES; layer start_time/end_time use SECONDS), and runtime probes (ffmpeg, GPU).
    Pass include_schema=true for the manifest JSON Schema generated from the engine's own types.
    Call this first; do not rely on documentation for fields, fonts, codecs or hardware.
    """
    args = ["capabilities"] + (["--schema"] if include_schema else [])
    return _call("capabilities", args, project_root=project_root, engine_path=engine_path, timeout=60)


@mcp.tool(annotations=READ_ONLY)
def vcr_doctor(project_root: str | None = None, engine_path: str | None = None) -> Doc:
    """Check runtime dependencies (ffmpeg, ffprobe, fonts, GPU). status=failed lists blockers."""
    return _call("doctor", ["doctor"], project_root=project_root, engine_path=engine_path, timeout=60)


@mcp.tool(annotations=READ_ONLY)
def vcr_list_examples(project_root: str | None = None) -> Doc:
    """List example manifests (examples/**/*.vcr) with their first comment line."""
    root = _root(project_root)
    out = []
    for f in sorted((root / "examples").rglob("*.vcr")):
        desc = ""
        try:
            for line in f.read_text()[:600].splitlines():
                t = line.strip()
                if t.startswith("#") and not t.startswith("# ===") and "Render:" not in t:
                    desc = t.lstrip("#").strip()
                    if desc:
                        break
        except OSError:
            pass
        out.append({"path": str(f.relative_to(root)), "description": desc})
    return {"contract": "vcr.agent/1", "operation": "list_examples", "ok": True, "status": "ok", "source": "adapter", "result": {"examples": out, "count": len(out)}}


# ── normalize ────────────────────────────────────────────────────────────────


@mcp.tool(annotations=READ_ONLY)
def vcr_normalize_brief(
    brief_text: str | None = None,
    brief_path: str | None = None,
    output_path: str | None = None,
    strict: bool = False,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Run the engine's prompt gate on a brief (natural language or YAML). Required before authoring.

    Returns normalized_spec, defaults_applied (specification defaults only), unknowns/blockers,
    and creative_inputs (text/palette/typeface/assets the brief does NOT specify; never invented).
    status=="blocked" means unresolved requirements: resolve them with the requester before
    authoring a manifest. Equivalent to `vcr prompt --json`.
    """
    args = ["prompt"]
    if (brief_text is None) == (brief_path is None):
        return _bad("prompt", "usage.invalid_argument", "provide exactly one of brief_text or brief_path")
    if brief_text is not None:
        args += ["--text", brief_text]
    else:
        args += ["--in", brief_path or ""]
    if output_path:
        out, bad = _output_arg("prompt", output_path, "")
        if bad:
            return bad
        args += ["-o", out or ""]
    if strict:
        args.append("--strict")
    return _call("prompt", args, project_root=project_root, engine_path=engine_path, timeout=30)


@mcp.tool(annotations=READ_ONLY)
def vcr_render_plan(
    brief_text: str,
    manifest_path: str | None = None,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Plan a render from a brief WITHOUT executing anything.

    Normalizes the brief with the engine (no adapter-side defaults). If blocked, returns the
    blockers; otherwise returns the normalized spec and the exact ordered CLI/MCP steps for the
    rest of the workflow. Put resolution/fps/duration/alpha in the brief; they are not overridable
    here because the engine owns those defaults.
    """
    doc = vcr_normalize_brief(brief_text=brief_text, project_root=project_root, engine_path=engine_path)
    if doc.get("status") != "ok":
        return doc
    spec = doc["result"]["normalized_spec"]
    manifest = manifest_path or "<MANIFEST.vcr>"
    out = spec["output"]["path"].lstrip("./")
    backend = "software"
    doc["result"]["plan"] = {
        "output": spec["output"]["path"],
        "steps": [
            "author the manifest from normalized_spec (do not choose unspecified text, brand or assets)",
            f"vcr_validate manifest_path={manifest}  (check -> lint -> preflight)",
            f"vcr_inspect manifest_path={manifest}  (sampled frames, contact sheet, bounds/timing diagnostics)",
            f"vcr_render manifest_path={manifest} output={out} backend={backend}",
            f"vcr_verify output_path={out} manifest_path={manifest}  (set expect_transparency=required for alpha deliverables)",
        ],
        "alpha": spec["output"]["alpha"],
        "note": "backend 'software' is the reproducible default; use gpu only for GPU-only layers (shader/video/lottie) or post effects",
    }
    return doc


# ── validate / preflight ─────────────────────────────────────────────────────


@mcp.tool(annotations=READ_ONLY)
def vcr_validate(
    manifest_path: str | None = None,
    manifest_yaml: str | None = None,
    save_as: str | None = None,
    overrides: dict[str, Any] | None = None,
    backend: str = "auto",
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Validate a manifest in three separate stages and report the first failing one.

    1. check: structural/semantic validity.  2. lint: likely visibility/timing problems.
    3. preflight (explain): can the requested backend render this scene on THIS machine
    (unsupported layers, ignored post effects, missing ffmpeg/fonts, font fallback)?
    Stages after a failing `check` are skipped. Provide manifest_path, or manifest_yaml (saved to
    save_as, default .vcr_mcp/<hash>.vcr; asset paths resolve relative to that file).
    """
    root = _root(project_root)
    rel, bad = _manifest_arg("validate", root, manifest_path, manifest_yaml, save_as)
    if bad:
        return bad
    assert rel is not None
    sets = eng.set_args(overrides)
    common = dict(project_root=project_root, engine_path=engine_path)
    stages: dict[str, Any] = {}
    check = _call("check", ["check", rel, *sets], timeout=60, **common)
    stages["check"] = check
    first_fail = None if check.get("status") == "ok" else "check"
    if first_fail is None:
        lint = _call("lint", ["lint", rel, *sets], timeout=120, **common)
        stages["lint"] = lint
        if lint.get("status") != "ok":
            first_fail = "lint"
        pre = _call("explain", ["explain", rel, *sets], backend=backend, timeout=60, **common)
        stages["preflight"] = pre
        ready = (pre.get("result") or {}).get("backend_preflight", {}).get("ready")
        if pre.get("status") != "ok" or ready is False:
            first_fail = first_fail or "preflight"
    else:
        stages["lint"] = {"status": "skipped", "reason": "check failed"}
        stages["preflight"] = {"status": "skipped", "reason": "check failed"}
    ok = first_fail is None
    return {
        "contract": "vcr.agent/1",
        "operation": "validate",
        "ok": ok,
        "status": "ok" if ok else "failed",
        "source": "adapter",
        "manifest": rel,
        "first_failing_stage": first_fail,
        "ready_to_render": ok,
        "stages": stages,
        "note": "lint findings (e.g. an unreachable layer) block ready_to_render; fix or consciously accept them. preflight.ready=false means the backend cannot honor the scene.",
    }


@mcp.tool(annotations=READ_ONLY)
def validate_vcr_manifest(manifest_yaml: str, run_lint: bool = True, save_as: str | None = None, project_root: str | None = None, engine_path: str | None = None) -> Doc:
    """Back-compat alias of vcr_validate for inline YAML (run_lint is ignored; all stages run)."""
    return vcr_validate(manifest_yaml=manifest_yaml, save_as=save_as, project_root=project_root, engine_path=engine_path)


@mcp.tool(annotations=READ_ONLY)
def lint_vcr_manifest(manifest_yaml: str, save_as: str | None = None, project_root: str | None = None, engine_path: str | None = None) -> Doc:
    """Back-compat alias of vcr_validate."""
    return vcr_validate(manifest_yaml=manifest_yaml, save_as=save_as, project_root=project_root, engine_path=engine_path)


# ── inspect / revise ─────────────────────────────────────────────────────────


@mcp.tool(annotations=WRITES)
def vcr_inspect(
    manifest_path: str | None = None,
    manifest_yaml: str | None = None,
    save_as: str | None = None,
    overrides: dict[str, Any] | None = None,
    samples: int = 12,
    output_dir: str | None = None,
    safe_margin: float = 0.05,
    backend: str = "software",
    timeout_seconds: float = 300,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Sample the timeline and return evidence for visual review plus mechanical diagnostics.

    Writes sample PNGs, contact_sheet.png and inspection.json under output_dir (default
    renders/<stem>_inspect). Result: exact frame indices/times, motion phases (entrance/hold/exit/
    ending), evaluated layer state per sample, exact per-layer pixel bounds, and diagnostics
    (clipping at the canvas edge, safe-area, cut-off animation, empty frames). Diagnostics are
    labelled exact/approximate. They do NOT judge design quality: look at the contact sheet.
    Revise by editing the layer with the reported stable `id`, or by changing declared params via
    `overrides`, then inspect again.
    """
    root = _root(project_root)
    rel, bad = _manifest_arg("inspect", root, manifest_path, manifest_yaml, save_as)
    if bad:
        return bad
    assert rel is not None
    args = ["inspect", rel, "--samples", str(samples), "--safe-margin", str(safe_margin), *eng.set_args(overrides)]
    if output_dir:
        out, bad = _output_arg("inspect", output_dir, "")
        if bad:
            return bad
        args += ["-o", out or ""]
    return _call("inspect", args, project_root=project_root, engine_path=engine_path, backend=backend, timeout=timeout_seconds)


@mcp.tool(annotations=WRITES)
def vcr_render_frame(
    manifest_path: str,
    frame: int = 0,
    output: str | None = None,
    overrides: dict[str, Any] | None = None,
    backend: str = "software",
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Render one full-resolution frame to PNG (exact frame index; metadata sidecar included)."""
    if frame < 0:
        return _bad("render-frame", "usage.invalid_argument", "frame must be >= 0")
    root = _root(project_root)
    rel, bad = _manifest_arg("render-frame", root, manifest_path, None, None)
    if bad:
        return bad
    assert rel is not None
    out, bad = _output_arg("render-frame", output, f"{Path(rel).stem}_f{frame}.png")
    if bad:
        return bad
    args = ["render-frame", rel, "--frame", str(frame), "-o", out or "", *eng.set_args(overrides)]
    return _call("render-frame", args, project_root=project_root, engine_path=engine_path, backend=backend, timeout=120)


# ── render / verify ──────────────────────────────────────────────────────────


@mcp.tool(annotations=WRITES)
def vcr_render(
    manifest_path: str,
    output: str | None = None,
    overrides: dict[str, Any] | None = None,
    backend: str = "software",
    verify: bool = True,
    expect_transparency: str | None = None,
    timeout_seconds: float = 900,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Golden-path render (ProRes 4444, alpha-preserving) then, by default, verify the encoded file.

    The engine publishes atomically (hidden partial file, conformance check, rename) and writes
    <output>.metadata.json (scene record) and <output>.provenance.json (execution record).
    A timeout terminates the whole process tree and publishes nothing. Returns both documents;
    a successful render is not "done" until `verify.status == "ok"`.
    For alpha deliverables pass expect_transparency="required" so decoded pixels are checked.
    """
    root = _root(project_root)
    rel, bad = _manifest_arg("render", root, manifest_path, None, None)
    if bad:
        return bad
    assert rel is not None
    out, bad = _output_arg("render", output, f"{Path(rel).stem}.mov")
    if bad:
        return bad
    render = _call("render", ["render", rel, "-o", out or "", *eng.set_args(overrides)], project_root=project_root, engine_path=engine_path, backend=backend, timeout=timeout_seconds)
    result: Doc = {"contract": "vcr.agent/1", "operation": "render+verify", "source": "adapter", "render": render, "verify": None}
    if render.get("status") != "ok" or not verify:
        result.update(ok=False if render.get("status") != "ok" else True, status=render.get("status"))
        return result
    result["verify"] = vcr_verify(
        output_path=out or "", manifest_path=rel, overrides=overrides, expect_transparency=expect_transparency,
        project_root=project_root, engine_path=engine_path,
    )
    ok = result["verify"].get("status") == "ok"
    result.update(ok=ok, status="ok" if ok else "failed")
    return result


@mcp.tool(annotations=READ_ONLY)
def vcr_verify(
    output_path: str,
    manifest_path: str | None = None,
    overrides: dict[str, Any] | None = None,
    expect_width: int | None = None,
    expect_height: int | None = None,
    expect_fps: int | None = None,
    expect_frames: int | None = None,
    expect_container: str | None = None,
    expect_codec: str | None = None,
    expect_profile: str | None = None,
    expect_alpha_capable: bool | None = None,
    expect_transparency: str | None = None,
    decode_frames: str | None = None,
    require_provenance: bool = False,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Verify the encoded media against explicit expectations (ffprobe + decoded pixels).

    Checks resolution, exact frame rate rational, packet-count frame count, duration, container,
    codec/profile, alpha capability, and (expect_transparency=required|none|any) transparency
    measured on decoded RGBA, never inferred from pix_fmt. With manifest_path it also derives
    expectations from the scene and detects stale output; with provenance it detects modified or
    incomplete files. `result.producer` is the engine that made the file; top-level `engine` is
    only the verifier. Reports what it checked and its limits.
    """
    root = _root(project_root)
    try:
        eng.resolve_in_project(root, output_path, must_exist=True, kind="output_path")
        if manifest_path:
            eng.resolve_in_project(root, manifest_path, must_exist=True, kind="manifest_path")
    except eng.PathRefused as exc:
        return _bad("verify", "usage.invalid_argument", str(exc))
    args = ["verify", output_path]
    if manifest_path:
        args += ["--manifest", manifest_path, *eng.set_args(overrides)]
    for flag, value in (
        ("--expect-width", expect_width), ("--expect-height", expect_height), ("--expect-fps", expect_fps),
        ("--expect-frames", expect_frames), ("--expect-container", expect_container), ("--expect-codec", expect_codec),
        ("--expect-profile", expect_profile), ("--expect-transparency", expect_transparency), ("--decode-frames", decode_frames),
    ):
        if value is not None:
            args += [flag, str(value)]
    if expect_alpha_capable is not None:
        args += ["--expect-alpha-capable", "true" if expect_alpha_capable else "false"]
    if require_provenance:
        args.append("--require-provenance")
    return _call("verify", args, project_root=project_root, engine_path=engine_path, timeout=300)


@mcp.tool(annotations=WRITES)
def vcr_execute_plan(
    manifest_path: str,
    output: str | None = None,
    backend: str = "software",
    overrides: dict[str, Any] | None = None,
    expect_transparency: str | None = None,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """Back-compat: validate (check/lint/preflight) then render and verify. Stops at the first failing stage."""
    validation = vcr_validate(manifest_path=manifest_path, overrides=overrides, backend=backend, project_root=project_root, engine_path=engine_path)
    if not validation["ready_to_render"]:
        return {"contract": "vcr.agent/1", "operation": "execute_plan", "ok": False, "status": "failed", "source": "adapter", "stopped_at": validation["first_failing_stage"], "validation": validation}
    done = vcr_render(manifest_path, output, overrides, backend, True, expect_transparency, project_root=project_root, engine_path=engine_path)
    done["validation"] = validation
    done["operation"] = "execute_plan"
    return done


# ── optional LLM synthesis (obeys the same gates) ────────────────────────────

SYNTH_PROMPT = """You write VCR YAML manifests. Output ONLY the YAML (no prose, no fences).
Follow the normalized spec exactly (resolution, fps, duration, alpha). Do not invent text, brand
colors or assets that the brief does not specify: use obvious placeholders and say so in a comment.
Procedural geometry is normalized 0..1. Expressions use `t` = FRAME index; start_time/end_time are seconds.
Use only fonts, layers and functions from the capabilities document provided."""


def _extract_yaml(content: str) -> str:
    if "version:" in content:
        text = content[content.index("version:"):]
        return text.split("```")[0].strip()
    m = re.search(r"```(?:yaml)?\n?(.*?)```", content, re.DOTALL)
    return (m.group(1) if m else content).strip()


async def _llm_manifest(spec: Doc, capabilities: Doc, brief: str) -> str | Doc:
    headers = {"Content-Type": "application/json"}
    if VCR_LLM_API_KEY:
        headers["Authorization"] = f"Bearer {VCR_LLM_API_KEY}"
    user = json.dumps({"brief": brief, "normalized_spec": spec, "capabilities": {k: capabilities.get("result", {}).get(k) for k in ("layers", "procedural_kinds", "time", "expression", "fonts", "encoding")}})
    async with httpx.AsyncClient() as client:
        model = VCR_LLM_MODEL
        if not model:
            try:
                r = await client.get(f"{VCR_LLM_ENDPOINT}/models", timeout=10)
                model = (r.json().get("data") or [{}])[0].get("id", "local-model")
            except Exception:
                model = "local-model"
        try:
            resp = await client.post(
                f"{VCR_LLM_ENDPOINT}/chat/completions",
                json={"model": model, "temperature": 0.0, "messages": [{"role": "system", "content": SYNTH_PROMPT}, {"role": "user", "content": user}]},
                headers=headers, timeout=120,
            )
            resp.raise_for_status()
        except httpx.HTTPError as exc:
            return _bad("synthesize", "dependency.llm_unavailable", f"LLM request failed: {exc}", category="dependency", exit_code=4, retryable=True,
                        recovery=["Set VCR_LLM_ENDPOINT / VCR_LLM_MODEL / VCR_LLM_API_KEY, or author the manifest yourself and use vcr_validate."])
    choices = resp.json().get("choices") or []
    return _extract_yaml(choices[0]["message"]["content"]) if choices else _bad("synthesize", "dependency.llm_unavailable", "LLM returned no choices", category="dependency", exit_code=4)


@mcp.tool(annotations=WRITES)
async def vcr_synthesize_manifest(
    brief_text: str,
    output_manifest: str | None = None,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """OPTIONAL: draft a manifest with a configured LLM, then run it through the engine gates.

    Calls the engine's prompt gate first and stops if blocked (no LLM call, no invented settings).
    The draft is saved, then validated with check/lint/preflight. Prefer authoring the manifest
    yourself; this exists for harnesses without a strong model.
    """
    norm = vcr_normalize_brief(brief_text=brief_text, project_root=project_root, engine_path=engine_path)
    if norm.get("status") != "ok":
        return norm
    caps = vcr_capabilities(project_root=project_root, engine_path=engine_path)
    drafted = await _llm_manifest(norm["result"]["normalized_spec"], caps, brief_text)
    if isinstance(drafted, dict):
        return drafted
    slug = re.sub(r"[^a-z0-9]+", "_", brief_text.lower())[:40].strip("_") or "draft"
    validation = vcr_validate(manifest_yaml=drafted, save_as=output_manifest or f".vcr_mcp/{slug}.vcr", project_root=project_root, engine_path=engine_path)
    return {"contract": "vcr.agent/1", "operation": "synthesize", "ok": validation["ok"], "status": validation["status"], "source": "adapter", "normalized": norm, "manifest": validation["manifest"], "yaml": drafted, "validation": validation}


@mcp.tool(annotations=WRITES)
async def render_video_from_prompt(
    prompt: str,
    context_ids: list[str] | None = None,
    project_root: str | None = None,
    engine_path: str | None = None,
) -> Doc:
    """OPTIONAL one-shot: normalize -> LLM draft -> check/lint/preflight -> render -> verify.

    Stops at the first failing stage and reports it; never renders a blocked or invalid scene.
    `context_ids` pulls extra creative notes from ~/.vcr/brain.db if present.
    """
    context = ""
    if BRAIN_DB.exists():
        try:
            conn = sqlite3.connect(str(BRAIN_DB))
            rows = (
                conn.execute(f"SELECT content FROM context_nodes WHERE id IN ({','.join('?' for _ in context_ids)})", context_ids).fetchall()
                if context_ids else conn.execute("SELECT content FROM context_nodes LIMIT 20").fetchall()
            )
            conn.close()
            context = "\n".join(r[0] for r in rows)
        except Exception as exc:  # optional context only
            log.debug("brain.db unavailable: %s", exc)
    brief = f"{prompt}\n\nContext:\n{context}" if context else prompt
    drafted = await vcr_synthesize_manifest(brief, project_root=project_root, engine_path=engine_path)
    if not drafted.get("ok"):
        return drafted
    rendered = vcr_render(drafted["manifest"], None, None, "software", True, None, project_root=project_root, engine_path=engine_path)
    rendered["synthesis"] = drafted
    return rendered


if __name__ == "__main__":
    mcp.run()
