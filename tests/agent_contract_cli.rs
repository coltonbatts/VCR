//! Contract tests for the agent-facing CLI surface (`--json`, contract `vcr.agent/1`).
//!
//! Every assertion parses the JSON document and checks structure and meaning. Nothing greps
//! human text.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const SMALL_ALPHA: &str = r#"version: 1
environment:
  resolution: { width: 64, height: 36 }
  fps: 10
  duration: { frames: 6 }
  encoding:
    prores_profile: prores4444
layers:
  - id: dot
    procedural:
      kind: circle
      center: { x: 0.5, y: 0.5 }
      radius: 0.2
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
"#;

fn vcr(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run vcr")
}

/// stdout must be exactly one JSON line; returns the parsed document.
fn doc(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim_end().lines().count(),
        1,
        "stdout must hold exactly one JSON line, got: {stdout}"
    );
    let value: Value = serde_json::from_str(stdout.trim()).expect("stdout parses as JSON");
    assert_eq!(value["contract"], "vcr.agent/1");
    assert!(
        value["engine"]["version"].is_string(),
        "engine identity present"
    );
    assert_eq!(value["ok"], value["status"] == "ok", "ok mirrors status");
    value
}

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

#[test]
fn check_success_reports_resolved_settings() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "a.vcr", SMALL_ALPHA);
    let out = vcr(dir.path(), &["check", "a.vcr", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let d = doc(&out);
    assert_eq!(d["operation"], "check");
    assert_eq!(d["status"], "ok");
    let env = &d["result"]["environment"];
    assert_eq!(env["width"], 64);
    assert_eq!(env["frames"], 6);
    assert_eq!(env["duration_rational"], "6/10");
    assert_eq!(env["alpha_capable_profile"], true);
    assert_eq!(d["result"]["layers"][0]["id"], "dot");
}

#[test]
fn schema_failure_has_stable_code_location_and_exit() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "bad.vcr",
        &SMALL_ALPHA.replace("  - id: dot", "  - id: dot\n    bogus_field: 1"),
    );
    let out = vcr(dir.path(), &["check", "bad.vcr", "--json"]);
    assert_eq!(out.status.code(), Some(3));
    let d = doc(&out);
    assert_eq!(d["status"], "error");
    let e = &d["error"];
    assert_eq!(e["code"], "manifest.schema_invalid");
    assert_eq!(e["category"], "manifest");
    assert_eq!(e["operation"], "check");
    assert_eq!(e["location"]["file"], "bad.vcr");
    assert_eq!(e["location"]["field"], "bogus_field");
    assert_eq!(e["retryable"], false);
    assert_eq!(e["exit_code"], 3);
    assert!(!e["recovery"].as_array().unwrap().is_empty());
}

#[test]
fn missing_asset_reports_layer_field_and_observed_path() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "img.vcr",
        &SMALL_ALPHA.replace(
            "    procedural:\n      kind: circle\n      center: { x: 0.5, y: 0.5 }\n      radius: 0.2\n      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }",
            "    image:\n      path: nope/missing.png",
        ),
    );
    let out = vcr(dir.path(), &["check", "img.vcr", "--json"]);
    let d = doc(&out);
    assert_eq!(d["error"]["code"], "asset.missing");
    assert_eq!(d["error"]["location"]["layer"], "dot");
    assert_eq!(d["error"]["location"]["field"], "image.path");
    assert_eq!(d["error"]["observed"], "nope/missing.png");
}

#[test]
fn io_failure_uses_exit_5_and_io_code() {
    let dir = TempDir::new().unwrap();
    let out = vcr(dir.path(), &["check", "absent.vcr", "--json"]);
    assert_eq!(out.status.code(), Some(5));
    assert_eq!(doc(&out)["error"]["code"], "io.read_failed");
}

#[test]
fn argument_errors_are_structured_when_json_requested() {
    let dir = TempDir::new().unwrap();
    let out = vcr(dir.path(), &["check", "--json"]); // missing <MANIFEST>
    assert_eq!(out.status.code(), Some(2));
    let d = doc(&out);
    assert_eq!(d["operation"], "check");
    assert_eq!(d["error"]["code"], "usage.invalid_argument");
    assert_eq!(d["error"]["category"], "usage");
    // without --json the human clap output is unchanged
    let human = vcr(dir.path(), &["check"]);
    assert_eq!(human.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&human.stderr).contains("required arguments"));
}

