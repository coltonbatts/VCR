#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENV_DIR="${VCR_PACK_CONTACT_VENV:-$ROOT_DIR/.venv-tools}"
PYTHON_BIN="$VENV_DIR/bin/python"

if [[ ! -x "$PYTHON_BIN" ]]; then
  python3 -m venv "$VENV_DIR"
fi

if ! "$PYTHON_BIN" -c "import PIL" >/dev/null 2>&1; then
  "$PYTHON_BIN" -m pip install pillow
fi

exec "$PYTHON_BIN" "$ROOT_DIR/scripts/pack_contact_sheet.py" "$@"
