use std::fs;
use std::path::Path;
use std::process::Command;

use image::{Rgba, RgbaImage};
use serde_json::Value;
use serde_yaml::Value as YamlValue;
use tempfile::tempdir;

fn write_manifest(path: &Path, yaml: &str) {
    fs::write(path, yaml).expect("manifest should write");
}

fn run_vcr(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("vcr command should run")
}

fn command_available(name: &str, version_arg: &str) -> bool {
    Command::new(name)
        .arg(version_arg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn no_args_prints_help_with_quick_start_footer() {
    let dir = tempdir().expect("tempdir should create");
    let output = run_vcr(dir.path(), &[]);
    assert!(output.status.success(), "no-args help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("Quick start:"));
    assert!(stdout.contains("vcr tape init"));
    assert!(stdout.contains("vcr tape list"));
    assert!(stdout.contains("vcr tape run <id>"));
    assert!(stdout.contains("vcr deck"));
}

#[test]
fn prompt_command_outputs_standardized_yaml_bundle() {
    let dir = tempdir().expect("tempdir should create");
    let output = run_vcr(
        dir.path(),
        &[
            "prompt",
            "--text",
            "Render a 5s alpha intro at 30fps to ./renders/intro.mov",
        ],
    );
    assert!(output.status.success(), "prompt command should succeed");

    let parsed: YamlValue =
        serde_yaml::from_slice(&output.stdout).expect("prompt output should be valid YAML");
    assert!(parsed.get("standardized_vcr_prompt").is_some());
    assert_eq!(
        parsed["normalized_spec"]["render"]["fps"].as_i64(),
        Some(30),
        "render fps should be parsed from natural language input"
    );
    assert_eq!(
        parsed["normalized_spec"]["output"]["path"].as_str(),
        Some("./renders/intro.mov"),
        "output path should round-trip into normalized spec"
    );
}

#[test]
fn params_json_output_is_stable_and_sorted() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 32, height: 32 }
  fps: 24
  duration: { frames: 2 }
params:
  zeta:
    type: float
    default: 2.0
  alpha:
    type: float
    default: 1.0
layers:
  - id: bg
    opacity: "0.4 + alpha * 0.0 + zeta * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let first = run_vcr(dir.path(), &["params", "scene.vcr", "--json"]);
    assert!(first.status.success(), "params --json should succeed");

    let second = run_vcr(dir.path(), &["params", "scene.vcr", "--json"]);
    assert!(second.status.success(), "params --json should succeed");
    assert_eq!(first.stdout, second.stdout, "json output should be stable");

    let parsed: Value = serde_json::from_slice(&first.stdout).expect("json should parse");
    let params = parsed["params"]
        .as_object()
        .expect("params should be object");
    let keys = params.keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys, vec!["alpha".to_owned(), "zeta".to_owned()]);
}

#[test]
fn explain_json_output_is_stable_and_sorted() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 32, height: 32 }
  fps: 24
  duration: { frames: 2 }
params:
  zeta:
    type: float
    default: 2.0
  alpha:
    type: float
    default: 1.0
