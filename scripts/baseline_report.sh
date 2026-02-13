#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${1:-renders/baseline}"
mkdir -p "$OUT_DIR"
BASELINE_GPU="${BASELINE_GPU:-1}"
BASELINE_STRESS="${BASELINE_STRESS:-0}"

BIN="$ROOT_DIR/target/release/vcr"
if [[ ! -x "$BIN" ]]; then
  cargo build --release --bin vcr >/dev/null
fi

TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
REPORT_PATH="$OUT_DIR/baseline_report.json"

declare -a CASES=(
  "examples/white_on_alpha.vcr|0|auto"
  "examples/white_on_alpha.vcr|0|software"
  "examples/white_on_alpha.vcr|0|gpu"
  "manifests/post_levels_example.vcr|12|auto"
  "manifests/post_levels_example.vcr|12|software"
  "manifests/post_levels_example.vcr|12|gpu"
  "manifests/ascii_post_debug.vcr|12|auto"
  "manifests/ascii_post_debug.vcr|12|software"
  "manifests/ascii_post_debug.vcr|12|gpu"
)

if [[ "$BASELINE_GPU" != "1" ]]; then
  filtered_cases=()
  for case_entry in "${CASES[@]}"; do
    if [[ "$case_entry" != *"|gpu" ]]; then
      filtered_cases+=("$case_entry")
    fi
  done
  CASES=("${filtered_cases[@]}")
fi

if [[ "$BASELINE_STRESS" == "1" ]]; then
  CASES+=("manifests/ascii_post_debug.vcr|120|software")
fi

json_escape() {
  local input="$1"
  input=${input//\\/\\\\}
  input=${input//\"/\\\"}
  input=${input//$'\n'/ }
  input=${input//$'\r'/ }
  input=${input//$'\t'/ }
  printf '%s' "$input"
}

to_cli_path() {
  local path="$1"
  if [[ "$path" == "$ROOT_DIR/"* ]]; then
    printf '%s' "${path#"$ROOT_DIR/"}"
    return
  fi
  printf '%s' "$path"
}

extract_timing_line() {
  local log="$1"
  local line
  line="$(printf '%s\n' "$log" | rg "^\[VCR\] timing" -N | tail -n1 || true)"
  printf '%s' "$line"
}

extract_timing_field() {
  local line="$1"
  local field="$2"
  printf '%s\n' "$line" | sed -n "s/.*${field}=\\([^ ]*\\).*/\\1/p" | head -n1
}

extract_json_field() {
  local json="$1"
  local key="$2"
  printf '%s\n' "$json" | sed -n "s/.*\"${key}\": \"\([^\"]*\)\".*/\1/p" | head -n1
}

printf '{\n' > "$REPORT_PATH"
printf '  "generated_at": "%s",\n' "$TIMESTAMP" >> "$REPORT_PATH"
printf '  "tool": "scripts/baseline_report.sh",\n' >> "$REPORT_PATH"
printf '  "cases": [\n' >> "$REPORT_PATH"

for i in "${!CASES[@]}"; do
  IFS='|' read -r manifest frame backend <<< "${CASES[$i]}"
  manifest_abs="$ROOT_DIR/$manifest"
  stem="$(basename "$manifest" .vcr)"

  png_out="$OUT_DIR/${stem}_f${frame}_${backend}.png"
  png_out_cli="$(to_cli_path "$png_out")"

  render_output="$($BIN --backend "$backend" render-frame "$manifest_abs" --frame "$frame" -o "$png_out_cli" 2>&1)"
  timing_line="$(extract_timing_line "$render_output")"
  parse_ms="$(extract_timing_field "$timing_line" "parse")"
  layout_ms="$(extract_timing_field "$timing_line" "layout")"
  render_ms="$(extract_timing_field "$timing_line" "render")"
  encode_ms="$(extract_timing_field "$timing_line" "encode")"
  total_ms="$(extract_timing_field "$timing_line" "total")"

  frame_hash=""
  if [[ "$backend" == "software" ]]; then
    hash_json="$($BIN determinism-report "$manifest_abs" --frame "$frame" --json)"
    frame_hash="$(extract_json_field "$hash_json" "frame_hash")"
  fi

  metadata_path="${png_out_cli}.metadata.json"
  actual_backend=""
  if [[ -f "$metadata_path" ]]; then
    actual_backend="$(sed -n 's/.*"backend": "\([^"]*\)".*/\1/p' "$metadata_path" | head -n1)"
  fi

  comma=","
  if [[ "$i" -eq $((${#CASES[@]} - 1)) ]]; then
    comma=""
  fi

  printf '    {\n' >> "$REPORT_PATH"
  printf '      "manifest": "%s",\n' "$(json_escape "$manifest")" >> "$REPORT_PATH"
  printf '      "frame": %s,\n' "$frame" >> "$REPORT_PATH"
  printf '      "requested_backend": "%s",\n' "$(json_escape "$backend")" >> "$REPORT_PATH"
  printf '      "actual_backend": "%s",\n' "$(json_escape "$actual_backend")" >> "$REPORT_PATH"
  printf '      "timing_line": "%s",\n' "$(json_escape "$timing_line")" >> "$REPORT_PATH"
  printf '      "timing_parse": "%s",\n' "$(json_escape "$parse_ms")" >> "$REPORT_PATH"
  printf '      "timing_layout": "%s",\n' "$(json_escape "$layout_ms")" >> "$REPORT_PATH"
  printf '      "timing_render": "%s",\n' "$(json_escape "$render_ms")" >> "$REPORT_PATH"
  printf '      "timing_encode": "%s",\n' "$(json_escape "$encode_ms")" >> "$REPORT_PATH"
  printf '      "timing_total": "%s",\n' "$(json_escape "$total_ms")" >> "$REPORT_PATH"
  printf '      "frame_hash": "%s",\n' "$(json_escape "$frame_hash")" >> "$REPORT_PATH"
  printf '      "png_output": "%s",\n' "$(json_escape "$png_out")" >> "$REPORT_PATH"
  printf '      "metadata_output": "%s"\n' "$(json_escape "$metadata_path")" >> "$REPORT_PATH"
  printf '    }%s\n' "$comma" >> "$REPORT_PATH"
done

printf '  ]\n' >> "$REPORT_PATH"
printf '}\n' >> "$REPORT_PATH"

printf 'Wrote %s\n' "$REPORT_PATH"
