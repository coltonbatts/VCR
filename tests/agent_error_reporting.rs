use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

#[test]
fn test_agent_mode_validation_error_json() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "missing_duration.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let output = run_agent("check", &manifest_path, &[]);
    assert_eq!(output.status.code(), Some(3));

    let json = parse_stderr_json(&output);
    assert_required_contract_fields(&json);
    assert_eq!(json["error_type"], "validation");
    assert!(json["summary"]
        .as_str()
        .unwrap()
        .contains("missing field `duration`"));
    assert!(!json["summary"]
        .as_str()
        .unwrap()
        .contains(manifest_path.to_string_lossy().as_ref()));

    let suggested_fix = json
        .get("suggested_fix")
        .and_then(Value::as_object)
        .expect("validation payload should include suggested_fix");
    assert!(suggested_fix
        .get("description")
        .and_then(Value::as_str)
        .unwrap()
        .contains("duration"));
    let actions = suggested_fix
        .get("actions")
        .and_then(Value::as_array)
        .expect("missing duration fix should include actions");
    assert_eq!(actions[0]["type"], "add_field");
    assert!(actions[0]["example_value"].is_number());

    let issue = first_validation_error(&json);
    assert_eq!(issue["path"], "environment.duration");
    assert!(issue["message"]
        .as_str()
        .unwrap()
        .contains("missing field `duration`"));
}

#[test]
fn test_agent_mode_unknown_field_error_json() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "unknown_field.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
  duration: 1.0
params:
  speed:
    type: float
    default: 1.0
    typo: 2.0
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let output = run_agent("check", &manifest_path, &[]);
    assert_eq!(output.status.code(), Some(3));

    let json = parse_stderr_json(&output);
    assert_required_contract_fields(&json);
    assert_eq!(json["error_type"], "validation");
    assert!(json["summary"]
        .as_str()
        .unwrap()
        .contains("unknown field 'typo'"));

    let issue = first_validation_error(&json);
    assert_eq!(issue["path"], "params.speed.typo");
    assert!(!issue["message"].as_str().unwrap().is_empty());
}

#[test]
fn test_agent_mode_wrong_type_error_json() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "wrong_type.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
  duration: 1.0
params:
  speed:
    type: float
    default: fast
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let output = run_agent("check", &manifest_path, &[]);
    assert_eq!(output.status.code(), Some(3));

    let json = parse_stderr_json(&output);
    assert_required_contract_fields(&json);
    assert_eq!(json["error_type"], "validation");
    assert!(json["summary"]
        .as_str()
        .unwrap()
        .contains("param 'speed'.default must be a number"));

    let issue = first_validation_error(&json);
    assert_eq!(issue["path"], "params.speed.default");
    assert!(!issue["message"].as_str().unwrap().is_empty());
}

#[test]
fn test_agent_mode_missing_required_field_error_json() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "missing_required_field.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
  duration: 1.0
params:
  speed:
    type: float
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let output = run_agent("check", &manifest_path, &[]);
    assert_eq!(output.status.code(), Some(3));

    let json = parse_stderr_json(&output);
    assert_required_contract_fields(&json);
    assert_eq!(json["error_type"], "validation");
    assert!(json["summary"]
        .as_str()
        .unwrap()
        .contains("param 'speed' must define 'default'"));

    let issue = first_validation_error(&json);
    assert_eq!(issue["path"], "params.speed.default");
    assert!(!issue["message"].as_str().unwrap().is_empty());
}

#[test]
fn test_agent_mode_lint_error_json_contract() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "unreachable.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
  duration: 1.0
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
  - id: unreachable
    start_time: 10.0
    text:
      content: NEVER VISIBLE
"#,
    );

    let output = run_agent("lint", &manifest_path, &[]);
    assert_eq!(output.status.code(), Some(3));

    let json = parse_stderr_json(&output);
    assert_required_contract_fields(&json);
    assert_eq!(json["error_type"], "lint");
    assert!(json["summary"].as_str().unwrap().contains("unreachable"));
    assert!(json.get("validation_errors").is_none());
}

#[test]
fn test_normal_mode_stays_human_readable() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "human_mode.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let output = Command::new(vcr_binary())
        .arg("check")
        .arg(&manifest_path)
        .output()
        .expect("failed to execute vcr");
    assert_eq!(output.status.code(), Some(3));

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("vcr check:"));
    assert!(!stderr.contains("\"error_type\""));
}

#[test]
fn test_agent_mode_json_is_deterministic_for_same_input() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "deterministic_missing_duration.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let first = parse_stderr_json(&run_agent("check", &manifest_path, &[]));
    let second = parse_stderr_json(&run_agent("check", &manifest_path, &[]));
    assert_eq!(first, second);
}

#[test]
fn test_agent_mode_usage_fix_action_uses_typed_json_example_value() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_manifest(
        temp_dir.path(),
        "param_override_type_error.vcr",
        r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
  duration: 1.0
params:
  speed:
    type: float
    default: 1.0
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#,
    );

    let output = run_agent("check", &manifest_path, &["--set", "speed=fast"]);
    assert_eq!(output.status.code(), Some(2));

    let json = parse_stderr_json(&output);
    assert_required_contract_fields(&json);
    assert_eq!(json["error_type"], "usage");

    let suggested_fix = json
        .get("suggested_fix")
        .and_then(Value::as_object)
        .expect("usage payload should include suggested_fix for typed override errors");
    let actions = suggested_fix
        .get("actions")
        .and_then(Value::as_array)
        .expect("usage fix should include actions");
    assert_eq!(actions[0]["type"], "set_field");
    assert_eq!(actions[0]["path"], "params.speed");
    assert_eq!(actions[0]["expected_type"], "float");
    assert!(actions[0]["example_value"].is_number());
}

fn run_agent(command: &str, manifest_path: &Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(vcr_binary());
    cmd.arg(command)
        .arg(manifest_path)
        .env("VCR_AGENT_MODE", "1");
    for arg in extra_args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to execute vcr")
}

fn parse_stderr_json(output: &Output) -> Value {
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr should be utf-8");
    serde_json::from_str(&stderr).unwrap_or_else(|error| {
        panic!("stderr was not valid json: {error}; stderr={stderr}");
    })
}

fn assert_required_contract_fields(json: &Value) {
    let error_type = json
        .get("error_type")
        .and_then(Value::as_str)
        .expect("error_type must exist and be a string");
    assert!(!error_type.is_empty());

    let summary = json
        .get("summary")
        .and_then(Value::as_str)
        .expect("summary must exist and be a string");
    assert!(!summary.is_empty());
}

fn first_validation_error<'a>(json: &'a Value) -> &'a Value {
    let issues = json["validation_errors"]
        .as_array()
        .expect("validation_errors must exist for schema validation failures");
    issues
        .first()
        .expect("validation_errors must include at least one item")
}

fn write_manifest(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn vcr_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_vcr"))
}
