#!/usr/bin/env python3
"""VCR MCP Server — Expose VCR rendering capabilities as MCP tools."""

import asyncio
import json
import logging
import os
import re
import shutil
import sqlite3
import subprocess
import tempfile
from pathlib import Path

import httpx
from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

log = logging.getLogger("vcr-mcp")

READ_ONLY = ToolAnnotations(readOnlyHint=True)

mcp = FastMCP("vcr")

VCR_HOME = Path.home() / ".vcr"
BRAIN_DB = VCR_HOME / "brain.db"
PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent
RENDERS_DIR = PROJECT_ROOT / "renders"

# ── LLM configuration (env vars) ─────────────────────────────────────────────
VCR_LLM_ENDPOINT = os.environ.get("VCR_LLM_ENDPOINT", "http://127.0.0.1:1234/v1").rstrip("/")
VCR_LLM_MODEL = os.environ.get("VCR_LLM_MODEL", "")
VCR_LLM_API_KEY = os.environ.get("VCR_LLM_API_KEY", "")

SYSTEM_PROMPT = """\
You are the VCR Engine Brain. You only output valid VCR YAML manifests.
A VCR manifest MUST follow this structure:

version: 1
environment:
  resolution: {width: 1280, height: 720}
  fps: 24
  duration: 5.0
layers:
  - id: background
    procedural:
      kind: solid_color
      color: {r: 0.0, g: 0.0, b: 0.0, a: 1.0}
  - id: sample_text
    text:
      content: "HELLO"
      font_size: 120
      font_family: "GeistPixel-Line"
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
    position: {x: 640, y: 360}
    anchor: center

Rules:
1. No conversational text.
2. Use "procedural" with "kind: solid_color" for backgrounds.
3. Colors (r, g, b, a) are 0.0 to 1.0.
4. Use ONLY font_family: "GeistPixel-Line".
5. Resolution and position are integers."""

# Load SKILL.md for the synthesizer system prompt (comprehensive manifest reference)
_SKILL_MD_PATH = PROJECT_ROOT / "SKILL.md"
_SKILL_MD = ""
if _SKILL_MD_PATH.exists():
    _SKILL_MD = _SKILL_MD_PATH.read_text()

SYNTHESIZER_SYSTEM_PROMPT = f"""\
You are a VCR manifest synthesizer. You output ONLY valid VCR YAML — no prose, no markdown
fences, no explanation. The YAML must pass `vcr check` without errors.

You will receive a render plan (JSON) describing what to produce. Generate a manifest that
exactly satisfies the plan's resolution, fps, duration, alpha, and backend requirements.

If alpha is true, do NOT add a solid_color background layer — leave the background transparent.
If alpha is false, add a solid_color background as the first layer.

STRICT RULES:
- version must be 1
- All layer ids must be unique non-empty strings
- Colors are {{r: 0.0-1.0, g: 0.0-1.0, b: 0.0-1.0, a: 0.0-1.0}}
- Fonts: GeistPixel-Line, GeistPixel-Square, GeistPixel-Grid, GeistPixel-Circle, GeistPixel-Triangle
- Procedural kinds: solid_color, gradient, circle, rounded_rect, ring, line, triangle, polygon
- Expressions use `t` (frame number float). Functions: sin, cos, abs, floor, ceil, round, fract,
  clamp, lerp, smoothstep, step, easeinout, saw, tri, random, noise1d, glitch, env
- Post-processing (levels, sobel, passthrough) requires GPU backend
- Shader layers require GPU backend
- Image paths must be relative
- No unknown fields (deny_unknown_fields is active)

{_SKILL_MD[:8000] if _SKILL_MD else ""}
Output the YAML now. Nothing else."""


def _find_vcr_binary() -> str:
    """Locate the vcr binary — prefer PATH, fall back to local debug build."""
    if shutil.which("vcr"):
        return "vcr"
    local = PROJECT_ROOT / "target" / "debug" / "vcr"
    if local.exists():
        return str(local)
    local_release = PROJECT_ROOT / "target" / "release" / "vcr"
    if local_release.exists():
        return str(local_release)
    raise FileNotFoundError(
        "vcr binary not found. Run `cargo build` in the VCR project root or add vcr to PATH."
    )


