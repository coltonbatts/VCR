#!/usr/bin/env python3
"""Validate a VCR render plan response against the skill schema.

Usage:
    # Validate a plan from a file:
    python3 validate_plan.py plan_response.md

    # Validate from stdin (pipe LLM output):
    echo "..." | python3 validate_plan.py -

    # Validate and check cli_commands against vcr check:
    python3 validate_plan.py plan_response.md --live

Exit codes:
    0  All checks passed
    1  Schema validation failed
    2  Live validation failed (--live only)
"""

import re
import sys
import subprocess
import argparse
import tempfile
import os

REQUIRED_SECTIONS = [
    "intent_summary",
    "capability_check",
    "render_plan",
    "required_assets",
    "cli_commands",
    "expected_outputs",
    "validation_steps",
]

RENDER_PLAN_FIELDS = {
    "stage_type": {"ascii", "raster", "hybrid"},
    "backend": {"software", "gpu", "auto"},
    "alpha": {"true", "false"},
    "prores_profile": {"4444", "422hq"},
    "determinism_mode": {"on", "off"},
}

RENDER_PLAN_REQUIRED = {
    "stage_type", "resolution", "fps", "duration",
    "backend", "alpha", "prores_profile", "source_mode", "determinism_mode",
}


def parse_sections(text: str) -> dict[str, str]:
    """Extract named sections from markdown response."""
    sections = {}
    # Match headers like "### 1. intent_summary", "**intent_summary**:", or just "**intent_summary**"
    pattern = r"(?:^|\n)\s*(?:#{1,4}\s*\d+\.\s*|(?:\*\*))?(intent_summary|capability_check|render_plan|required_assets|cli_commands|expected_outputs|validation_steps)(?:\*\*)?:?\s*\n"
    matches = list(re.finditer(pattern, text, re.IGNORECASE))

    for i, m in enumerate(matches):
        name = m.group(1).lower()
        start = m.end()
        end = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        sections[name] = text[start:end].strip()

    return sections


def parse_render_plan_table(text: str) -> dict[str, str]:
    """Extract key-value pairs from a markdown table."""
    fields = {}
    for line in text.split("\n"):
        # Match: | field | `value` | or | field | value |
        m = re.match(r"\|\s*(\w[\w_]*)\s*\|\s*`?([^|`]+?)`?\s*\|", line)
        if m:
            fields[m.group(1).strip().lower()] = m.group(2).strip().lower()
    return fields


def extract_cli_commands(text: str) -> list[str]:
    """Extract commands from fenced code blocks or raw lines."""
    commands = []
    in_block = False
    for line in text.split("\n"):
        if line.strip().startswith("```"):
            in_block = not in_block
            continue
        if in_block:
            stripped = line.strip()
            if stripped and not stripped.startswith("#"):
                commands.append(stripped)
    # If no fenced block found, try raw lines starting with vcr/ffprobe/test
    if not commands:
        for line in text.split("\n"):
            stripped = line.strip()
            if stripped and re.match(r"^(vcr|ffprobe|test)\s", stripped):
                commands.append(stripped)
    return commands


