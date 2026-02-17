#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: scripts/ascii_link_overlay.sh <url1> [url2 ...] [-- extra args]"
  echo "Example: scripts/ascii_link_overlay.sh https://www.ascii.co.uk/animated-art/milk-water-droplet-animated-ascii-art.html -- --width 1920 --height 1080"
  echo "Example: scripts/ascii_link_overlay.sh <url1> <url2> -- --fps 24"
  exit 1
fi

URLS=()
EXTRA_ARGS=()
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "--" ]]; then
    shift
    EXTRA_ARGS=("$@")
    break
  fi
  URLS+=("$1")
  shift
done

if [[ ${#URLS[@]} -eq 0 ]]; then
  echo "No URL arguments provided."
  exit 1
fi

for URL in "${URLS[@]}"; do
  echo "Importing $URL"
  cargo run --features workflow --bin ascii-link-overlay -- --url "$URL" "${EXTRA_ARGS[@]}"
done
