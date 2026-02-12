use std::process::Command;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_agent_mode_validation_error_json() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = temp_dir.path().join("broken.vcr");
    
    // Create an intentionally broken manifest (missing duration)
    let broken_manifest = r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#;
    
    fs::write(&manifest_path, broken_manifest).unwrap();
    
    // Run vcr check with VCR_AGENT_MODE=1
    let output = Command::new(vcr_binary())
        .arg("check")
        .arg(&manifest_path)
        .env("VCR_AGENT_MODE", "1")
        .output()
        .expect("failed to execute vcr");
    
    assert!(!output.status.success());
    
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // Verify JSON structure
    assert!(stderr.contains("\"error_type\""), "Should contain error_type field");
    assert!(stderr.contains("\"validation\""), "Should be a validation error");
    assert!(stderr.contains("\"summary\""), "Should contain summary field");
    assert!(stderr.contains("\"suggested_fix\""), "Should contain suggested_fix");
    assert!(stderr.contains("duration"), "Should mention missing duration field");
}

#[test]
fn test_agent_mode_lint_error_with_fix() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = temp_dir.path().join("unreachable.vcr");
    
    // Create a manifest with an unreachable layer
    let manifest = r#"
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
    start_time: 10.0  # Beyond duration
    text:
      content: "NEVER VISIBLE"
"#;
    
    fs::write(&manifest_path, manifest).unwrap();
    
    // Run vcr lint with VCR_AGENT_MODE=1
    let output = Command::new(vcr_binary())
        .arg("lint")
        .arg(&manifest_path)
        .env("VCR_AGENT_MODE", "1")
        .output()
        .expect("failed to execute vcr");
    
    assert!(!output.status.success());
    
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // Verify lint error structure
    assert!(stderr.contains("\"error_type\": \"lint\""), "Should be a lint error");
    assert!(stderr.contains("unreachable"), "Should mention unreachable layer");
    assert!(stderr.contains("\"suggested_fix\""), "Should provide suggested fix");
    assert!(stderr.contains("start_time") || stderr.contains("opacity"), "Fix should mention timing or opacity");
}

#[test]
fn test_normal_mode_human_readable_error() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = temp_dir.path().join("broken.vcr");
    
    // Create an intentionally broken manifest
    let broken_manifest = r#"
version: 1
environment:
  resolution: {width: 1920, height: 1080}
  fps: 30
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}
"#;
    
    fs::write(&manifest_path, broken_manifest).unwrap();
    
    // Run vcr check WITHOUT VCR_AGENT_MODE
    let output = Command::new(vcr_binary())
        .arg("check")
        .arg(&manifest_path)
        .output()
        .expect("failed to execute vcr");
    
    assert!(!output.status.success());
    
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // Verify human-readable output (NOT JSON)
    assert!(stderr.starts_with("vcr "), "Should start with 'vcr' command name");
    assert!(!stderr.contains("\"error_type\""), "Should NOT be JSON in normal mode");
    assert!(!stderr.contains("\"suggested_fix\""), "Should NOT contain JSON fields");
}

#[test]
fn test_agent_mode_param_override_error() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = temp_dir.path().join("params.vcr");
    
    // Create a manifest with typed parameters
    let manifest = r#"
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
"#;
    
    fs::write(&manifest_path, manifest).unwrap();
    
    // Run with invalid parameter override
    let output = Command::new(vcr_binary())
        .arg("check")
        .arg(&manifest_path)
        .arg("--set")
        .arg("speed=fast")  // Should be a number
        .env("VCR_AGENT_MODE", "1")
        .output()
        .expect("failed to execute vcr");
    
    assert!(!output.status.success());
    
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    // Verify parameter error includes type information
    assert!(stderr.contains("\"error_type\": \"usage\""), "Should be a usage error");
    assert!(stderr.contains("speed") || stderr.contains("param"), "Should mention parameter");
}

/// Helper to locate the vcr binary
fn vcr_binary() -> PathBuf {
    // Try debug build first
    let debug = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/debug/vcr");
    if debug.exists() {
        return debug;
    }
    
    // Try release build
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/release/vcr");
    if release.exists() {
        return release;
    }
    
    // Just use "vcr" and hope it's on PATH
    PathBuf::from("vcr")
}
