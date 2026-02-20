use std::path::PathBuf;
use std::process::Command;

#[derive(serde::Deserialize)]
struct RenderJsonOutput {
    frame_hash: String,
    output_hash: String,
}

#[test]
fn golden_manifest_stability() {
    let manifest_path = PathBuf::from("tests/golden/minimal_manifest.yaml");
    let output_path = PathBuf::from("tests/golden/minimal_manifest_render.mov");

    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .arg("render")
        .arg(&manifest_path)
        .arg("-o")
        .arg(&output_path)
        .arg("--backend")
        .arg("software")
        .arg("--json")
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "VCR render failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The JSON should be the last line in stdout
    let json_str = stdout.lines().last().expect("No output from VCR");
    let result: RenderJsonOutput = serde_json::from_str(json_str).expect("Failed to parse JSON");

    let expected_frame_hash = "288b75c64f91afbdf3ea31803f526e8e0677b21cd4a5ede58246d4e5595f70a6";
    let expected_output_hash = "0a46ddb3e569448c59413b2c4c9153e133f6bed6aac296e677be0240731bea62";

    assert_eq!(
        result.frame_hash, expected_frame_hash,
        "Golden frame hash mismatch! The rendering core logic may have unexpectedly shifted."
    );
    assert_eq!(
        result.output_hash, expected_output_hash,
        "Golden output hash mismatch! The encoding pipeline may have unexpectedly changed."
    );
}