layers:
  - id: bg
    opacity: "0.4 + alpha * 0.0 + zeta * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let first = run_vcr(
        dir.path(),
        &[
            "explain",
            "scene.vcr",
            "--set",
            "zeta=3.0",
            "--set",
            "alpha=2.0",
            "--json",
        ],
    );
    assert!(first.status.success(), "explain --json should succeed");

    let second = run_vcr(
        dir.path(),
        &[
            "explain",
            "scene.vcr",
            "--set",
            "alpha=2.0",
            "--set",
            "zeta=3.0",
            "--json",
        ],
    );
    assert!(second.status.success(), "explain --json should succeed");

    let parsed_first: Value = serde_json::from_slice(&first.stdout).expect("json should parse");
    let parsed_second: Value = serde_json::from_slice(&second.stdout).expect("json should parse");
    assert_eq!(
        parsed_first["manifest_hash"], parsed_second["manifest_hash"],
        "override ordering should not change manifest hash"
    );

    let resolved = parsed_first["resolved_params"]
        .as_object()
        .expect("resolved_params should be object");
    let keys = resolved.keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys, vec!["alpha".to_owned(), "zeta".to_owned()]);

    let backend = parsed_first["backend_preflight"]
        .as_object()
        .expect("backend_preflight should be object");
    assert_eq!(backend["requested_backend"], "auto");
    assert_eq!(backend["recommended_backend"], "software");
    assert_eq!(backend["software_compatible"], Value::Bool(true));
    assert_eq!(
        backend["unsupported_software_layers"]
            .as_array()
            .expect("unsupported layers should be array")
            .len(),
        0
    );
    assert_eq!(
        backend["software_supported_layer_types"]
            .as_array()
            .expect("supported layer types should be array")
            .iter()
            .map(|value| value.as_str().expect("layer type should be string"))
            .collect::<Vec<_>>(),
        vec!["asset", "image", "procedural", "text", "ascii", "sequence"]
    );
    assert_eq!(
        backend["blockers"]
            .as_array()
            .expect("blockers should be array")
            .len(),
        0
    );
}

#[test]
fn explain_json_reports_software_incompatibility_preflight() {
    let dir = tempdir().expect("tempdir should create");
    fs::write(dir.path().join("shader.wgsl"), "// placeholder").expect("shader file should write");
    fs::write(dir.path().join("clip.mov"), "").expect("video file should write");
    fs::write(dir.path().join("anim.json"), "{}").expect("lottie file should write");

    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: shader_only
    shader:
      fragment: |
        fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
          return vec4<f32>(uv.x, uv.y, 0.0, 1.0);
        }
  - id: wgpu_only
    wgpu_shader:
      shader_path: ./shader.wgsl
      width: 16
      height: 16
      time_mode: seconds
  - id: video_only
    video:
      path: ./clip.mov
  - id: lottie_only
    lottie:
      path: ./anim.json
"#,
    );

    let manifest_arg = manifest_path.to_string_lossy().to_string();
    let output = run_vcr(
        dir.path(),
        &["explain", &manifest_arg, "--backend", "software", "--json"],
    );
    assert!(
        output.status.success(),
        "explain --json should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json should parse");
    let backend = parsed["backend_preflight"]
        .as_object()
        .expect("backend_preflight should be object");
    assert_eq!(backend["requested_backend"], "software");
    assert_eq!(backend["recommended_backend"], "gpu");
    assert_eq!(backend["software_compatible"], Value::Bool(false));

    let unsupported = backend["unsupported_software_layers"]
        .as_array()
        .expect("unsupported layers should be array");
    assert_eq!(unsupported.len(), 4);
    assert_eq!(unsupported[0]["id"], "shader_only");
    assert_eq!(unsupported[0]["kind"], "shader");
    assert_eq!(unsupported[1]["id"], "wgpu_only");
    assert_eq!(unsupported[1]["kind"], "wgpu_shader");
    assert_eq!(unsupported[2]["id"], "video_only");
    assert_eq!(unsupported[2]["kind"], "video");
    assert_eq!(unsupported[3]["id"], "lottie_only");
    assert_eq!(unsupported[3]["kind"], "lottie");

    let blockers = backend["blockers"]
        .as_array()
        .expect("blockers should be array");
    assert_eq!(blockers.len(), 1);
    assert!(blockers[0]
        .as_str()
        .expect("blocker should be string")
        .contains("--backend gpu"));
}

#[test]
fn quiet_mode_suppresses_nonessential_logs_but_keeps_success_outputs() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let output = run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "frame.png",
        ],
    );
    assert!(output.status.success(), "render-frame should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stdout.contains("Wrote frame.png"));
    assert!(stdout.contains("Wrote frame.png.metadata.json"));
    assert!(!stderr.contains("[VCR] Output path:"));
    assert!(!stderr.contains("[VCR] Backend:"));
    assert!(!stderr.contains("[VCR] Params"));
    assert!(!stderr.contains("[VCR] timing"));
}

