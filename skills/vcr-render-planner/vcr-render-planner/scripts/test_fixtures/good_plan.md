### 1. intent_summary

Render white ASCII text animation on transparent background as ProRes 4444 at 60fps for 6 seconds.

### 2. capability_check

Supported. ASCII layer with white foreground, transparent background, encoded to ProRes 4444 with alpha.

### 3. render_plan

| Field | Value |
|---|---|
| stage_type | `ascii` |
| resolution | `1920x1080` |
| fps | `60` |
| duration | `6` |
| backend | `software` |
| alpha | `true` |
| prores_profile | `4444` |
| font | `GeistPixel-Line` |
| ascii_grid | `120x45` |
| source_mode | `manifest` |
| determinism_mode | `on` |

### 4. required_assets

Manifest `wave.vcr` with ASCII layer, foreground `{r:1,g:1,b:1,a:1}`, background `{r:0,g:0,b:0,a:0}`.

### 5. cli_commands

```bash
vcr check wave.vcr
vcr build wave.vcr -o renders/wave.mov --backend software
```

### 6. expected_outputs

`renders/wave.mov` (ProRes 4444, 1920x1080, 60fps, 6s, alpha channel present)

### 7. validation_steps

```bash
test -f renders/wave.mov
ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,pix_fmt renders/wave.mov
# expect: codec_name=prores, pix_fmt=yuva444p10le (alpha present)
```
