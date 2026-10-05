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
    assert_eq!(
        result.frame_hash, expected_frame_hash,
        "Golden frame hash mismatch! The rendering core logic may have unexpectedly shifted."
    );

    // The raster hash above is toolchain-independent. The SHA-256 of the MOV on disk is not: it
    // is tied to `encoding::ffmpeg_args` AND to the ffmpeg build that muxes it (see
    // DETERMINISM_SPEC.md). So the file hash is asserted only for ffmpeg builds with a verified
    // hash; on any other build the test says so instead of failing on a hash nobody can reproduce.
    // To cover another build, run this test on it and add the printed (version, hash) pair.
    let ffmpeg_version = ffmpeg_version().unwrap_or_default();
    match GOLDEN_OUTPUT_HASHES
        .iter()
        .find(|(version, _)| *version == ffmpeg_version)
    {
        Some((_, expected)) => assert_eq!(
            result.output_hash, *expected,
            "Golden output hash mismatch for ffmpeg {ffmpeg_version}! The encoding pipeline may \
             have unexpectedly changed."
        ),
        None => eprintln!(
            "golden output hash NOT checked: no verified hash for ffmpeg {ffmpeg_version:?}; \
             observed {} (add it to GOLDEN_OUTPUT_HASHES if this build should be covered)",
            result.output_hash
        ),
    }
}

/// `(ffmpeg version token, SHA-256 of the golden MOV)`, each verified by rendering on that build.
const GOLDEN_OUTPUT_HASHES: &[(&str, &str)] = &[(
    // Ubuntu 24.04 `apt install ffmpeg`, which is what GitHub's ubuntu-latest runners install.
    "6.1.1-3ubuntu5",
    "ab54a3f8430da496a4b6f1e7a68b2580ca112ffa19b9d689c6ef96c592f73f6d",
)];

/// The version token from `ffmpeg -version` ("ffmpeg version 6.1.1-3ubuntu5 Copyright ..." yields
/// "6.1.1-3ubuntu5").
fn ffmpeg_version() -> Option<String> {
    let out = Command::new("ffmpeg").arg("-version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?;
    first
        .strip_prefix("ffmpeg version ")?
        .split_whitespace()
        .next()
        .map(str::to_owned)
}