#[test]
fn render_metadata_sidecar_includes_agent_context_layer_summaries() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    z_index: 0
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
  - id: fg
    z_index: 1
    procedural:
      kind: solid_color
      color: { r: 0, g: 0, b: 0, a: 1 }
"#,
    );

    let output = run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "frame.png",
            "--backend",
            "software",
        ],
    );
    assert!(
        output.status.success(),
        "render-frame should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta_path = dir.path().join("frame.png.metadata.json");
    let raw = fs::read(&meta_path).expect("metadata sidecar should exist");
    let parsed: Value = serde_json::from_slice(&raw).expect("metadata should parse as JSON");

    let agent = parsed
        .get("agent_context")
        .expect("metadata should include agent_context");
    assert_eq!(
        agent["manifest_hash"], parsed["resolved_manifest_hash"],
        "agent_context.manifest_hash should match resolved manifest hash"
    );

    let layers = agent["layers_rendered"]
        .as_array()
        .expect("layers_rendered should be an array");
    assert_eq!(layers.len(), 2, "expected one summary per manifest layer");
    assert_eq!(layers[0]["id"], "bg");
    assert_eq!(layers[1]["id"], "fg");
    assert!(
        layers[0].get("final_position").is_some() && layers[0].get("was_visible").is_some(),
        "layer summary should include position and visibility"
    );
}

#[test]
fn preview_image_sequence_default_output_is_manifest_scoped() {
    let dir = tempdir().expect("tempdir should create");

    let scene_a = dir.path().join("scene_a.vcr");
    write_manifest(
        &scene_a,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let scene_b = dir.path().join("scene_b.vcr");
    write_manifest(
        &scene_b,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 0, g: 0, b: 0, a: 1 }
"#,
    );

    let first = run_vcr(
        dir.path(),
        &[
            "preview",
            "scene_a.vcr",
            "--image-sequence",
            "--frames",
            "1",
        ],
    );
    assert!(first.status.success(), "first preview should succeed");

    let second = run_vcr(
        dir.path(),
        &[
            "preview",
            "scene_b.vcr",
            "--image-sequence",
            "--frames",
            "1",
        ],
    );
    assert!(second.status.success(), "second preview should succeed");

    assert!(dir.path().join("renders/scene_a_preview").is_dir());
    assert!(dir.path().join("renders/scene_b_preview").is_dir());
    assert!(dir
        .path()
        .join("renders/scene_a_preview/frame_000000.png")
        .is_file());
    assert!(dir
        .path()
        .join("renders/scene_b_preview/frame_000000.png")
        .is_file());
}

#[test]
fn explain_text_output_shows_only_non_default_changes() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 32, height: 32 }
  fps: 24
  duration: { frames: 2 }
params:
  speed:
    type: float
    default: 1.0
  gain:
    type: float
    default: 2.0
layers:
  - id: bg
    opacity: "0.4 + speed * 0.0 + gain * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let output = run_vcr(
        dir.path(),
        &[
            "explain",
            "scene.vcr",
            "--set",
            "speed=1.0",
            "--set",
            "gain=3.0",
        ],
    );
    assert!(output.status.success(), "explain should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("- overrides (non-default):"));
    assert!(stdout.contains("gain=3.000000"));
    assert!(!stdout.contains("speed=1.000000"));
    assert!(stdout.contains("- resolved_non_default_params:"));
    assert!(stdout.contains("- resolved_param_total=2"));
}

