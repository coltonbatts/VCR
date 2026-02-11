# VCR Exit Codes

VCR uses stable process exit codes for scripting and CI:

| Code | Meaning | Typical examples |
| --- | --- | --- |
| `0` | Success | Command completed normally |
| `2` | Usage / argument error | Invalid `--set`, incompatible flags, out-of-range frame args |
| `3` | Manifest validation error | YAML/schema validation, bad substitutions, unknown manifest fields |
| `4` | Missing dependency | `ffmpeg` missing, required fonts missing |
| `5` | I/O error | Failed manifest read/write, metadata write failure, filesystem errors |

Notes:
- Errors are prefixed with the command name (for example `vcr check: ...`).
- Set `VCR_ERROR_VERBOSE=1` to print cause-chain details after the summary line.