#[test]
fn bad_set_override_is_a_usage_error() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "a.vcr", SMALL_ALPHA);
    let out = vcr(
        dir.path(),
        &["check", "a.vcr", "--set", "nonsense", "--json"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(doc(&out)["error"]["code"], "usage.invalid_argument");
}

#[test]
fn prompt_blockers_stay_blockers_and_defaults_are_listed() {
    let dir = TempDir::new().unwrap();
    let blocked = vcr(
        dir.path(),
        &["prompt", "--json", "--text", "make a lower third"],
    );
    assert_eq!(
        blocked.status.code(),
        Some(0),
        "non-strict blocked exits 0 but says so"
    );
    let d = doc(&blocked);
    assert_eq!(d["status"], "blocked");
    assert_eq!(d["ok"], false);
    assert!(d["diagnostics"][0]["severity"] == "blocker");
    assert_eq!(
        d["result"]["creative_inputs"]["text_content"],
        "unspecified"
    );

    let strict = vcr(
        dir.path(),
        &[
            "prompt",
            "--json",
            "--strict",
            "--text",
            "make a lower third",
        ],
    );
    assert_eq!(strict.status.code(), Some(6));

    let ok = vcr(
        dir.path(),
        &[
            "prompt",
            "--json",
            "--text",
            "5s alpha lower third 1280x720 at 24fps",
        ],
    );
    let d = doc(&ok);
    assert_eq!(d["status"], "ok");
    let spec = &d["result"]["normalized_spec"];
    assert_eq!(spec["render"]["fps"], 24);
    assert_eq!(spec["render"]["frames"], 120);
    assert_eq!(spec["output"]["alpha"], true);
    let defaults: Vec<&str> = d["result"]["defaults_applied"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["field"].as_str().unwrap())
        .collect();
    assert!(defaults.contains(&"determinism.seed"));
    assert!(
        !defaults.contains(&"render.fps"),
        "fps was specified, not defaulted"
    );
}

#[test]
fn prompt_missing_fps_defaults_to_60() {
    let dir = TempDir::new().unwrap();
    let out = vcr(dir.path(), &["prompt", "--json", "--text", "3s title card"]);
    let d = doc(&out);
    assert_eq!(d["result"]["normalized_spec"]["render"]["fps"], 60);
    let fps = d["result"]["defaults_applied"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["field"] == "render.fps")
        .unwrap();
    assert_eq!(fps["value"], 60);
}

#[test]
fn lint_failure_is_a_result_with_layer_located_diagnostics() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "l.vcr",
        &format!("{SMALL_ALPHA}  - id: ghost\n    start_time: 10.0\n    procedural:\n      kind: solid_color\n      color: {{ r: 1.0, g: 1.0, b: 1.0, a: 1.0 }}\n"),
    );
    let out = vcr(dir.path(), &["lint", "l.vcr", "--json"]);
    assert_eq!(out.status.code(), Some(3));
    let d = doc(&out);
    assert_eq!(d["status"], "failed");
    assert!(
        d.get("error").is_none(),
        "a lint finding is a result, not an operation error"
    );
    let issue = &d["diagnostics"][0];
    assert_eq!(issue["code"], "lint.unreachable_layer");
    assert_eq!(issue["location"]["layer"], "ghost");
    assert_eq!(issue["stage"], "lint");
}

#[test]
fn doctor_json_has_runtime_probes() {
    let dir = TempDir::new().unwrap();
    let out = vcr(dir.path(), &["doctor", "--json"]);
    let d = doc(&out);
    assert_eq!(d["operation"], "doctor");
    let runtime = &d["result"]["runtime"];
    assert!(runtime["ffmpeg"]["available"].is_boolean());
    assert!(runtime["gpu"]["detail"].is_string());
    assert_eq!(runtime["software_backend_available"], true);
    if d["result"]["ready"] == false {
        assert_eq!(out.status.code(), Some(4), "missing dependency exit code");
    }
}

#[test]
fn legacy_agent_mode_errors_use_the_envelope_and_keep_old_keys() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "broken.vcr",
        "version: 1\nenvironment:\n  resolution: { width: 64, height: 36 }\n  fps: 10\nlayers:\n  - id: a\n    procedural: { kind: solid_color, color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 } }\n",
    );
    let out = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .args(["check", "broken.vcr"])
        .env("VCR_AGENT_MODE", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(
        out.stdout.is_empty(),
        "legacy agent mode keeps stdout clean"
    );
    let d: Value = serde_json::from_slice(&out.stderr).expect("stderr is one JSON document");
    // contract fields
    assert_eq!(d["contract"], "vcr.agent/1");
    assert_eq!(d["ok"], false);
    assert_eq!(d["error"]["category"], "manifest");
    assert_eq!(d["error"]["operation"], "check");
    // legacy AgentErrorReport keys still present at top level
    assert_eq!(d["error_type"], "validation");
    assert!(d["summary"].as_str().unwrap().contains("duration"));
    assert!(d["suggested_fix"]["description"]
        .as_str()
        .unwrap()
        .contains("duration"));

    // coded errors share the same envelope
    write(dir.path(), "a.vcr", SMALL_ALPHA);
    let coded = Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(dir.path())
        .args([
            "--backend",
            "software",
            "render-frame",
            "a.vcr",
            "--frame",
            "0",
            "--aspect",
            "bogus",
        ])
        .env("VCR_AGENT_MODE", "1")
        .output()
        .unwrap();
    // unknown flag => argument error, structured because agent mode is on
    let c: Value = serde_json::from_slice(&coded.stderr)
        .expect("argument errors are structured in agent mode");
    assert_eq!(c["error"]["code"], "usage.invalid_argument");
    assert_eq!(coded.status.code(), Some(2));
}