#[test]
fn ascii_lab_output_is_deterministic_and_includes_required_sections() {
    let dir = tempdir().expect("tempdir should create");

    let first = run_vcr(dir.path(), &["ascii", "lab"]);
    assert!(first.status.success(), "ascii lab should succeed");
    assert!(
        first.stderr.is_empty(),
        "ascii lab should not emit stderr on success"
    );

    let second = run_vcr(dir.path(), &["ascii", "lab"]);
    assert!(second.status.success(), "ascii lab should succeed");
    assert_eq!(
        first.stdout, second.stdout,
        "ascii lab output should be deterministic"
    );

    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("=== Pattern: Horizontal Gradient ==="));
    assert!(stdout.contains("=== Pattern: Radial Gradient ==="));
    assert!(stdout.contains("=== Pattern: Checkerboard ==="));
    assert!(stdout.contains("=== Pattern: Vertical Edge ==="));
    assert!(stdout.contains("=== Pattern: Moving Vertical Bar ==="));

    assert!(stdout.contains("Mode: temporal=none, dither=none"));
    assert!(stdout.contains("Mode: temporal=none, dither=FS"));
    assert!(stdout.contains("Mode: temporal=hysteresis, dither=none, band=8"));
    assert!(stdout.contains("Mode: temporal=hysteresis, dither=none, band=16"));
    assert!(stdout.contains("Mode: temporal=hysteresis, dither=FS, band=8"));

    assert!(stdout.contains("Hash: 0x"));
    assert!(stdout.contains("Frame 0 Hash: 0x"));
    assert!(stdout.contains("Canonical Sequence Hash: 0x"));
    assert!(stdout.contains("----------------------------------------"));
}

#[test]
fn ascii_lab_export_writes_txt_and_json_with_stage_hashes() {
    let dir = tempdir().expect("tempdir should create");
    let export_dir = "ascii_lab_exports";

    let output = run_vcr(
        dir.path(),
        &[
            "ascii",
            "lab",
            "--export-dir",
            export_dir,
            "--debug-stage-hashes",
        ],
    );
    assert!(output.status.success(), "ascii lab export should succeed");

    let export_root = dir.path().join(export_dir);
    assert!(export_root.is_dir(), "export dir should be created");

    let entries = fs::read_dir(&export_root)
        .expect("export dir should be readable")
        .filter_map(|entry| entry.ok())
        .collect::<Vec<_>>();
    let txt_count = entries
        .iter()
        .filter(|entry| entry.path().extension().and_then(|v| v.to_str()) == Some("txt"))
        .count();
    let json_count = entries
        .iter()
        .filter(|entry| entry.path().extension().and_then(|v| v.to_str()) == Some("json"))
        .count();
    assert_eq!(txt_count, 25, "expected one text export per pattern/mode");
    assert_eq!(json_count, 25, "expected one json export per pattern/mode");

    let sample_txt = export_root.join("horizontal_gradient_temporal_none__dither_none.txt");
    let txt = fs::read_to_string(&sample_txt).expect("sample txt should be readable");
    assert!(txt.contains("Mode: temporal=none, dither=none"));
    assert!(txt.contains("Hash: 0x"));

    let sample_json =
        export_root.join("moving_vertical_bar_temporal_hysteresis_band_8__dither_fs.json");
    let parsed: Value =
        serde_json::from_slice(&fs::read(&sample_json).expect("sample json should be readable"))
            .expect("sample json should parse");

    assert_eq!(parsed["mode"]["temporal"], "hysteresis");
    assert_eq!(parsed["mode"]["dither"], "FS");
    assert_eq!(parsed["mode"]["band"], 8);
    assert_eq!(
        parsed["frame_hashes"]
            .as_array()
            .map(|value| value.len())
            .unwrap_or_default(),
        3
    );
    assert!(parsed["canonical_sequence_hash"]
        .as_str()
        .map(|value| value.starts_with("0x"))
        .unwrap_or(false));
    assert_eq!(
        parsed["stage_hashes"]
            .as_array()
            .map(|value| value.len())
            .unwrap_or_default(),
        3
    );
}

