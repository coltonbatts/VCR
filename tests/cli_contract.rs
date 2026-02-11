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