def _run(cmd: list[str], timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=timeout,
        cwd=str(PROJECT_ROOT),
    )


def _resolve_manifest_path(manifest_path: str) -> Path:
    """Resolve manifest path to absolute Path. Paths are relative to PROJECT_ROOT."""
    p = Path(manifest_path)
    if not p.is_absolute():
        p = PROJECT_ROOT / p
    p = p.resolve()
    if not p.exists():
        raise FileNotFoundError(
            f"Manifest not found: {manifest_path}\n"
            f"Resolved to: {p}\n"
            f"Paths are relative to project root: {PROJECT_ROOT}"
        )
    if not p.is_file():
        raise FileNotFoundError(f"Not a file: {p}")
    return p


def _resolve_output_path(output: str) -> Path:
    """Resolve output path relative to PROJECT_ROOT."""
    p = Path(output)
    if not p.is_absolute():
        p = PROJECT_ROOT / p
    return p.resolve()


# ── Tools ────────────────────────────────────────────────────────────────────


@mcp.tool(annotations=READ_ONLY)
def vcr_doctor() -> str:
    """Check VCR system health: binary availability, FFmpeg, GPU support."""
    try:
        vcr = _find_vcr_binary()
    except FileNotFoundError as e:
        return (
            f"FAIL: {e}\n\n"
            "Run `cargo build` in the VCR project root, or add the vcr binary to PATH."
        )

    result = _run([vcr, "doctor"], timeout=30)
    output = (result.stdout + result.stderr).strip()
    status = "HEALTHY" if result.returncode == 0 else "ISSUES DETECTED"
    return f"[{status}]\n{output}"


@mcp.tool(annotations=READ_ONLY)
def validate_vcr_manifest(manifest_yaml: str, run_lint: bool = True) -> str:
    """Validate a VCR YAML manifest: schema (vcr check) and optionally unreachable layers (vcr lint).

    Runs vcr check first for schema validation. If run_lint is True, also runs vcr lint
    to detect layers that never become visible across the timeline.

    Args:
        manifest_yaml: The full YAML content of a .vcr manifest to validate.
        run_lint: If True, also run vcr lint for unreachable-layer analysis. Default: True.
    """
    try:
        vcr = _find_vcr_binary()
    except FileNotFoundError as e:
        return f"ERROR: {e}\n\nRun `vcr doctor` or `cargo build` in the VCR project root."

    with tempfile.NamedTemporaryFile(
        suffix=".vcr", mode="w", delete=False, dir=str(PROJECT_ROOT)
    ) as f:
        f.write(manifest_yaml)
        tmp_path = f.name

    try:
        # 1. Schema validation (vcr check) — fast, required for any render
        check_result = _run([vcr, "check", tmp_path], timeout=30)
        check_out = (check_result.stdout + check_result.stderr).strip()
        if check_result.returncode != 0:
            return (
                f"SCHEMA VALIDATION FAILED (vcr check):\n{check_out}\n\n"
                "Fix schema errors (typos, unknown fields, invalid values) and retry."
            )

        # 2. Unreachable layer analysis (vcr lint) — optional, samples frames
        if run_lint:
            lint_result = _run([vcr, "lint", tmp_path], timeout=60)
            lint_out = (lint_result.stdout + lint_result.stderr).strip()
            if lint_result.returncode != 0:
                return (
                    f"SCHEMA OK (vcr check passed)\n\n"
                    f"LINT WARNINGS (vcr lint — unreachable layers):\n{lint_out}\n\n"
                    "Unreachable layers never become visible. Consider removing them or fixing timing/opacity."
                )

        return "OK: Manifest is valid. Schema (vcr check) passed." + (
            " Lint (vcr lint) passed." if run_lint else ""
        )
    finally:
        os.unlink(tmp_path)


@mcp.tool(annotations=READ_ONLY)
def lint_vcr_manifest(manifest_yaml: str) -> str:
    """Validate a VCR YAML manifest (alias for validate_vcr_manifest). Prefer validate_vcr_manifest."""
    return validate_vcr_manifest(manifest_yaml, run_lint=True)


