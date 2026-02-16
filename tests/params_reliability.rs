use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;
use vcr::manifest::{load_and_validate_manifest_with_options, ManifestLoadOptions, ParamOverride};
use vcr::schema::{Layer, ParamValue};

fn write_manifest(path: &Path, yaml: &str) {
    fs::write(path, yaml).expect("manifest should write");
}

fn load_with_sets(path: &Path, sets: &[&str]) -> anyhow::Result<vcr::schema::Manifest> {
    let overrides = sets
        .iter()
        .map(|raw| ParamOverride::parse(raw))
        .collect::<anyhow::Result<Vec<_>>>()?;
    load_and_validate_manifest_with_options(
        path,
        &ManifestLoadOptions {
            overrides,
            allow_raw_paths: false,
        },
    )
}

#[test]
fn bool_override_parsing_accepts_true_false_and_01() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  gate:
    type: bool
    default: false
layers:
  - id: bg
    opacity: "0.5 + gate * 0.2"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let true_manifest = load_with_sets(&manifest_path, &["gate=true"]).expect("true should parse");
    assert_eq!(
        true_manifest.resolved_params.get("gate"),
        Some(&ParamValue::Bool(true))
    );

    let one_manifest = load_with_sets(&manifest_path, &["gate=1"]).expect("1 should parse");
    assert_eq!(
        one_manifest.resolved_params.get("gate"),
        Some(&ParamValue::Bool(true))
    );

    let false_manifest =
        load_with_sets(&manifest_path, &["gate=false"]).expect("false should parse");
    assert_eq!(
        false_manifest.resolved_params.get("gate"),
        Some(&ParamValue::Bool(false))
    );

    let zero_manifest = load_with_sets(&manifest_path, &["gate=0"]).expect("0 should parse");
    assert_eq!(
        zero_manifest.resolved_params.get("gate"),
        Some(&ParamValue::Bool(false))
    );
}

#[test]
fn float_override_rejects_nan_and_inf() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  speed:
    type: float
    default: 1.0
layers:
  - id: bg
    opacity: "0.5 + speed * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let nan_error = load_with_sets(&manifest_path, &["speed=NaN"]).expect_err("NaN should fail");
    let nan_message = nan_error.to_string();
    assert!(nan_message.contains("param 'speed'"));
    assert!(nan_message.contains("expected float"));
    assert!(nan_message.contains("got 'NaN'"));
    assert!(nan_message.contains("--set speed=1.25"));

    let inf_error = load_with_sets(&manifest_path, &["speed=inf"]).expect_err("inf should fail");
    assert!(inf_error.to_string().contains("finite"));
}

#[test]
fn int_override_is_strict_and_enforces_bounds() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  steps:
    type: int
    default: 3
    min: 1
    max: 5
layers:
  - id: bg
    opacity: "0.5 + steps * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let valid = load_with_sets(&manifest_path, &["steps=4"]).expect("integer should parse");
    assert_eq!(
        valid.resolved_params.get("steps"),
        Some(&ParamValue::Int(4))
    );

    let decimal_error =
        load_with_sets(&manifest_path, &["steps=3.0"]).expect_err("3.0 should fail");
    assert!(decimal_error.to_string().contains("expected int"));

    let bounds_error = load_with_sets(&manifest_path, &["steps=0"]).expect_err("0 should fail");
    assert!(bounds_error.to_string().contains("below min"));
}

#[test]
fn color_override_parsing_accepts_hex_and_numeric_and_rejects_invalid() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  tint:
    type: color
    default: { r: 0.1, g: 0.2, b: 0.3, a: 1.0 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: "${tint}"
"#,
    );

    let hex = load_with_sets(&manifest_path, &["tint=#112233"]).expect("hex color should parse");
    match hex.resolved_params.get("tint") {
        Some(ParamValue::Color(color)) => {
            assert!((color.r - (17.0 / 255.0)).abs() < 0.0001);
            assert!((color.g - (34.0 / 255.0)).abs() < 0.0001);
            assert!((color.b - (51.0 / 255.0)).abs() < 0.0001);
            assert!((color.a - 1.0).abs() < 0.0001);
        }
        _ => panic!("expected color value"),
    }

    let numeric = load_with_sets(&manifest_path, &["tint=0.1, 0.2, 0.3, 0.4"])
        .expect("numeric color should parse");
    assert!(matches!(
        numeric.resolved_params.get("tint"),
        Some(ParamValue::Color(_))
    ));

    let invalid_hex =
        load_with_sets(&manifest_path, &["tint=#12GG33"]).expect_err("invalid hex should fail");
    assert!(invalid_hex.to_string().contains("expected color"));

    let invalid_fn =
        load_with_sets(&manifest_path, &["tint=rgb(1,2,3)"]).expect_err("rgb() should fail");
    assert!(invalid_fn.to_string().contains("expected color"));
}

#[test]
fn vec2_override_parsing_handles_whitespace_and_negatives() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  offset:
    type: vec2
    default: [0.0, 0.0]
