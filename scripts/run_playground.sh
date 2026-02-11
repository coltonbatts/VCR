#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

VCR_BIN="$ROOT_DIR/target/release/vcr"
PLAYGROUND_ROOT="$ROOT_DIR/renders/playground"
INDEX_PATH="$PLAYGROUND_ROOT/index.json"
FRAME_COUNT=24

declare -a ENTRIES_JSON=()
declare -a RAN_LABELS=()
declare -a CONTACT_SHEETS=()

json_escape() {
  local value="$1"
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//$'\n'/\\n}
  printf '%s' "$value"
}

param_value_json() {
  local value="$1"
  if [[ "$value" =~ ^-?[0-9]+([.][0-9]+)?$ ]]; then
    printf '%s' "$value"
  elif [[ "$value" == "true" || "$value" == "false" ]]; then
    printf '%s' "$value"
  else
    printf '"%s"' "$(json_escape "$value")"
  fi
}

ensure_release_binary() {
  if [[ -x "$VCR_BIN" ]]; then
    return
  fi

  if ! command -v cargo >/dev/null 2>&1; then
    echo "[playground] Missing release binary: $VCR_BIN" >&2
    echo "[playground] Cargo not found. Build it manually with: cargo build --release --bin vcr" >&2
    exit 1
  fi

  echo "[playground] Release binary missing. Building with cargo..."
  cargo build --release --bin vcr
}

run_preset() {
  local scene="$1"
  local manifest="$2"
  local preset="$3"
  shift 3
  local -a set_pairs=("$@")

  local output_rel="renders/playground/${scene}/${preset}"
  local output_abs="$ROOT_DIR/$output_rel"
  local metadata_rel="${scene}/${preset}/preview.metadata.json"

  echo "[playground] Running ${scene}/${preset}"
  mkdir -p "$output_abs"

  local -a cmd=(
    "$VCR_BIN"
    "--quiet"
    "preview"
    "$manifest"
    "--image-sequence"
    "--frames"
    "$FRAME_COUNT"
    "-o"
    "$output_rel"
  )

  local pair=""
  for pair in "${set_pairs[@]}"; do
    cmd+=("--set" "$pair")
  done

  "${cmd[@]}"

  local params_json="{"
  local first=1
  for pair in "${set_pairs[@]}"; do
    local name="${pair%%=*}"
    local value="${pair#*=}"
    if [[ $first -eq 0 ]]; then
      params_json+=", "
    fi
    first=0
    params_json+="\"$(json_escape "$name")\": $(param_value_json "$value")"
  done
  params_json+="}"

  ENTRIES_JSON+=("{\"scene\":\"$(json_escape "$scene")\",\"preset\":\"$(json_escape "$preset")\",\"output_folder\":\"$(json_escape "$output_rel")\",\"params\":${params_json},\"metadata\":\"$(json_escape "$metadata_rel")\"}")
  RAN_LABELS+=("${scene}/${preset}")
}