@mcp.tool()
def vcr_render_frame(
    manifest_path: str,
    frame: int = 0,
    output: str | None = None,
    backend: str = "software",
) -> str:
    """Render a single frame from a VCR manifest to PNG. Fast preview without full video encode.

    Use this to quickly verify a manifest renders correctly before running a full build.
    Output is written to renders/ by default.

    Args:
        manifest_path: Path to the .vcr manifest (relative to project root).
        frame: Frame index to render (0-based). Default: 0.
        output: Output PNG path (relative). Default: renders/<manifest_stem>_f<frame>.png.
        backend: Render backend: "software", "gpu", "auto". Default: software.
    """
    if backend not in ("software", "gpu", "auto"):
        return f"ERROR: backend must be 'software', 'gpu', or 'auto', got '{backend}'"

    if frame < 0:
        return "ERROR: frame must be >= 0"

    try:
        vcr = _find_vcr_binary()
        manifest_abs = _resolve_manifest_path(manifest_path)
    except FileNotFoundError as e:
        return f"ERROR: {e}\n\nRun `vcr doctor` to verify the VCR binary."

    if not output:
        RENDERS_DIR.mkdir(parents=True, exist_ok=True)
        stem = manifest_abs.stem
        output = f"renders/{stem}_f{frame}.png"
    output_abs = _resolve_output_path(output)
    output_abs.parent.mkdir(parents=True, exist_ok=True)

    result = _run(
        [vcr, "render-frame", str(manifest_abs), "--frame", str(frame), "-o", str(output_abs), "--backend", backend],
        timeout=60,
    )
    out = (result.stdout + result.stderr).strip()
    if result.returncode != 0:
        return f"RENDER FRAME FAILED:\n{out}\n\nRun `vcr doctor` to verify dependencies."

    return json.dumps({
        "status": "OK",
        "output": str(output_abs),
        "manifest": manifest_path,
        "frame": frame,
        "backend": backend,
    }, indent=2)


@mcp.tool(annotations=READ_ONLY)
def vcr_list_examples() -> str:
    """List available VCR example manifests in the examples/ directory.

    Returns paths and descriptions for reference when creating or modifying manifests.
    Use these as starting points or to understand VCR capabilities.
    """
    examples_dir = PROJECT_ROOT / "examples"
    if not examples_dir.exists():
        return json.dumps({"examples": [], "note": "examples/ directory not found"}, indent=2)

    files = sorted(examples_dir.glob("*.vcr"))
    examples = []
    for f in files:
        rel = str(f.relative_to(PROJECT_ROOT))
        # Try to extract a one-line comment from the file
        desc = ""
        try:
            first_lines = f.read_text()[:500]
            for line in first_lines.splitlines():
                line = line.strip()
                if line.startswith("#") and "Render:" not in line and "Preview:" not in line:
                    desc = line.lstrip("#").strip()
                    break
        except Exception:
            pass
        examples.append({"path": rel, "name": f.stem, "description": desc or "(no description)"})

    return json.dumps({
        "examples": examples,
        "count": len(examples),
        "usage": "Use vcr_render_frame or vcr_execute_plan with these paths, e.g. examples/demo_scene.vcr",
    }, indent=2)


def _extract_yaml(content: str) -> str:
    """Extract YAML manifest from LLM response text."""
    # Prefer the version: marker
    if "version:" in content:
        idx = content.index("version:")
        yaml_text = content[idx:]
        if "```" in yaml_text:
            yaml_text = yaml_text.split("```")[0]
        return yaml_text.strip()
    # Fallback: code-block extraction
    m = re.search(r"```(?:yaml)?\n?(.*?)```", content, re.DOTALL)
    if m:
        return m.group(1).strip()
    return content.strip()


async def _resolve_model(client: httpx.AsyncClient) -> str:
    """Return the model ID to use, auto-detecting from /models if needed."""
    if VCR_LLM_MODEL:
        return VCR_LLM_MODEL
    try:
        resp = await client.get(f"{VCR_LLM_ENDPOINT}/models", timeout=10)
        resp.raise_for_status()
        data = resp.json().get("data", [])
        if data:
            return data[0]["id"]
    except Exception as exc:
        log.debug("Model auto-detect failed: %s", exc)
    return "local-model"


