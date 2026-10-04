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

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn need_ffmpeg() -> bool {
    if have("ffmpeg") && have("ffprobe") {
        true
    } else {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        false
    }
}

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
    assert_eq!(d["result"]["layers"][0]["kind"], "procedural");
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

#[test]
fn preflight_flags_software_incompatibility_post_and_font_fallback() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "p.vcr",
        r#"version: 1
environment:
  resolution: { width: 64, height: 36 }
  fps: 10
  duration: { frames: 2 }
layers:
  - id: t
    text:
      content: "HI"
      font_family: "Inter"
      font_size: 20
post:
  - shader: sobel
    strength: 1.0
"#,
    );
    let out = vcr(
        dir.path(),
        &["--backend", "software", "explain", "--json", "p.vcr"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "explain describes; it does not fail by default"
    );
    let d = doc(&out);
    // legacy keys preserved
    assert!(d["backend_preflight"]["software_compatible"] == false);
    assert!(d["manifest_hash"].is_string());
    let b = &d["result"]["backend_preflight"];
    assert_eq!(b["ready"], false);
    assert_eq!(b["resolved_backend"], "software");
    let codes: Vec<&str> = b["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"backend.software_ignores_feature"));
    assert!(codes.contains(&"text.font_family_fallback"));
    let font = b["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["code"] == "text.font_family_fallback")
        .unwrap();
    assert_eq!(font["severity"], "warning");
    assert_eq!(font["location"]["layer"], "t");
    assert_eq!(font["observed"], "Inter");

    // --strict turns a not-ready preflight into a failed result
    let strict = vcr(
        dir.path(),
        &[
            "--backend",
            "software",
            "explain",
            "--json",
            "--strict",
            "p.vcr",
        ],
    );
    assert_eq!(strict.status.code(), Some(3));
    assert_eq!(doc(&strict)["status"], "failed");

    // and the render itself refuses, with the same facts
    if need_ffmpeg() {
        let render = vcr(
            dir.path(),
            &[
                "--backend",
                "software",
                "render",
                "--json",
                "p.vcr",
                "-o",
                "p.mov",
            ],
        );
        assert_eq!(render.status.code(), Some(2));
        let r = doc(&render);
        assert_eq!(r["error"]["code"], "UNSUPPORTED_SOFTWARE_FEATURES");
        assert_eq!(r["error"]["category"], "usage");
        assert!(
            !dir.path().join("p.mov").exists(),
            "no output may be published"
        );
    }
}

#[test]
fn requesting_gpu_without_adapter_is_a_preflight_blocker() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "a.vcr", SMALL_ALPHA);
    let d = doc(&vcr(
        dir.path(),
        &["--backend", "gpu", "explain", "--json", "a.vcr"],
    ));
    let b = &d["result"]["backend_preflight"];
    if b["gpu_available"] == false {
        assert!(b["resolved_backend"].is_null());
        assert_eq!(b["ready"], false);
        assert_eq!(b["checks"][0]["code"], "backend.gpu_unavailable");
    }
}

const CLIP_SCENE: &str = r#"version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 10
  duration: { frames: 20 }
params:
  x_offset:
    type: float
    default: 0.0
    min: -400.0
    max: 400.0
layers:
  - id: title
    pos_x: "40 + x_offset"
    pos_y: 60
    text:
      content: "CLIPPED TITLE TEXT"
      font_size: 24
"#;