pick_contact_frame() {
  local preset_dir="$1"
  local mid="$preset_dir/frame_000012.png"
  local last_nominal="$preset_dir/frame_000023.png"
  if [[ -f "$mid" ]]; then
    printf '%s' "$mid"
    return 0
  fi
  if [[ -f "$last_nominal" ]]; then
    printf '%s' "$last_nominal"
    return 0
  fi

  shopt -s nullglob
  local frames=("$preset_dir"/frame_*.png)
  shopt -u nullglob
  if [[ ${#frames[@]} -eq 0 ]]; then
    return 1
  fi

  local last_index=$(( ${#frames[@]} - 1 ))
  printf '%s' "${frames[$last_index]}"
}

maybe_generate_contact_sheet() {
  local scene="$1"

  if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "[playground] ffmpeg not found; skipping contact sheet for ${scene}"
    return
  fi

  local scene_dir="$PLAYGROUND_ROOT/$scene"
  local default_frame
  local aggressive_frame
  local minimal_frame
  local output_sheet="$scene_dir/contact_sheet.png"

  default_frame="$(pick_contact_frame "$scene_dir/default")" || {
    echo "[playground] Missing frames for ${scene}/default; skipping contact sheet"
    return
  }
  aggressive_frame="$(pick_contact_frame "$scene_dir/aggressive")" || {
    echo "[playground] Missing frames for ${scene}/aggressive; skipping contact sheet"
    return
  }
  minimal_frame="$(pick_contact_frame "$scene_dir/minimal")" || {
    echo "[playground] Missing frames for ${scene}/minimal; skipping contact sheet"
    return
  }

  ffmpeg -hide_banner -loglevel error -y \
    -i "$default_frame" \
    -i "$aggressive_frame" \
    -i "$minimal_frame" \
    -filter_complex "hstack=inputs=3" \
    "$output_sheet"

  CONTACT_SHEETS+=("${scene}/contact_sheet.png")
  echo "[playground] Wrote renders/playground/${scene}/contact_sheet.png"
}

write_index_json() {
  mkdir -p "$PLAYGROUND_ROOT"

  {
    echo "{"
    echo "  \"generated_by\": \"scripts/run_playground.sh\","
    echo "  \"frame_count\": $FRAME_COUNT,"
    echo "  \"entries\": ["
    local i=0
    while [[ $i -lt ${#ENTRIES_JSON[@]} ]]; do
      if [[ $i -gt 0 ]]; then
        echo ","
      fi
      printf '    %s' "${ENTRIES_JSON[$i]}"
      i=$((i + 1))
    done
    echo
    echo "  ]"
    echo "}"
  } > "$INDEX_PATH"
}

main() {
  ensure_release_binary
  mkdir -p "$PLAYGROUND_ROOT"

  run_preset "instrument_typography" "examples/instrument_typography.vcr" "default" \
    "scan_speed=1.0" "accent_color=#38DCEF" "noise_intensity=0.30"
  run_preset "instrument_typography" "examples/instrument_typography.vcr" "aggressive" \
    "scan_speed=2.4" "accent_color=#FF4C73" "noise_intensity=1.20"
  run_preset "instrument_typography" "examples/instrument_typography.vcr" "minimal" \
    "scan_speed=0.55" "accent_color=#A7F5DA" "noise_intensity=0.08"

  run_preset "instrument_grid" "examples/instrument_grid.vcr" "default" \
    "grid_scale=1.0" "jitter=0.30" "contrast=1.0"
  run_preset "instrument_grid" "examples/instrument_grid.vcr" "aggressive" \
    "grid_scale=1.55" "jitter=1.40" "contrast=2.0"
  run_preset "instrument_grid" "examples/instrument_grid.vcr" "minimal" \
    "grid_scale=0.72" "jitter=0.05" "contrast=0.65"

  run_preset "instrument_logo_reveal" "examples/instrument_logo_reveal.vcr" "default" \
    "reveal_duration=1.0" "reveal_bias=0.70" "accent_color=#F56A3A"
  run_preset "instrument_logo_reveal" "examples/instrument_logo_reveal.vcr" "aggressive" \
    "reveal_duration=0.50" "reveal_bias=0.95" "accent_color=#FF2C84"
  run_preset "instrument_logo_reveal" "examples/instrument_logo_reveal.vcr" "minimal" \
    "reveal_duration=1.90" "reveal_bias=0.25" "accent_color=#7BE8D4"

  write_index_json

  maybe_generate_contact_sheet "instrument_typography"
  maybe_generate_contact_sheet "instrument_grid"
  maybe_generate_contact_sheet "instrument_logo_reveal"

  echo
  echo "[playground] Completed ${#RAN_LABELS[@]} preset renders."
  local label=""
  for label in "${RAN_LABELS[@]}"; do
    echo "  - $label"
  done
  echo "[playground] Index: renders/playground/index.json"
  if [[ ${#CONTACT_SHEETS[@]} -gt 0 ]]; then
    echo "[playground] Contact sheets:"
    for label in "${CONTACT_SHEETS[@]}"; do
      echo "  - renders/playground/${label}"
    done
  else
    echo "[playground] Contact sheets: none generated"
  fi
  echo "[playground] Output root: renders/playground"
}

main "$@"