@mcp.tool()
async def render_video_from_prompt(
    prompt: str, context_ids: list[str] | None = None
) -> str:
    """Generate a broadcast-quality .mov video from a natural language prompt.

    Uses VCR's agentic pipeline: queries the Intelligence Tree for creative
    context, generates a manifest via LLM, then renders with the GPU engine.

    Configure the LLM provider via environment variables:
      VCR_LLM_ENDPOINT  — Base URL (OpenAI-compatible, default http://127.0.0.1:1234/v1)
      VCR_LLM_MODEL     — Model ID (auto-detected from /models if empty)
      VCR_LLM_API_KEY   — Bearer token (omit for local models)

    Args:
        prompt: Natural language description of the video to create.
        context_ids: Optional list of Intelligence Tree node IDs for additional creative context.
    """
    status_log: list[str] = []

    # 1. Gather context from brain.db
    context_str = ""
    if BRAIN_DB.exists():
        try:
            conn = sqlite3.connect(str(BRAIN_DB))
            if context_ids:
                placeholders = ",".join("?" for _ in context_ids)
                rows = conn.execute(
                    f"SELECT content FROM context_nodes WHERE id IN ({placeholders})",
                    context_ids,
                ).fetchall()
            else:
                rows = conn.execute(
                    "SELECT content FROM context_nodes LIMIT 20"
                ).fetchall()
            conn.close()
            context_str = "\n".join(r[0] for r in rows)
        except Exception as e:
            context_str = f"(brain.db read failed: {e})"

    status_log.append("Reading Intelligence Tree...")

    # 2. Query LLM via OpenAI-compatible API
    headers: dict[str, str] = {"Content-Type": "application/json"}
    if VCR_LLM_API_KEY:
        headers["Authorization"] = f"Bearer {VCR_LLM_API_KEY}"

    async with httpx.AsyncClient() as client:
        # Resolve model
        status_log.append("Syncing with LLM provider...")
        try:
            model = await _resolve_model(client)
        except Exception:
            model = "local-model"

        user_message = (
            f"Creative Context from Intelligence Tree:\n{context_str}\n\n"
            f"User Request: {prompt}\n\nGenerate the YAML manifest now:"
        )

        payload = {
            "model": model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": user_message},
            ],
            "temperature": 0.0,
        }

        status_log.append(f"Thinking... (model: {model})")
        try:
            resp = await client.post(
                f"{VCR_LLM_ENDPOINT}/chat/completions",
                json=payload,
                headers=headers,
                timeout=90,
            )
            resp.raise_for_status()
        except httpx.ConnectError:
            return (
                f"ERROR: Could not connect to LLM at {VCR_LLM_ENDPOINT}.\n\n"
                "Ensure your LLM provider is running (e.g. LM Studio on 127.0.0.1:1234). "
                "Set VCR_LLM_ENDPOINT, VCR_LLM_MODEL, and optionally VCR_LLM_API_KEY."
            )
        except httpx.HTTPStatusError as exc:
            return f"ERROR: LLM returned HTTP {exc.response.status_code}: {exc.response.text[:500]}"
        except httpx.TimeoutException:
            return "ERROR: LLM request timed out after 90 seconds."

    ai_resp = resp.json()
    choices = ai_resp.get("choices", [])
    if not choices:
        return "ERROR: LLM returned empty response (no choices)."

    content = choices[0].get("message", {}).get("content", "")
    yaml_content = _extract_yaml(content)

    if not yaml_content:
        return "ERROR: Could not extract YAML manifest from LLM response."

    # 3. Write manifest, lint, build
    try:
        vcr = _find_vcr_binary()
    except FileNotFoundError as e:
        return f"ERROR: {e}"

    RENDERS_DIR.mkdir(parents=True, exist_ok=True)

    manifest_path = str(PROJECT_ROOT / "agent_manifest.yaml")
    with open(manifest_path, "w") as f:
        f.write(yaml_content)

    # Schema validation (vcr check) — required before build
    check_result = _run([vcr, "check", manifest_path], timeout=30)
    if check_result.returncode != 0:
        check_out = (check_result.stdout + check_result.stderr).strip()
        return (
            f"SCHEMA VALIDATION FAILED (manifest rejected):\n{check_out}\n\n"
            f"Generated YAML:\n{yaml_content}\n\n"
            "Fix schema errors and retry, or use validate_vcr_manifest to debug."
        )

    status_log.append("Manifest validated. Starting GPU render...")

    # Build
    output_path = str(RENDERS_DIR / "agentic_result.mov")
    proc = await asyncio.create_subprocess_exec(
        vcr, "build", manifest_path, "-o", output_path,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        cwd=str(PROJECT_ROOT),
    )

    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=180)
    except asyncio.TimeoutError:
        proc.kill()
        return "ERROR: Render timed out after 180 seconds."

    # Parse build progress from stderr
    for line in (stderr or b"").decode().splitlines():
        line = line.strip()
        if "rendered frame" in line:
            status_log.append(line)

    if proc.returncode != 0:
        build_err = (stdout or b"").decode() + (stderr or b"").decode()
        return f"RENDER FAILED (exit {proc.returncode}):\n{build_err.strip()}"

    abs_path = str(Path(output_path).resolve())
    return f"RENDER COMPLETE\nOutput: {abs_path}\n\nLog:\n" + "\n".join(status_log)