layers:
  - id: bg
    position: "${offset}"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let parsed =
        load_with_sets(&manifest_path, &["offset=-10.5,  2.25"]).expect("vec2 should parse");
    match parsed.resolved_params.get("offset") {
        Some(ParamValue::Vec2(vec)) => {
            assert!((vec.x + 10.5).abs() < 0.0001);
            assert!((vec.y - 2.25).abs() < 0.0001);
        }
        _ => panic!("expected vec2"),
    }

    let invalid =
        load_with_sets(&manifest_path, &["offset=1 2"]).expect_err("invalid vec2 should fail");
    assert!(invalid.to_string().contains("expected vec2"));
}

#[test]
fn missing_param_reference_is_a_hard_error() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
layers:
  - id: bg
    start_time: "${missing}"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let error = load_with_sets(&manifest_path, &[]).expect_err("missing reference should fail");
    let message = error.to_string();
    assert!(message.contains("unknown parameter reference '${missing}'"));
    assert!(message.contains("$${missing}"));
}

#[test]
fn substitution_rejects_ambiguous_embedded_tokens() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
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

    let error = load_with_sets(&manifest_path, &[]).expect_err("embedded token should fail");
    assert!(error.to_string().contains("invalid substitution string"));
}

#[test]
fn escaped_substitution_token_is_preserved_as_literal() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  speed:
    type: float
    default: 1.0
layers:
  - id: t
    text:
      content: "$${speed}"
"#,
    );

    let manifest = load_with_sets(&manifest_path, &[]).expect("escaped token should load");
    match manifest.layers.first() {
        Some(Layer::Text(text_layer)) => assert_eq!(text_layer.text.content, "${speed}"),
        _ => panic!("expected text layer"),
    }
}

#[test]
fn recursive_param_defaults_are_rejected() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  a:
    type: float
    default: "${b}"
  b:
    type: float
    default: "${a}"
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let error = load_with_sets(&manifest_path, &[]).expect_err("recursive defaults should fail");
    assert!(error.to_string().contains("default cannot reference"));
}

#[test]
fn duplicate_set_overrides_are_rejected() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  speed:
    type: float
    default: 1.0
layers:
  - id: bg
    opacity: "0.5 + speed * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let error = load_and_validate_manifest_with_options(
        &manifest_path,
        &ManifestLoadOptions {
            overrides: vec![
                ParamOverride::parse("speed=1.0").expect("first override should parse"),
                ParamOverride::parse("speed=2.0").expect("second override should parse"),
            ],
            allow_raw_paths: false,
        },
    )
    .expect_err("duplicate overrides should fail");

    assert!(error
        .to_string()
        .contains("duplicate --set override for param 'speed'"));
}

#[test]
fn metadata_is_deterministic_and_hash_depends_on_effective_inputs() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("scene.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  speed:
    type: float
    default: 1.0
  enabled:
    type: bool
    default: true
layers:
  - id: bg
    opacity: "0.4 + enabled * 0.2 + speed * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "a.png",
            "--set",
            "speed=1.5",
            "--set",
            "enabled=true",
        ],
    );
    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "b.png",
            "--set",
            "speed=1.5",
            "--set",
            "enabled=true",
        ],
    );

    let metadata_a =
        fs::read(dir.path().join("a.png.metadata.json")).expect("metadata a should exist");
    let metadata_b =
        fs::read(dir.path().join("b.png.metadata.json")).expect("metadata b should exist");
    assert_eq!(
        metadata_a, metadata_b,
        "metadata must be byte-identical for same effective inputs"
    );

    let parsed_a: Value = serde_json::from_slice(&metadata_a).expect("metadata a should parse");
    assert!(
        parsed_a.get("manifest_path").is_none(),
        "metadata must not contain machine-specific manifest path"
    );

    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "c.png",
            "--set",
            "speed=1.0",
            "--set",
            "enabled=true",
        ],
    );
    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "d.png",
            "--set",
            "speed=2.0",
            "--set",
            "enabled=true",
        ],
    );

    let parsed_c: Value = serde_json::from_slice(
        &fs::read(dir.path().join("c.png.metadata.json")).expect("metadata c should exist"),
    )
    .expect("metadata c should parse");
    let parsed_d: Value = serde_json::from_slice(
        &fs::read(dir.path().join("d.png.metadata.json")).expect("metadata d should exist"),
    )
    .expect("metadata d should parse");
    assert_ne!(
        parsed_c["manifest_hash"], parsed_d["manifest_hash"],
        "changing effective overrides must change metadata hash"
    );

    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "e.png",
            "--set",
            "speed=1.7",
            "--set",
            "enabled=false",
        ],
    );
    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "scene.vcr",
            "--frame",
            "0",
            "-o",
            "f.png",
            "--set",
            "enabled=false",
            "--set",
            "speed=1.7",
        ],
    );

    let parsed_e: Value = serde_json::from_slice(
        &fs::read(dir.path().join("e.png.metadata.json")).expect("metadata e should exist"),
    )
    .expect("metadata e should parse");
    let parsed_f: Value = serde_json::from_slice(
        &fs::read(dir.path().join("f.png.metadata.json")).expect("metadata f should exist"),
    )
    .expect("metadata f should parse");

    assert_eq!(
        parsed_e["manifest_hash"], parsed_f["manifest_hash"],
        "override ordering must not affect hash"
    );
}

fn run_vcr(cwd: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("vcr command should run");

    if !output.status.success() {
        panic!(
            "vcr command failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