#[test]
fn exit_codes_and_error_prefixes_are_consistent() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
params:
  speed:
    type: float
    default: 1.0
layers:
  - id: bg
    opacity: "0.4 + speed * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let usage = run_vcr(dir.path(), &["check", "scene.vcr", "--set", "speed"]);
    assert_eq!(usage.status.code(), Some(2));
    let usage_stderr = String::from_utf8_lossy(&usage.stderr);
    assert!(usage_stderr.contains("vcr check:"));
    assert!(usage_stderr.contains("expected NAME=VALUE"));

    let invalid_manifest_path = dir.path().join("bad_manifest.vcr");
    write_manifest(
        &invalid_manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
params:
  speed:
    type: float
    default: 1.0
layers:
  - id: t
    text:
      content: "speed=${speed}"
"#,
    );
    let manifest_validation = run_vcr(dir.path(), &["check", "bad_manifest.vcr"]);
    assert_eq!(manifest_validation.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&manifest_validation.stderr).contains("vcr check:"));

    let io_failure = run_vcr(dir.path(), &["check", "missing-file.vcr"]);
    assert_eq!(io_failure.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&io_failure.stderr).contains("vcr check:"));

    let missing_dependency = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .env("PATH", "")
        .args(["doctor"])
        .output()
        .expect("doctor command should run");
    assert_eq!(missing_dependency.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&missing_dependency.stderr).contains("vcr doctor:"));
}

#[test]
fn render_unsupported_software_layers_uses_a_stable_error_contract() {
    let dir = tempdir().expect("tempdir should create");
    fs::write(dir.path().join("shader.wgsl"), "// placeholder").expect("shader file should write");
    fs::write(dir.path().join("clip.mov"), "").expect("video file should write");
    fs::write(dir.path().join("anim.json"), "{}").expect("lottie file should write");

    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: shader_only
    shader:
      fragment: |
        fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
          return vec4<f32>(uv.x, uv.y, 0.0, 1.0);
        }
  - id: wgpu_only
    wgpu_shader:
      shader_path: ./shader.wgsl
      width: 16
      height: 16
      time_mode: seconds
  - id: video_only
    video:
      path: ./clip.mov
  - id: lottie_only
    lottie:
      path: ./anim.json
"#,
    );

    let manifest_arg = manifest_path.to_string_lossy().to_string();

    let human = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .args([
            "render",
            &manifest_arg,
            "-o",
            "out.mov",
            "--backend",
            "software",
        ])
        .output()
        .expect("command should run");
    assert_eq!(human.status.code(), Some(2));
    let human_stderr = String::from_utf8_lossy(&human.stderr);
    assert!(human_stderr.contains("vcr render: UNSUPPORTED_SOFTWARE_LAYER_TYPES:"));
    assert!(human_stderr.contains("software mode does not support these layer types"));
    assert!(human_stderr.contains("shader_only (shader)"));
    assert!(human_stderr.contains("wgpu_only (wgpu_shader)"));
    assert!(human_stderr.contains("video_only (video)"));
    assert!(human_stderr.contains("lottie_only (lottie)"));
    assert!(human_stderr.contains("re-run with `--backend gpu`"));

    let agent = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .env("VCR_AGENT_MODE", "1")
        .args([
            "render",
            &manifest_arg,
            "-o",
            "out.mov",
            "--backend",
            "software",
        ])
        .output()
        .expect("command should run");
    assert_eq!(agent.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&agent.stderr);
    let json_start = stderr
        .find('{')
        .expect("stderr should contain an envelope json object");
    let parsed: Value =
        serde_json::from_str(&stderr[json_start..]).expect("stderr should be envelope json");
    assert_eq!(parsed["ok"], Value::Bool(false));
    assert_eq!(
        parsed["error"]["code"],
        Value::String("UNSUPPORTED_SOFTWARE_LAYER_TYPES".to_owned())
    );
    assert!(
        parsed["error"]["message"]
            .as_str()
            .expect("message should be string")
            .contains("software mode does not support these layer types"),
        "expected direct explanation in error message"
    );
    assert_eq!(parsed["error"]["details"]["backend"], "software");
    assert_eq!(
        parsed["error"]["details"]["unsupported_layers"]
            .as_array()
            .expect("unsupported_layers should be an array")
            .len(),
        4
    );
    assert_eq!(
        parsed["error"]["details"]["supported_layer_types"]
            .as_array()
            .expect("supported_layer_types should be an array")
            .len(),
        6
    );
    assert_eq!(
        parsed["error"]["details"]["next_steps"][0],
        "re-run with --backend gpu"
    );
}