@mcp.tool(annotations=READ_ONLY)
def vcr_render_plan(
    prompt: str,
    resolution: str | None = None,
    fps: int | None = None,
    duration: float | None = None,
    alpha: bool | None = None,
    backend: str | None = None,
    manifest_path: str | None = None,
) -> str:
    """Plan a VCR render from a natural language video description.

    Returns a structured render plan with CLI commands — does NOT execute them.
    The calling agent decides whether to run the commands.

    This tool enforces the VCR capability contract:
    - 2D only (no 3D). Procedural shapes, text, ASCII, custom WGSL shaders.
    - ProRes 4444 (alpha) or 422HQ output.
    - Fonts: GeistPixel-Line, Square, Grid, Circle, Triangle only.
    - Post-processing (levels, sobel, passthrough) requires GPU backend.
    - Deterministic output on software backend.
    - No audio, no network, no video editing.

    If a manifest_path is provided, validates it with `vcr check` and includes
    the result. Otherwise, the plan specifies what manifest is needed.

    Args:
        prompt: Natural language description of the video to create.
        resolution: Override resolution (e.g. "1920x1080"). Default: 1920x1080.
        fps: Override frames per second. Default: 24.
        duration: Override duration in seconds. Default: 5.0.
        alpha: Whether to produce alpha channel. Default: false unless implied.
        backend: Force backend: "software", "gpu", or "auto". Default: software.
        manifest_path: Optional path to existing .vcr manifest to validate and use.
    """
    # Apply defaults
    res = resolution or "1920x1080"
    fps_val = fps or 24
    dur = duration or 5.0
    alpha_val = alpha if alpha is not None else False
    backend_val = backend or "software"

    if backend_val not in ("software", "gpu", "auto"):
        return f"ERROR: backend must be 'software', 'gpu', or 'auto', got '{backend_val}'"

    # Determine ProRes profile
    prores = "4444" if alpha_val else "422hq"

    # Determine output filename from prompt
    slug = re.sub(r"[^a-z0-9]+", "_", prompt.lower().strip())[:40].strip("_")
    output_path = f"renders/{slug}.mov"

    # Build plan
    plan = {
        "intent_summary": prompt,
        "render_plan": {
            "resolution": res,
            "fps": fps_val,
            "duration": dur,
            "backend": backend_val,
            "alpha": alpha_val,
            "prores_profile": prores,
            "determinism_mode": "on" if backend_val == "software" else "off",
        },
        "cli_commands": [],
        "expected_outputs": [output_path],
        "validation_steps": [
            f"test -f {output_path}",
            f"ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,pix_fmt {output_path}",
        ],
    }

    # If manifest provided, validate it
    if manifest_path:
        try:
            vcr = _find_vcr_binary()
            manifest_abs = _resolve_manifest_path(manifest_path)
        except FileNotFoundError as e:
            return f"ERROR: {e}"

        check_result = _run([vcr, "check", str(manifest_abs)], timeout=30)
        check_output = (check_result.stdout + check_result.stderr).strip()

        if check_result.returncode != 0:
            plan["manifest_validation"] = f"FAILED: {check_output}"
            return json.dumps(plan, indent=2)

        plan["manifest_validation"] = "PASSED"
        plan["cli_commands"] = [
            f"vcr check {manifest_path}",
            f"vcr build {manifest_path} -o {output_path} --backend {backend_val}",
        ]
    else:
        plan["required_assets"] = f"A .vcr manifest matching this request. Write it, then validate with: vcr check <file>"
        plan["cli_commands"] = [
            "vcr check <MANIFEST_PATH>",
            f"vcr build <MANIFEST_PATH> -o {output_path} --backend {backend_val}",
        ]

    if alpha_val:
        plan["validation_steps"].append(
            "Expect pix_fmt=yuva444p10le (alpha present)"
        )

    return json.dumps(plan, indent=2)


