# VHS Tape Deck (`tape-deck`)

`vhs-tape-deck` is a Bubble Tea terminal UI for driving VCR renders as if they were VHS tapes.

- Left panel: tape shelf
- Right top: animated cassette + metadata
- Right bottom: live logs (stdout/stderr)
- Footer: keybinds, status, last output

Runs are reproducible and logged to JSON run records.

## Features (v0.1)

- Tape shelf with deterministic tape presets from YAML config
- Insert/eject selected tape (`Enter`)
- Play primary render (`Space`)
- Preview frame render (`P`) when enabled
- Live log streaming while process runs
- Deterministic ASCII cassette animation driven by app ticks
- Dry-run mode (`D`) to print command only
- Run record JSON saved per run

## Install / Build

```bash
cd /Users/coltonbatts/Desktop/VCR/vhs-tape-deck
go mod tidy
go build -o tape-deck ./cmd/tape-deck
```

## Quick Start

```bash
# create starter config with 5 tapes
./tape-deck init

# run the UI
./tape-deck run

# run is implied
./tape-deck
```

## Keybinds

- `↑/k`: previous tape
- `↓/j`: next tape
- `Enter`: insert/eject selected tape
- `Space`: play primary render for inserted tape
- `P`: preview frame render (if enabled)
- `Ctrl+X`: cancel active run
- `L`: clear logs
- `D`: toggle dry-run
- `H` or `?`: help overlay
- `Q` or `Ctrl+C`: quit

## Config Location

Default config path is OS-specific:

- macOS: `~/Library/Application Support/vhs-tape-deck/config.yaml`
- Linux: `~/.config/vhs-tape-deck/config.yaml`
- Windows: `%AppData%/vhs-tape-deck/config.yaml`

## Config Schema

```yaml
vcr_binary: vcr                # optional, default: vcr
output_flag: --output          # optional, default: --output
project_root: /path/to/project # optional, default: cwd at launch
runs_dir: /path/to/runs        # optional, default: <configDir>/runs
env:
  VCR_SEED: "0"

tapes:
  - id: alpha-lower-third
    name: Alpha Lower Third
    manifest: ./manifests/alpha_lower_third.yaml
    mode: video                # video | frame
    primary_args: ["--duration", "5", "--fps", "60"]
    output_dir: ./renders/alpha # optional, default: <runs_dir>/<tapeId>
    preview:
      enabled: true
      frame: 48
      args: ["--fps", "60"]
    aesthetic:
      label_style: clean        # clean | noisy | handwritten
      shell_colorway: black     # black | gray | clear
    notes: Broadcast-safe lower third
```

## Command Resolution Rules

- If `primary_args` begins with a subcommand (non-flag), it is treated as a full command payload.
- Otherwise defaults are used:
  - `video` primary: `vcr render <manifest> ...`
  - `frame` primary: `vcr render-frame <manifest> --frame 0 ...`
  - preview: `vcr render-frame <manifest> --frame <preview.frame> ...`
- If output flag is missing, output path is auto-injected using `output_flag`:
  - primary video: `<output_dir>/<run_id>.mov`
  - primary frame: `<output_dir>/<run_id>.png`
  - preview: `<output_dir>/<run_id>_preview.png`

## Run Records

Run records are written to:

- `<runs_dir>/records/<run_id>.json`

`run_id` format:

- `YYYYMMDD_HHMMSS_tapeId_counter`

Example record is in [`docs/sample-run-record.json`](docs/sample-run-record.json).

## Troubleshooting

- `load config ... no such file`: run `tape-deck init`
- `vcr` not found: set `vcr_binary` in config to an absolute path
- preview command fails: check if your VCR build supports `render-frame`
- no output path in record: your args likely specify custom output handling

## Dev

```bash
go test ./...
go build ./...
```
