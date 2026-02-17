# Dreamcore & Y2K Asset Pack Implementation Plan

## 1. Scope and Deliverables
- Deliver 10 procedural motion assets as `.mov` files with alpha.
- Per asset target:
  - Resolution: `1080x1080`
  - Duration: `5.0s`
  - FPS: `60`
  - Codec profile: `ProRes 4444`
  - Vendor tag: `apl0` (QuickTime/macOS compatibility)
- Source authoring layout:
  - Manifests: `manifests/dreamcore_set/*.vcr`
  - WGSL shaders: `manifests/dreamcore_set/shaders/*.wgsl`

## 2. Prompt-Gate Baseline (Blocking Unknowns Resolved)
- Ran `vcr prompt` on natural language brief first (required by repo policy).
- Initial run surfaced blockers (`unknowns_and_fixes`) from ambiguous style parsing.
- Created explicit YAML prompt input and reran normalization.
- Final normalized artifact: `manifests/dreamcore_set/prompt.resolved.normalized.yaml`
  - Result: `unknowns_and_fixes: []`
  - Locked params: `1080x1080`, `60fps`, `5.0s`, `alpha: true`, `prores_4444`, seed `0`.

## 3. Asset List and File Mapping
1. `01_iridescent_chrome_heart`
2. `02_cloud_gate`
3. `03_error_window_drift`
4. `04_wireframe_star_trails`
5. `05_dolphin_prism`
6. `06_celestial_dither_sun`
7. `07_y2k_tech_ring`
8. `08_cdrom_rainbow`
9. `09_falling_data_water`
10. `10_butterfly_pulse`

Each item gets:
- `manifests/dreamcore_set/<name>.vcr`
- `manifests/dreamcore_set/shaders/<name>.wgsl`

## 4. Shader + Manifest Authoring Rules
- WGSL entrypoint signature for every shader:

```wgsl
fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
    let time = uniforms.time;
    let res = uniforms.resolution;
    // logic
}
```

- Shaders are procedural only; no external path dependencies.
- Manifest shader references are relative to manifest file:
  - `path: "shaders/<name>.wgsl"`
- Encoding block required in every manifest:

```yaml
encoding:
  prores_profile: prores4444
  vendor: "apl0"
```

## 5. Validation Sequence
- Per-manifest validation:
  - `vcr check manifests/dreamcore_set/<name>.vcr`
  - `vcr lint manifests/dreamcore_set/<name>.vcr`
- Batch validation (loop all 10 manifests).

## 6. Render Sequence
- Batch render command style (per request):
  - `cargo run --release -- render manifests/dreamcore_set/<name>.vcr -o renders/dreamcore_set/<name>.mov`
- Output folder:
  - `renders/dreamcore_set/`

## 7. Verification Sequence
- Inspect each MOV with ffprobe:

```bash
ffprobe -v error -select_streams v:0 \
  -show_entries stream=codec_name,codec_tag_string,pix_fmt,width,height,r_frame_rate \
  -of default=noprint_wrappers=1:nokey=0 renders/dreamcore_set/<name>.mov
```

- Acceptance target from request:
  - pixel format should report `yuva444p12le`.

## 8. Final Report
- Summarize:
  - check/lint status for all 10 manifests
  - render status for all 10 outputs
  - ffprobe pixel format per output
  - any deviations from expected `yuva444p12le`