@mcp.tool()
async def vcr_synthesize_manifest(
    prompt: str,
    resolution: str = "1920x1080",
    fps: int = 24,
    duration: float = 5.0,
    alpha: bool = False,
    backend: str = "software",
    output_manifest: str | None = None,
) -> str:
    """Generate a valid VCR YAML manifest from a natural language description.

    Writes the manifest to disk and validates it with `vcr check`. Returns the
    manifest YAML and validation result. Does NOT render — use vcr_execute_plan
    or vcr build separately.

    Args:
        prompt: Natural language description of the video to create.
        resolution: Resolution as "WIDTHxHEIGHT". Default: 1920x1080.
        fps: Frames per second. Default: 24.
        duration: Duration in seconds. Default: 5.0.
        alpha: Produce transparent background. Default: false.
        backend: Target backend: "software", "gpu", "auto". Default: software.
        output_manifest: Where to write the .vcr file. Default: auto-generated.
    """
    if backend not in ("software", "gpu", "auto"):
        return f"ERROR: backend must be 'software', 'gpu', or 'auto', got '{backend}'"

    # Parse resolution
    m = re.match(r"(\d+)x(\d+)", resolution)
    if not m:
        return f"ERROR: resolution must be WIDTHxHEIGHT, got '{resolution}'"
    width, height = int(m.group(1)), int(m.group(2))

    # Build the render plan context for the LLM
    render_plan = json.dumps({
        "prompt": prompt,
        "resolution": {"width": width, "height": height},
        "fps": fps,
        "duration": duration,
        "alpha": alpha,
        "backend": backend,
        "prores_profile": "4444" if alpha else "422hq",
    })

    # Call LLM to generate manifest
    headers: dict[str, str] = {"Content-Type": "application/json"}
    if VCR_LLM_API_KEY:
        headers["Authorization"] = f"Bearer {VCR_LLM_API_KEY}"

    async with httpx.AsyncClient() as client:
        try:
            model = await _resolve_model(client)
        except Exception:
            model = "local-model"

        payload = {
            "model": model,
            "messages": [
                {"role": "system", "content": SYNTHESIZER_SYSTEM_PROMPT},
                {"role": "user", "content": render_plan},
            ],
            "temperature": 0.0,
        }

        try:
            resp = await client.post(
                f"{VCR_LLM_ENDPOINT}/chat/completions",
                json=payload,
                headers=headers,
                timeout=90,
            )
            resp.raise_for_status()
        except httpx.ConnectError:
            return (
                f"ERROR: Could not connect to LLM at {VCR_LLM_ENDPOINT}.\n\n"
                "Ensure your LLM provider is running. Set VCR_LLM_ENDPOINT, VCR_LLM_MODEL, "
                "and optionally VCR_LLM_API_KEY."
            )
        except httpx.HTTPStatusError as exc:
            return f"ERROR: LLM HTTP {exc.response.status_code}: {exc.response.text[:500]}"
        except httpx.TimeoutException:
            return "ERROR: LLM request timed out."

    choices = resp.json().get("choices", [])
    if not choices:
        return "ERROR: LLM returned empty response."

    content = choices[0].get("message", {}).get("content", "")
    yaml_content = _extract_yaml(content)
    if not yaml_content:
        return "ERROR: Could not extract YAML from LLM response."

    # Write manifest
    slug = re.sub(r"[^a-z0-9]+", "_", prompt.lower().strip())[:40].strip("_")
    manifest_file = output_manifest or f"{slug}.vcr"
    manifest_abs = str(PROJECT_ROOT / manifest_file)

    with open(manifest_abs, "w") as f:
        f.write(yaml_content)

    # Validate
    try:
        vcr = _find_vcr_binary()
    except FileNotFoundError as e:
        return f"ERROR: {e}\n\nManifest written to: {manifest_file}\n\n{yaml_content}"

    check = _run([vcr, "check", manifest_abs], timeout=30)
    check_output = (check.stdout + check.stderr).strip()

    result = {
        "manifest_path": manifest_file,
        "validation": "PASSED" if check.returncode == 0 else f"FAILED: {check_output}",
        "yaml": yaml_content,
    }

    if check.returncode != 0:
        result["hint"] = "Fix the errors above and re-run vcr check, or call this tool again with a refined prompt."

    return json.dumps(result, indent=2)