def validate_schema(text: str) -> list[str]:
    """Validate response structure. Returns list of errors."""
    errors = []
    sections = parse_sections(text)

    # Check all required sections present
    for s in REQUIRED_SECTIONS:
        if s not in sections:
            errors.append(f"Missing section: {s}")

    if not sections:
        errors.append("No sections found. Response may not follow the required format.")
        return errors

    # If capability_check says unsupported, remaining sections can be null
    cap = sections.get("capability_check", "")
    is_unsupported = "unsupported" in cap.lower() and "null" in text.lower()

    if is_unsupported:
        return errors  # null sections are valid for unsupported requests

    # Validate intent_summary is a single sentence
    intent = sections.get("intent_summary", "")
    if intent and intent.count("\n") > 1:
        errors.append("intent_summary should be a single sentence")

    # Validate render_plan fields
    if "render_plan" in sections:
        plan = sections["render_plan"]
        # Could be a table or inline text
        fields = parse_render_plan_table(plan)

        if fields:  # Table format
            for field in RENDER_PLAN_REQUIRED:
                if field not in fields:
                    errors.append(f"render_plan missing field: {field}")

            for field, valid in RENDER_PLAN_FIELDS.items():
                if field in fields and fields[field] not in valid:
                    errors.append(
                        f"render_plan.{field} = '{fields[field]}' "
                        f"not in {valid}"
                    )

            # Validate resolution format
            if "resolution" in fields:
                if not re.match(r"\d+x\d+", fields["resolution"]):
                    errors.append(
                        f"render_plan.resolution = '{fields['resolution']}' "
                        f"should be WIDTHxHEIGHT"
                    )

            # Validate fps is numeric
            if "fps" in fields:
                try:
                    float(fields["fps"])
                except ValueError:
                    errors.append(f"render_plan.fps = '{fields['fps']}' is not numeric")

    # Validate cli_commands has vcr check before vcr build
    if "cli_commands" in sections:
        cmds = extract_cli_commands(sections["cli_commands"])
        has_check = any("vcr check" in c for c in cmds)
        has_build = any("vcr build" in c for c in cmds)

        if has_build and not has_check:
            errors.append("cli_commands: vcr build without preceding vcr check")

        if has_check and has_build:
            check_idx = next(i for i, c in enumerate(cmds) if "vcr check" in c)
            build_idx = next(i for i, c in enumerate(cmds) if "vcr build" in c)
            if check_idx > build_idx:
                errors.append("cli_commands: vcr check must come before vcr build")

        # Check output is .mov for ProRes
        for cmd in cmds:
            if "vcr build" in cmd and "-o" in cmd:
                m = re.search(r"-o\s+(\S+)", cmd)
                if m and not m.group(1).endswith(".mov"):
                    errors.append(
                        f"cli_commands: output '{m.group(1)}' should be .mov for ProRes"
                    )

    return errors


def validate_live(text: str, project_dir: str = ".") -> list[str]:
    """Extract manifests from cli_commands and run vcr check on them."""
    errors = []
    sections = parse_sections(text)

    if "cli_commands" not in sections:
        return ["No cli_commands section for live validation"]

    cmds = extract_cli_commands(sections["cli_commands"])

    for cmd in cmds:
        if "vcr check" not in cmd:
            continue

        # Extract manifest path
        parts = cmd.split()
        try:
            idx = parts.index("check") + 1
            manifest_path = parts[idx]
        except (ValueError, IndexError):
            continue

        # Skip placeholders
        if "<" in manifest_path:
            continue

        full_path = os.path.join(project_dir, manifest_path)
        if not os.path.exists(full_path):
            errors.append(f"Manifest not found: {full_path}")
            continue

        result = subprocess.run(
            ["vcr", "check", full_path],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode != 0:
            errors.append(
                f"vcr check {manifest_path} failed (exit {result.returncode}): "
                f"{result.stderr.strip()}"
            )

    return errors


def main():
    parser = argparse.ArgumentParser(description="Validate VCR render plan response")
    parser.add_argument("input", help="Path to response file, or - for stdin")
    parser.add_argument("--live", action="store_true",
                       help="Also run vcr check on referenced manifests")
    parser.add_argument("--project-dir", default=".",
                       help="Project directory for resolving manifest paths")
    args = parser.parse_args()

    if args.input == "-":
        text = sys.stdin.read()
    else:
        with open(args.input) as f:
            text = f.read()

    # Schema validation
    errors = validate_schema(text)
    if errors:
        print("SCHEMA VALIDATION FAILED:")
        for e in errors:
            print(f"  - {e}")
        sys.exit(1)

    print("Schema validation passed.")

    # Live validation
    if args.live:
        live_errors = validate_live(text, args.project_dir)
        if live_errors:
            print("LIVE VALIDATION FAILED:")
            for e in live_errors:
                print(f"  - {e}")
            sys.exit(2)
        print("Live validation passed.")

    sys.exit(0)


if __name__ == "__main__":
    main()
