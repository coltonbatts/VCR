use std::fs;
use vcr::manifest::load_and_validate_manifest;
use std::process::Command;

#[test]
fn test_absolute_asset_path_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_path = temp.path().join("test.vcr");
    let asset_path = "/etc/passwd";

    let yaml = format!(r#"
version: 1
environment:
  resolution: {{ width: 100, height: 100 }}
  fps: 24
  duration: 1.0
layers:
  - id: background
    image:
      path: {}
"#, asset_path);

    fs::write(&manifest_path, yaml).unwrap();

    let result = load_and_validate_manifest(&manifest_path);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("absolute paths are not allowed"));
}

#[test]
fn test_path_traversal_asset_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_dir = temp.path().join("manifests");
    fs::create_dir_all(&manifest_dir).unwrap();
    
    let secret_file = temp.path().join("secret.txt");
    fs::write(&secret_file, "secret").unwrap();
    
    let manifest_path = manifest_dir.join("test.vcr");
    let asset_path = "../secret.txt";

    let yaml = format!(r#"
version: 1
environment:
  resolution: {{ width: 100, height: 100 }}
  fps: 24
  duration: 1.0
layers:
  - id: background
    image:
      path: {}
"#, asset_path);

    fs::write(&manifest_path, yaml).unwrap();

    let result = load_and_validate_manifest(&manifest_path);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("escapes the manifest directory"));
}

#[test]
fn test_resource_limits_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_path = temp.path().join("test.vcr");

    let yaml = r#"
version: 1
environment:
  resolution: { width: 10000, height: 100 }
  fps: 24
  duration: 1.0
layers:
  - id: text
    text: { content: "too big" }
"#;

    fs::write(&manifest_path, yaml).unwrap();

    let result = load_and_validate_manifest(&manifest_path);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("resolution exceeds maximum allowed"));
}

#[test]
fn test_resource_limits_frames() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_path = temp.path().join("test.vcr");

    let yaml = r#"
version: 1
environment:
  resolution: { width: 100, height: 100 }
  fps: 60
  duration: 100000.0
layers:
  - id: text
    text: { content: "too long" }
"#;

    fs::write(&manifest_path, yaml).unwrap();

    let result = load_and_validate_manifest(&manifest_path);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(err.contains("frames exceeds maximum allowed"));
}

#[test]
fn test_cli_output_safety_absolute() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_path = temp.path().join("test.vcr");
    fs::write(&manifest_path, "version: 1\nenvironment: { resolution: { width: 10, height: 10 }, fps: 1, duration: 1 }\nlayers: [ { id: t, text: { content: x } } ]").unwrap();

    let output = Command::new("cargo")
        .arg("run")
        .arg("--bin")
        .arg("vcr")
        .arg("--")
        .arg("build")
        .arg(&manifest_path)
        .arg("-o")
        .arg("/tmp/evil.mov")
        .output()
        .expect("failed to execute process");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Absolute output paths are restricted for security"));
}

#[test]
fn test_cli_output_safety_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let manifest_path = temp.path().join("test.vcr");
    fs::write(&manifest_path, "version: 1\nenvironment: { resolution: { width: 10, height: 10 }, fps: 1, duration: 1 }\nlayers: [ { id: t, text: { content: x } } ]").unwrap();

    let output = Command::new("cargo")
        .arg("run")
        .arg("--bin")
        .arg("vcr")
        .arg("--")
        .arg("build")
        .arg(&manifest_path)
        .arg("-o")
        .arg("../forbidden.mov")
        .output()
        .expect("failed to execute process");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Directory traversal in output path is not allowed"));
}