#[test]
fn capabilities_are_derived_and_distinguish_compiled_from_usable() {
    let dir = TempDir::new().unwrap();
    let out = vcr(dir.path(), &["capabilities", "--json", "--schema"]);
    assert_eq!(out.status.code(), Some(0));
    let d = doc(&out);
    let r = &d["result"];
    assert_eq!(r["manifest"]["supported_versions"][0], 1);
    // layers: derived list, with backend requirements
    let layers = r["layers"].as_array().unwrap();
    let shader = layers.iter().find(|l| l["kind"] == "shader").unwrap();
    assert_eq!(shader["requires_gpu"], true);
    assert_eq!(shader["compiled_backends"], serde_json::json!(["gpu"]));
    let text = layers.iter().find(|l| l["kind"] == "text").unwrap();
    assert_eq!(text["usable_here"], true);
    // schema is real and strict
    let schema = &r["manifest"]["schema"];
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["properties"]["layers"].is_object());
    // time units are explicit
    assert!(r["time"]["expression_t"]
        .as_str()
        .unwrap()
        .contains("frame"));
    assert!(r["time"]["layer_start_time_end_time"]
        .as_str()
        .unwrap()
        .contains("seconds"));
    assert!(r["expression"]["functions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "smoothstep"));
    assert!(r["encoding"]["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p == "prores4444"));
    assert_eq!(r["runtime"]["software_backend_available"], true);
    // exit-code table is part of the contract
    assert!(d["result"]["contract"]["exit_codes"]["6"].is_string());
    // commands list says which support --json
    let cmds = r["cli"]["commands"].as_array().unwrap();
    for name in [
        "check",
        "lint",
        "doctor",
        "prompt",
        "verify",
        "inspect",
        "render",
        "capabilities",
    ] {
        let c = cmds
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("{name}"));
        if name != "capabilities" {
            assert_eq!(c["json"], true, "{name} supports --json");
        }
    }
}

#[test]
fn inspect_detects_clipping_and_a_targeted_param_revision_fixes_it() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "clip.vcr", CLIP_SCENE);
    let run = |extra: &[&str], out: &str| -> Value {
        let mut args = vec![
            "--backend",
            "software",
            "inspect",
            "clip.vcr",
            "--json",
            "-o",
            out,
            "--samples",
            "4",
        ];
        args.extend_from_slice(extra);
        doc(&vcr(dir.path(), &args))
    };
    let clipped = run(&["--set", "x_offset=120"], "ins_bad");
    let d = clipped["diagnostics"].as_array().unwrap();
    let hit = d
        .iter()
        .find(|x| x["code"] == "layout.touches_canvas_edge")
        .expect("clipping detected");
    assert_eq!(hit["location"]["layer"], "title");
    assert_eq!(hit["severity"], "warning");
    assert_eq!(hit["basis"], "exact");
    assert!(hit["message"].as_str().unwrap().contains("right"));
    assert!(clipped["result"]["revise_with"]
        .as_str()
        .unwrap()
        .contains("--set"));

    // Revise one declared parameter; nothing else is rebuilt.
    let fixed = run(&["--set", "x_offset=0"], "ins_ok");
    assert!(
        !fixed["diagnostics"].as_array().map_or(false, |a| a
            .iter()
            .any(|x| x["code"] == "layout.touches_canvas_edge")),
        "{:?}",
        fixed["diagnostics"]
    );

    // Evidence: frame indices and PNG names agree; contact sheet exists; first and last sampled.
    let samples = fixed["result"]["samples"].as_array().unwrap();
    assert_eq!(samples.first().unwrap()["frame"], 0);
    assert_eq!(samples.last().unwrap()["frame"], 19);
    for s in samples {
        let frame = s["frame"].as_u64().unwrap();
        let rel = s["preview"].as_str().unwrap();
        assert!(rel.ends_with(&format!("sample_{frame:06}.png")), "{rel}");
        assert!(dir.path().join(rel).exists());
        assert_eq!(s["time_rational"], format!("{frame}/10"));
    }
    let sheet = fixed["result"]["contact_sheet"]["path"].as_str().unwrap();
    assert!(dir.path().join(sheet).exists());
    let roles: Vec<&str> = fixed["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["role"].as_str().unwrap())
        .collect();
    assert!(
        roles.contains(&"contact_sheet")
            && roles.contains(&"preview_frame")
            && roles.contains(&"inspection")
    );
}

const MISTIMED: &str = r#"version: 1
environment:
  resolution: { width: 160, height: 90 }
  fps: 10
  duration: { frames: 30 }
params:
  exit_start:
    type: float
    default: 20.0
    min: 1.0
    max: 100.0
layers:
  - id: box
    opacity: "1.0 - clamp((t - exit_start) / 20.0, 0.0, 1.0)"
    procedural:
      kind: rounded_rect
      center: { x: 0.5, y: 0.5 }
      size: { x: 0.4, y: 0.3 }
      corner_radius: 0.0
      color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
"#;

#[test]
fn inspect_detects_a_cut_off_exit_and_timing_param_fixes_it() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "m.vcr", MISTIMED);
    let codes = |extra: &[&str], out: &str| -> Vec<String> {
        let mut args = vec![
            "--backend",
            "software",
            "inspect",
            "m.vcr",
            "--json",
            "-o",
            out,
        ];
        args.extend_from_slice(extra);
        let d = doc(&vcr(dir.path(), &args));
        d["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["code"].as_str().unwrap().to_owned())
            .collect()
    };
    // exit begins at frame 20 and needs 20 frames, but the clip ends at frame 29: still moving.
    assert!(codes(&[], "a").contains(&"timing.motion_on_last_frame".to_owned()));
    assert!(
        !codes(&["--set", "exit_start=5"], "b").contains(&"timing.motion_on_last_frame".to_owned())
    );
}