#[test]
fn ascii_capture_help_lists_expected_flags() {
    let dir = tempdir().expect("tempdir should create");
    let output = run_vcr(dir.path(), &["ascii", "capture", "--help"]);
    assert!(output.status.success(), "help should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--source"));
    assert!(stdout.contains("--out"));
    assert!(stdout.contains("--fps"));
    assert!(stdout.contains("--duration"));
    assert!(stdout.contains("--frames"));
    assert!(stdout.contains("--size"));
    assert!(stdout.contains("--font-path"));
    assert!(stdout.contains("--font-size"));
    assert!(stdout.contains("--tmp-dir"));
    assert!(stdout.contains("--symbol-remap"));
    assert!(stdout.contains("--symbol-ramp"));
    assert!(stdout.contains("--fit-padding"));
    assert!(stdout.contains("--aspect"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn ascii_capture_dry_run_prints_pipeline_plan() {
    let dir = tempdir().expect("tempdir should create");
    let output = run_vcr(
        dir.path(),
        &[
            "ascii",
            "capture",
            "--source",
            "ascii-live:earth",
            "--out",
            "custom_root",
            "--frames",
            "3",
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "dry-run should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Capture plan:"));
    assert!(stdout.contains("source: ascii-live:earth"));
    assert!(stdout.contains("source_command: curl -L --no-buffer https://ascii.live/earth"));
    assert!(stdout.contains("output_dir: custom_root/ascii_live_earth/1/cinema_30"));
    assert!(stdout.contains("frame_count: 3"));
    assert!(stdout.contains("aspect: cinema (1920x1080)"));
    assert!(stdout.contains("safe_area: left=96, right=96, top=54, bottom=54"));
    assert!(
        stdout.contains("encoder: ffmpeg -c:v prores_ks -profile:v standard -pix_fmt yuv422p10le")
    );
    assert!(stdout.contains("symbol_remap: Equalize"));
    assert!(stdout.contains("symbol_ramp: .,:;iltfrxnuvczXYUJCLQOZmwqpdbkhao*#MW&@$"));
    assert!(stdout.contains("fit_padding: 0.120"));
}

#[test]
fn ascii_capture_writes_output_mov_when_tools_are_available() {
    if !command_available("ffmpeg", "-version") || !command_available("chafa", "--version") {
        return;
    }

    let dir = tempdir().expect("tempdir should create");
    let input = dir.path().join("tiny_source.png");
    let source_image = RgbaImage::from_pixel(2, 2, Rgba([255, 255, 255, 255]));
    source_image.save(&input).expect("source image should save");
    let source = format!("chafa:{}", input.display());

    let output = run_vcr(
        dir.path(),
        &[
            "ascii",
            "capture",
            "--source",
            &source,
            "--out",
            "custom_root",
            "--frames",
            "3",
            "--fps",
            "24",
            "--size",
            "80x40",
            "--aspect",
            "cinema",
        ],
    );
    assert!(
        output.status.success(),
        "capture should succeed. stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mov_line = stdout
        .lines()
        .find(|line| line.starts_with("Wrote ") && line.ends_with(".mov"))
        .expect("capture should print output mov path");
    let mov_rel = mov_line.trim_start_matches("Wrote ").trim();
    let mov = dir.path().join(mov_rel);
    assert!(
        mov_rel.starts_with("custom_root/"),
        "mov path should be rooted under --out"
    );
    assert!(mov.is_file(), "capture output should exist");
    let metadata = fs::metadata(&mov).expect("capture output metadata should load");
    assert!(metadata.len() > 0, "capture output should not be empty");
    assert!(
        mov.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .contains("__cinema__24__"),
        "artifact filename should include aspect and fps"
    );
    let frame_hashes = mov
        .parent()
        .expect("mov should have parent")
        .join("frame_hashes.json");
    let artifact_manifest = mov
        .parent()
        .expect("mov should have parent")
        .join("artifact_manifest.json");
    assert!(frame_hashes.is_file(), "frame_hashes.json should exist");
    assert!(
        artifact_manifest.is_file(),
        "artifact_manifest.json should exist"
    );
}

#[test]
fn ascii_capture_invalid_aspect_emits_typed_error_envelope() {
    let dir = tempdir().expect("tempdir should create");
    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .env("VCR_AGENT_MODE", "1")
        .args([
            "ascii",
            "capture",
            "--source",
            "library:geist-wave",
            "--aspect",
            "square",
            "--frames",
            "1",
        ])
        .output()
        .expect("command should run");
    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: Value = serde_json::from_str(&stderr).expect("stderr should be envelope json");
    assert_eq!(parsed["ok"], Value::Bool(false));
    assert_eq!(
        parsed["error"]["code"],
        Value::String("INVALID_ASPECT_PRESET".to_owned())
    );
}

#[test]
fn preview_scale_keeps_the_whole_composition() {
    // Regression: `preview --scale` used to render layers at their original pixel coordinates
    // into a smaller canvas, i.e. a cropped top-left view instead of a scaled composition.
    let dir = tempdir().expect("tempdir");
    write_manifest(
        &dir.path().join("c.vcr"),
        "version: 1\nenvironment:\n  resolution: { width: 400, height: 200 }\n  fps: 10\n  duration: { frames: 2 }\nlayers:\n  - id: t\n    position: { x: 200, y: 100 }\n    anchor: center\n    text:\n      content: \"HELLO\"\n      font_size: 60\n",
    );
    let output = run_vcr(
        dir.path(),
        &[
            "--backend",
            "software",
            "preview",
            "c.vcr",
            "--scale",
            "0.5",
            "-o",
            "pv/",
            "--frames",
            "2",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let img = image::open(dir.path().join("pv/sample_000000.png"))
        .expect("sample frame")
        .to_rgba8();
    assert_eq!((img.width(), img.height()), (200, 100));
    // At full size the text spans x 108..291 (centred at 200). Scaled by 0.5 it must span about
    // 54..146: entirely inside the 200px preview, not cut off at x=200.
    let xs: Vec<u32> = img
        .enumerate_pixels()
        .filter(|(_, _, p)| p[3] > 0)
        .map(|(x, _, _)| x)
        .collect();
    let (min, max) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
    assert!(
        max < 160 && min > 40,
        "preview should be a scaled composition, got x {min}..{max}"
    );
}

#[test]
fn manifest_with_assets_validates_when_given_by_bare_filename() {
    // Regression: `Path::parent()` of "scene.vcr" is "", and canonicalizing "" failed, so any
    // manifest with an image/video/sequence asset broke when run from its own directory.
    let dir = tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("assets")).expect("assets dir");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
        .save(dir.path().join("assets/dot.png"))
        .expect("png should write");
    write_manifest(
        &dir.path().join("scene.vcr"),
        "version: 1\nenvironment:\n  resolution: { width: 64, height: 36 }\n  fps: 10\n  duration: { frames: 2 }\nlayers:\n  - id: pic\n    image:\n      path: assets/dot.png\n",
    );
    let output = run_vcr(dir.path(), &["check", "scene.vcr"]);
    assert!(
        output.status.success(),
        "check by bare filename should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
