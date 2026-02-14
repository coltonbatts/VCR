### 1. intent_summary

Render a circle animation to ProRes.

### 2. capability_check

Supported.

### 3. render_plan

| Field | Value |
|---|---|
| stage_type | `raster` |
| resolution | `1920x1080` |
| fps | `24` |
| duration | `5` |
| backend | `auto` |
| alpha | `false` |
| prores_profile | `422hq` |
| source_mode | `manifest` |
| determinism_mode | `on` |

### 4. required_assets

Manifest `circle.vcr`.

### 5. cli_commands

```bash
vcr build circle.vcr -o renders/circle.mov
```

### 6. expected_outputs

`renders/circle.mov`

### 7. validation_steps

```bash
test -f renders/circle.mov
```
