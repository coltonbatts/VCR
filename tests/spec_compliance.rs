use std::fs;
use std::path::Path;

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
fn spec_whole_string_substitution_only() {
    let dir = tempdir().expect("tempdir should create");
    let valid_manifest = dir.path().join("valid.vcr");
    write_manifest(
        &valid_manifest,
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
    opacity: "${speed}"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );
    load_with_sets(&valid_manifest, &[]).expect("whole-string token should resolve");

    let invalid_manifest = dir.path().join("invalid.vcr");
    write_manifest(
        &invalid_manifest,
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

    let error = load_with_sets(&invalid_manifest, &[]).expect_err("embedded token should fail");
    assert!(error.to_string().contains("invalid substitution string"));
}

#[test]
fn spec_escaped_token_and_dollar_prefixed_literals() {
    let dir = tempdir().expect("tempdir should create");
    let escaped_manifest = dir.path().join("escaped.vcr");
    write_manifest(
        &escaped_manifest,
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
  - id: escaped
    text:
      content: "$${speed}"
  - id: literal
    text:
      content: "$HOME/path/$DATA"
"#,
    );

    let manifest = load_with_sets(&escaped_manifest, &[]).expect("manifest should load");
    match &manifest.layers[0] {
        Layer::Text(layer) => assert_eq!(layer.text.content, "${speed}"),
        _ => panic!("expected text layer"),
    }
    match &manifest.layers[1] {
        Layer::Text(layer) => assert_eq!(layer.text.content, "$HOME/path/$DATA"),
        _ => panic!("expected text layer"),
    }
}

#[test]
fn spec_rejects_param_default_references_for_depth_one_resolution() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("recursive_defaults.vcr");
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
    default: 1.0
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let error =
        load_with_sets(&manifest_path, &[]).expect_err("param default references must fail");
    assert!(error.to_string().contains("default cannot reference"));
}

#[test]
fn spec_type_parsing_rules_match_docs() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("types.vcr");
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
  steps:
    type: int
    default: 2
  gate:
    type: bool
    default: false
  drift:
    type: vec2
    default: [0.0, 0.0]
  tint:
    type: color
    default: { r: 0.1, g: 0.2, b: 0.3, a: 1.0 }
layers:
  - id: bg
    opacity: "0.4 + speed * 0.0 + steps * 0.0 + gate * 0.0"
    position: "${drift}"
    procedural:
      kind: solid_color
      color: "${tint}"
"#,
    );

    let valid = load_with_sets(
        &manifest_path,
        &[
            "speed=1.25",
            "steps=3",
            "gate=1",
            "drift=-10.5, 2.25",
            "tint=#112233",
        ],
    )
    .expect("valid typed overrides should parse");
    assert_eq!(
        valid.resolved_params.get("speed"),
        Some(&ParamValue::Float(1.25))
    );
    assert_eq!(
        valid.resolved_params.get("steps"),
        Some(&ParamValue::Int(3))
    );
    assert_eq!(
        valid.resolved_params.get("gate"),
        Some(&ParamValue::Bool(true))
    );

    let nan_error = load_with_sets(&manifest_path, &["speed=NaN"]).expect_err("NaN should fail");
    assert!(nan_error.to_string().contains("expected float"));

    let int_error =
        load_with_sets(&manifest_path, &["steps=3.0"]).expect_err("decimal int should fail");
    assert!(int_error.to_string().contains("expected int"));

    let vec_space_error = load_with_sets(&manifest_path, &["drift=1 2"])
        .expect_err("space-delimited vec2 should fail");
    assert!(vec_space_error
        .to_string()
        .contains("whitespace-only separators are not supported"));

    let vec_semicolon_error =
        load_with_sets(&manifest_path, &["drift=1;2"]).expect_err("semicolon vec2 should fail");
    assert!(vec_semicolon_error
        .to_string()
        .contains("must use ',' as the delimiter"));

    let color_error =
        load_with_sets(&manifest_path, &["tint=#12GG33"]).expect_err("invalid color should fail");
    assert!(color_error.to_string().contains("expected color"));
}

#[test]
fn spec_override_ordering_does_not_change_hash() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("hash_ordering.vcr");
    write_manifest(
        &manifest_path,
        r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
params:
  enabled:
    type: bool
    default: true
  speed:
    type: float
    default: 1.0
layers:
  - id: bg
    opacity: "0.4 + enabled * 0.2 + speed * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    );

    let a = load_with_sets(&manifest_path, &["speed=1.7", "enabled=false"])
        .expect("first ordering should load");
    let b = load_with_sets(&manifest_path, &["enabled=false", "speed=1.7"])
        .expect("second ordering should load");

    assert_eq!(a.manifest_hash, b.manifest_hash);
}

#[test]
fn spec_manifest_hash_is_independent_of_absolute_manifest_location() {
    let dir_a = tempdir().expect("tempdir should create");
    let dir_b = tempdir().expect("tempdir should create");
    let manifest_a = dir_a.path().join("scene.vcr");
    let manifest_b = dir_b.path().join("scene.vcr");
    let content = r#"
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
    opacity: "0.4 + speed * 0.0"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#;
    write_manifest(&manifest_a, content);
    write_manifest(&manifest_b, content);

    let loaded_a = load_with_sets(&manifest_a, &["speed=1.5"]).expect("manifest a should load");
    let loaded_b = load_with_sets(&manifest_b, &["speed=1.5"]).expect("manifest b should load");

    assert_eq!(loaded_a.manifest_hash, loaded_b.manifest_hash);
}

#[test]
fn spec_rejects_duplicate_set_overrides() {
    let dir = tempdir().expect("tempdir should create");
    let manifest_path = dir.path().join("duplicates.vcr");
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
    opacity: "0.4 + speed * 0.0"
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
