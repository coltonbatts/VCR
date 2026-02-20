use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

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

fn first_diff_offset(a: &[u8], b: &[u8]) -> Option<usize> {
    let min_len = a.len().min(b.len());
    for idx in 0..min_len {
        if a[idx] != b[idx] {
            return Some(idx);
        }
    }
    if a.len() != b.len() {
        return Some(min_len);
    }
    None
}

fn assert_files_identical(path_a: &Path, path_b: &Path) {
    let a = fs::read(path_a).expect("left file should read");
    let b = fs::read(path_b).expect("right file should read");
    if let Some(offset) = first_diff_offset(&a, &b) {
        panic!(
            "files differ at byte offset {offset}: {} vs {}",
            path_a.display(),
            path_b.display()
        );
    }
}

fn run_pack_compile(cwd: &Path, aspect: &str) -> PathBuf {
    let output = run_vcr(
        cwd,
        &[
            "pack",
            "compile",
            "--pack",
            "docu_pack_v1",
            "--fields",
            r#"{"title":"DOCU","subtitle":"SCENE"}"#,
            "--aspect",
            aspect,
            "--fps",
            "24",
            "--out",
            "out",
            "--backend",
            "software",
        ],
    );

    assert!(
        output.status.success(),
        "pack compile should succeed for aspect={aspect}. stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mov_line = stdout
        .lines()
        .find(|line| line.starts_with("Wrote ") && line.ends_with(".mov"))
        .expect("pack compile should print mov output path");
    let mov_rel = mov_line.trim_start_matches("Wrote ").trim();
    cwd.join(mov_rel)
}

#[test]
fn pack_compile_help_lists_compile_and_flags() {
    let dir = tempdir().expect("tempdir should create");

    let pack_help = run_vcr(dir.path(), &["pack", "--help"]);
    assert!(pack_help.status.success(), "pack --help should succeed");
    let pack_stdout = String::from_utf8_lossy(&pack_help.stdout);
    assert!(pack_stdout.contains("compile"));

    let compile_help = run_vcr(dir.path(), &["pack", "compile", "--help"]);
    assert!(
        compile_help.status.success(),
        "pack compile --help should succeed"
    );
    let compile_stdout = String::from_utf8_lossy(&compile_help.stdout);
    assert!(compile_stdout.contains("--pack"));
    assert!(compile_stdout.contains("--fields"));
    assert!(compile_stdout.contains("--aspect"));
    assert!(compile_stdout.contains("--fps"));
    assert!(compile_stdout.contains("--out"));
}

#[test]
fn tape_compile_help_exists_and_lists_compile_flags() {
    let dir = tempdir().expect("tempdir should create");

    let tape_help = run_vcr(dir.path(), &["tape", "--help"]);
    assert!(tape_help.status.success(), "tape --help should succeed");
    let tape_stdout = String::from_utf8_lossy(&tape_help.stdout);
    assert!(tape_stdout.contains("compile"));
    assert!(tape_stdout.contains("recommended for repeatable render runs"));

    let compile_help = run_vcr(dir.path(), &["tape", "compile", "--help"]);
    assert!(
        compile_help.status.success(),
        "tape compile --help should succeed"
    );
    let compile_stdout = String::from_utf8_lossy(&compile_help.stdout);
    assert!(compile_stdout.contains("--pack"));
    assert!(compile_stdout.contains("--fields"));
    assert!(compile_stdout.contains("--aspect"));
    assert!(compile_stdout.contains("--fps"));
    assert!(compile_stdout.contains("--out"));
}

#[test]
fn pack_compile_invalid_pack_returns_typed_agent_error() {
    let dir = tempdir().expect("tempdir should create");
    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .env("VCR_AGENT_MODE", "1")
        .args([
            "pack",
            "compile",
            "--pack",
            "unknown_pack",
            "--fields",
            r#"{"title":"DOCU","subtitle":"SCENE"}"#,
            "--aspect",
            "cinema",
            "--fps",
            "24",
            "--out",
            "out",
        ])
        .output()
        .expect("command should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: Value = serde_json::from_str(&stderr).expect("stderr should be envelope json");
    assert_eq!(parsed["ok"], Value::Bool(false));
    assert_eq!(
        parsed["error"]["code"],
        Value::String("INVALID_PACK".to_owned())
    );
}

#[test]
fn tape_compile_invalid_pack_returns_typed_agent_error() {
    let dir = tempdir().expect("tempdir should create");
    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .env("VCR_AGENT_MODE", "1")
        .args([
            "tape",
            "compile",
            "--pack",
            "unknown_pack",
            "--fields",
            r#"{"title":"DOCU","subtitle":"SCENE"}"#,
            "--aspect",
            "cinema",
            "--fps",
            "24",
            "--out",
            "out",
        ])
        .output()
        .expect("command should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: Value = serde_json::from_str(&stderr).expect("stderr should be envelope json");
    assert_eq!(parsed["ok"], Value::Bool(false));
    assert_eq!(
        parsed["error"]["code"],
        Value::String("INVALID_PACK".to_owned())
    );
}

#[test]
fn pack_compile_missing_required_field_returns_typed_agent_error() {
    let dir = tempdir().expect("tempdir should create");
    let output = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .env("VCR_AGENT_MODE", "1")
        .args([
            "pack",
            "compile",
            "--pack",
            "docu_pack_v1",
            "--fields",
            r#"{"title":"DOCU"}"#,
            "--aspect",
            "cinema",
            "--fps",
            "24",
            "--out",
            "out",
        ])
        .output()
        .expect("command should run");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: Value = serde_json::from_str(&stderr).expect("stderr should be envelope json");
    assert_eq!(parsed["ok"], Value::Bool(false));
    assert_eq!(
        parsed["error"]["code"],
        Value::String("MISSING_REQUIRED_FIELD".to_owned())
    );
}

#[test]
fn pack_compile_is_byte_deterministic_for_all_aspect_presets() {
    if !command_available("ffmpeg", "-version") {
        return;
    }

    for aspect in ["cinema", "social", "phone"] {
        let run_a = tempdir().expect("tempdir A");
        let run_b = tempdir().expect("tempdir B");

        let mov_a = run_pack_compile(run_a.path(), aspect);
        let mov_b = run_pack_compile(run_b.path(), aspect);
        let dir_a = mov_a.parent().expect("mov A parent");
        let dir_b = mov_b.parent().expect("mov B parent");

        assert_files_identical(&mov_a, &mov_b);
        assert_files_identical(
            &dir_a.join("artifact_manifest.json"),
            &dir_b.join("artifact_manifest.json"),
        );
        assert_files_identical(
            &dir_a.join("frame_hashes.json"),
            &dir_b.join("frame_hashes.json"),
        );
    }
}
