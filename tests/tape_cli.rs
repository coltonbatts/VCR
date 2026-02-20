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

fn parse_run_record_path(stdout: &str) -> PathBuf {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("Run record: "))
        .map(PathBuf::from)
        .expect("run output should include run record path")
}

#[test]
fn tape_init_and_list_work_with_config_override() {
    let dir = tempdir().expect("tempdir should create");
    let config_path = dir.path().join("tapes.yaml");

    let init = run_vcr(
        dir.path(),
        &["tape", "--config", config_path.to_str().unwrap(), "init"],
    );
    assert!(
        init.status.success(),
        "tape init should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(config_path.exists(), "tape init should write config file");

    let list = run_vcr(
        dir.path(),
        &["tape", "--config", config_path.to_str().unwrap(), "list"],
    );
    assert!(
        list.status.success(),
        "tape list should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );

    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list_stdout.contains("alpha-lower-third"));
    assert!(list_stdout.contains("frame-poster"));
}

#[test]
fn tape_new_appends_and_rejects_duplicate_id() {
    let dir = tempdir().expect("tempdir should create");
    let config_path = dir.path().join("tapes.yaml");

    let init = run_vcr(
        dir.path(),
        &["tape", "--config", config_path.to_str().unwrap(), "init"],
    );
    assert!(init.status.success(), "init should succeed");

    let add = run_vcr(
        dir.path(),
        &[
            "tape",
            "--config",
            config_path.to_str().unwrap(),
            "new",
            "custom-id",
        ],
    );
    assert!(
        add.status.success(),
        "new should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&add.stdout),
        String::from_utf8_lossy(&add.stderr)
    );

    let add_duplicate = run_vcr(
        dir.path(),
        &[
            "tape",
            "--config",
            config_path.to_str().unwrap(),
            "new",
            "custom-id",
        ],
    );
    assert!(
        !add_duplicate.status.success(),
        "duplicate tape id should fail"
    );

    let config_contents = fs::read_to_string(&config_path).expect("config should read");
    assert!(config_contents.contains("id: custom-id"));
}

#[test]
fn tape_run_dry_run_writes_record_and_can_emit_json() {
    let dir = tempdir().expect("tempdir should create");
    let config_path = dir.path().join("tapes.yaml");

    let init = run_vcr(
        dir.path(),
        &["tape", "--config", config_path.to_str().unwrap(), "init"],
    );
    assert!(init.status.success(), "init should succeed");

    let run = run_vcr(
        dir.path(),
        &[
            "tape",
            "--config",
            config_path.to_str().unwrap(),
            "run",
            "alpha-lower-third",
            "--dry-run",
            "--json",
        ],
    );
    assert!(
        run.status.success(),
        "tape run dry-run should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let stdout = String::from_utf8_lossy(&run.stdout);
    let record_path = parse_run_record_path(&stdout);
    assert!(record_path.exists(), "run record should be written");

    let json_start = stdout.find('{').expect("expected json output at end");
    let parsed: Value = serde_json::from_str(&stdout[json_start..]).expect("json should parse");
    assert_eq!(parsed["dry_run"], Value::Bool(true));

    let record_json: Value =
        serde_json::from_str(&fs::read_to_string(&record_path).expect("record json should read"))
            .expect("record json should parse");
    assert_eq!(record_json["action"], Value::String("primary".to_owned()));
    assert_eq!(record_json["dry_run"], Value::Bool(true));
}
