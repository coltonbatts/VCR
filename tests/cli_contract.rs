use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
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
    assert!(stdout.contains("encoder: ffmpeg -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le"));
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
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/welcome_terminal_scene.gif");
    assert!(input.exists(), "fixture gif should exist");
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
