# VCR Exit Codes

VCR uses stable process exit codes for scripting and CI. See `docs/ARCHITECTURE.md` for system overview.

| Code | Meaning | Typical examples |
| --- | --- | --- |
| `0` | Success | Command completed normally |
| `2` | Usage / argument error | Invalid `--set`, incompatible flags, out-of-range frame args |
| `3` | Manifest validation error | YAML/schema validation, bad substitutions, unknown manifest fields |
| `4` | Missing dependency | `ffmpeg` missing, required fonts missing |
| `5` | I/O error | Failed manifest read/write, metadata write failure, filesystem errors, encoder (ffmpeg) failure |
| `6` | Blocked | `vcr prompt --strict` with unresolved requirements (otherwise `prompt` exits 0 and reports `status: blocked`) |

Codes `3` also covers *negative findings that are results, not crashes*: `lint` issues, `verify` mismatches and `inspect` errors exit 3 with a full JSON document on stdout when `--json` is set. Code `2` also covers requests the selected backend cannot honor (`UNSUPPORTED_SOFTWARE_LAYER_TYPES`, `UNSUPPORTED_SOFTWARE_FEATURES`).

Notes:
- Errors are prefixed with the command name (for example `vcr check: ...`).
- With `--json`, stdout carries one contract document (`vcr.agent/1`) whose `error.exit_code` / `status` match the process exit code. See `docs/AGENT_CONTRACT.md`.
- Set `VCR_ERROR_VERBOSE=1` to print cause-chain details after the summary line.
