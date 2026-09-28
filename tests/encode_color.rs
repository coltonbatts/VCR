//! ProRes encode correctness: explicit BT.709 matrix and stream tags, and known sRGB colors
//! surviving RGBA -> Y'CbCr -> RGBA. Skips when ffmpeg/ffprobe are not installed.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

/// Per-channel tolerance after a 10-bit limited-range 4:4:4 round trip (quantization is
/// well under one 8-bit code; two codes leaves room for decoder rounding differences).
const TOLERANCE: u8 = 2;

/// Six 16x16 swatches in a 96x32 frame; row 2 repeats row 1 at 50% alpha.
const SWATCHES: [(f32, f32, f32); 6] = [
    (0.80, 0.30, 0.10),
    (0.50, 0.50, 0.50),
    (0.03, 0.03, 0.03),
    (0.97, 0.97, 0.97),
    (0.10, 0.20, 0.90),
    (0.20, 0.85, 0.30),
];

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn write_swatch_manifest(dir: &Path) {
    let mut layers = String::new();
    for (index, (r, g, b)) in SWATCHES.iter().enumerate() {
        for (row, alpha) in [(0, 1.0), (1, 0.5)] {
            layers.push_str(&format!(
                r#"  - id: swatch_{index}_{row}
    procedural:
      kind: rounded_rect
      center: [{cx}, {cy}]
      size: [{w}, 0.5]
      corner_radius: 0
      color: {{ r: {r}, g: {g}, b: {b}, a: {alpha} }}
"#,
                cx = (index as f32 + 0.5) / 6.0,
                cy = 0.25 + 0.5 * row as f32,
                w = 1.0 / 6.0,
            ));
        }
    }
    fs::write(
        dir.join("swatches.vcr"),
        format!(
            "version: 2\nenvironment:\n  resolution: {{ width: 96, height: 32 }}\n  fps: 24\n  duration: {{ frames: 2 }}\nlayers:\n{layers}"
        ),
    )
    .expect("write manifest");
}

fn run_vcr(dir: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run vcr");
    assert!(
        output.status.success(),
        "vcr {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn decode_rgba(input: &Path, filter: Option<&str>) -> Vec<u8> {
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-i"]).arg(input);
    command.args(["-frames:v", "1"]);
    if let Some(filter) = filter {
        command.args(["-vf", filter]);
    }
    command.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
    let output = command.output().expect("run ffmpeg");
    assert!(
        output.status.success(),
        "ffmpeg decode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn max_channel_diff(a: &[u8], b: &[u8]) -> (u8, usize) {
    assert_eq!(a.len(), b.len(), "decoded frame size mismatch");
    a.iter()
        .zip(b)
        .enumerate()
        .map(|(index, (x, y))| (x.abs_diff(*y), index / 4))
        .max()
        .unwrap_or((0, 0))
}

#[test]
fn prores_encode_is_bt709_tagged_and_round_trips_srgb_colors() {
    if !tool_available("ffmpeg") || !tool_available("ffprobe") {
        eprintln!("skipping encode color test: ffmpeg/ffprobe not found on PATH");
        return;
    }
    let dir = tempdir().expect("tempdir");
    write_swatch_manifest(dir.path());
    run_vcr(
        dir.path(),
        &["--quiet", "build", "swatches.vcr", "-o", "out.mov"],
    );
    run_vcr(
        dir.path(),
        &[
            "--quiet",
            "render-frame",
            "swatches.vcr",
            "--frame",
            "0",
            "-o",
            "reference.png",
        ],
    );

    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,profile,color_range,color_space,color_transfer,color_primaries",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(dir.path().join("out.mov"))
        .output()
        .expect("run ffprobe");
    let probe = String::from_utf8_lossy(&probe.stdout);
    for expected in [
        "codec_name=prores",
        "profile=4444",
        "color_primaries=bt709",
        "color_transfer=bt709",
        "color_space=bt709",
        "color_range=tv",
    ] {
        assert!(
            probe.lines().any(|line| line == expected),
            "missing `{expected}` in ffprobe output:\n{probe}"
        );
    }

    let reference = decode_rgba(&dir.path().join("reference.png"), None);
    let movie = dir.path().join("out.mov");

    // Decode as a BT.709-aware player would. Before explicit tagging, VCR converted with
    // FFmpeg's BT.601 default, which this check catches (red swatch was off by ~12 codes).
    let explicit = decode_rgba(
        &movie,
        Some("scale=in_color_matrix=bt709:in_range=tv,format=rgba"),
    );
    let (diff, pixel) = max_channel_diff(&reference, &explicit);
    assert!(
        diff <= TOLERANCE,
        "BT.709 decode differs from the rendered PNG by {diff} codes at pixel {pixel}"
    );

    // Decode trusting the stream tags (FFmpeg default behaviour).
    let tagged = decode_rgba(&movie, None);
    let (diff, pixel) = max_channel_diff(&reference, &tagged);
    assert!(
        diff <= TOLERANCE,
        "tag-driven decode differs from the rendered PNG by {diff} codes at pixel {pixel}"
    );

    // Negative control: interpreting the stream with the wrong matrix must be detectable,
    // otherwise the checks above prove nothing.
    let wrong = decode_rgba(
        &movie,
        Some("scale=in_color_matrix=bt601:in_range=tv,format=rgba"),
    );
    let (diff, _) = max_channel_diff(&reference, &wrong);
    assert!(
        diff > TOLERANCE,
        "BT.601 decode should not match; the swatches cannot distinguish matrices"
    );
}