@mcp.tool()
async def vcr_execute_plan(
    manifest_path: str,
    output: str | None = None,
    backend: str = "software",
) -> str:
    """Validate and render a VCR manifest to ProRes video.

    Runs vcr check, then vcr build. Returns the output path or error details.
    This is the final step after vcr_render_plan and vcr_synthesize_manifest.

    Args:
        manifest_path: Path to the .vcr manifest (relative to project root).
        output: Output .mov path (relative). Default: renders/<manifest_name>.mov.
        backend: Render backend: "software", "gpu", "auto". Default: software.
    """
    if backend not in ("software", "gpu", "auto"):
        return f"ERROR: backend must be 'software', 'gpu', or 'auto', got '{backend}'"

    try:
        vcr = _find_vcr_binary()
        manifest_abs = _resolve_manifest_path(manifest_path)
    except FileNotFoundError as e:
        return f"ERROR: {e}\n\nRun `vcr doctor` to verify the VCR binary and dependencies."

    # Validate first
    check = _run([vcr, "check", str(manifest_abs)], timeout=30)
    if check.returncode != 0:
        check_out = (check.stdout + check.stderr).strip()
        return (
            f"VALIDATION FAILED:\n{check_out}\n\n"
            "Fix the manifest and retry. Use validate_vcr_manifest to debug schema errors."
        )

    # Determine output path (relative to project root for display)
    if not output:
        RENDERS_DIR.mkdir(parents=True, exist_ok=True)
        stem = manifest_abs.stem
        output = f"renders/{stem}.mov"
    output_abs = _resolve_output_path(output)
    output_abs.parent.mkdir(parents=True, exist_ok=True)

    # Render
    proc = await asyncio.create_subprocess_exec(
        vcr, "build", str(manifest_abs), "-o", str(output_abs), "--backend", backend,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        cwd=str(PROJECT_ROOT),
    )

    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=300)
    except asyncio.TimeoutError:
        proc.kill()
        return "ERROR: Render timed out after 300 seconds."

    if proc.returncode != 0:
        err = (stdout or b"").decode() + (stderr or b"").decode()
        return (
            f"RENDER FAILED (exit {proc.returncode}):\n{err.strip()}\n\n"
            "Run `vcr doctor` to verify FFmpeg and GPU dependencies."
        )

    return json.dumps({
        "status": "RENDER COMPLETE",
        "output": str(output_abs),
        "manifest": manifest_path,
        "backend": backend,
        "commands_executed": [
            f"vcr check {manifest_path}",
            f"vcr build {manifest_path} -o {output} --backend {backend}",
        ],
    }, indent=2)


if __name__ == "__main__":
    mcp.run()
