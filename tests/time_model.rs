use std::fs;
use std::path::Path;

use tempfile::tempdir;
use vcr::manifest::load_and_validate_manifest;
use vcr::renderer::Renderer;
use vcr::schema::Manifest;
use vcr::timeline::{evaluate_manifest_layers_at_frame, RenderSceneData};

/// Exercises every time-dependent path: expressions on transform/opacity, a seconds-based
/// keyframe mapping, a procedural color expression, env() with seconds defaults, and layer
/// timing controls (start window + time_offset).
const TIME_SCENE: &str = r#"
version: VERSION
environment:
  resolution: { width: 64, height: 48 }
  fps: FPS
  duration: 2.0
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: "0.5 + 0.5 * sin(t * 3)", g: 0.1, b: "clamp(t / 2, 0, 1)", a: 1 }
  - id: mover
    pos_x: "4 + t * 12"
    pos_y: 6
    rotation_degrees: { start_time: 0.25, end_time: 1.5, from: 0, to: 90, easing: ease_in_out }
    opacity: "0.25 + 0.75 * env(t)"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
    scale: [0.25, 0.25]
  - id: late
    start_time: 0.5
    time_offset: -0.5
    pos_x: 30
    pos_y: "10 + smoothstep(0, 1, t) * 20"
    scale: [0.2, 0.2]
    procedural:
      kind: solid_color
      color: { r: 0.1, g: "clamp(t, 0, 1)", b: 0.9, a: 0.8 }
"#;

fn load_scene(dir: &Path, version: u32, fps: u32) -> Manifest {
    let path = dir.join(format!("scene_v{version}_{fps}.vcr"));
    let yaml = TIME_SCENE
        .replace("VERSION", &version.to_string())
        .replace("FPS", &fps.to_string());
    fs::write(&path, yaml).expect("write manifest");
    load_and_validate_manifest(&path).expect("manifest should load")
}

fn render(manifest: &Manifest, frame: u32) -> Vec<u8> {
    let mut renderer = Renderer::new_software(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(manifest),
    )
    .expect("software renderer");
    renderer.render_frame_rgba(frame).expect("render frame")
}

fn max_channel_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// (seconds, frame at 24fps, frame at 60fps)
const MATCHING_TIMESTAMPS: [(f32, u32, u32); 6] = [
    (0.0, 0, 0),
    (0.25, 6, 15),
    (0.5, 12, 30),
    (1.0, 24, 60),
    (1.25, 30, 75),
    (1.5, 36, 90),
];

#[test]
fn v2_scene_agrees_across_fps_at_matching_timestamps() {
    let dir = tempdir().expect("tempdir");
    let at_24 = load_scene(dir.path(), 2, 24);
    let at_60 = load_scene(dir.path(), 2, 60);
    assert_eq!(at_24.environment.total_frames(), 48);
    assert_eq!(at_60.environment.total_frames(), 120);

    for (seconds, frame_24, frame_60) in MATCHING_TIMESTAMPS {
        let states_24 = evaluate_manifest_layers_at_frame(&at_24, frame_24).expect("eval 24");
        let states_60 = evaluate_manifest_layers_at_frame(&at_60, frame_60).expect("eval 60");
        for (a, b) in states_24.iter().zip(&states_60) {
            assert_eq!(a.visible, b.visible, "{} visibility at {seconds}s", a.id);
            let deltas = [
                a.position.x - b.position.x,
                a.position.y - b.position.y,
                a.rotation_degrees - b.rotation_degrees,
                a.opacity - b.opacity,
            ];
            assert!(
                deltas.iter().all(|delta| delta.abs() < 1e-3),
                "layer '{}' diverges at {seconds}s between 24fps and 60fps: {deltas:?}",
                a.id
            );
        }

        // Pixels are quantized to 8 bits, so allow a single code value of float noise.
        let diff = max_channel_diff(&render(&at_24, frame_24), &render(&at_60, frame_60));
        assert!(
            diff <= 1,
            "v2 frame at {seconds}s differs between 24fps and 60fps by {diff} code values"
        );
    }
}

#[test]
fn v1_scene_keeps_legacy_frame_time_and_diverges_across_fps() {
    // Documents the legacy behaviour that version 2 fixes: in version 1 `t` counts frames,
    // so the same manifest animates 2.5x faster in wall-clock time at 60fps than at 24fps.
    let dir = tempdir().expect("tempdir");
    let at_24 = load_scene(dir.path(), 1, 24);
    let at_60 = load_scene(dir.path(), 1, 60);

    let a = evaluate_manifest_layers_at_frame(&at_24, 24).expect("eval 24");
    let b = evaluate_manifest_layers_at_frame(&at_60, 60).expect("eval 60");
    let mover_24 = a.iter().find(|s| s.id == "mover").expect("mover");
    let mover_60 = b.iter().find(|s| s.id == "mover").expect("mover");
    assert_eq!(mover_24.position.x, 4.0 + 24.0 * 12.0);
    assert_eq!(mover_60.position.x, 4.0 + 60.0 * 12.0);
}

#[test]
fn v2_expressions_expose_frame_and_fps() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("builtins.vcr");
    fs::write(
        &path,
        r#"
version: 2
environment:
  resolution: { width: 16, height: 16 }
  fps: 30
  duration: { frames: 90 }
layers:
  - id: probe
    pos_x: "frame"
    pos_y: "fps"
    rotation_degrees: "t"
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    )
    .expect("write manifest");
    let manifest = load_and_validate_manifest(&path).expect("load");
    let states = evaluate_manifest_layers_at_frame(&manifest, 45).expect("eval");
    assert_eq!(states[0].position.x, 45.0);
    assert_eq!(states[0].position.y, 30.0);
    assert!((states[0].rotation_degrees - 1.5).abs() < 1e-6);
}

#[test]
fn v2_rejects_params_shadowing_time_builtins() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("shadow.vcr");
    fs::write(
        &path,
        r#"
version: 2
environment:
  resolution: { width: 16, height: 16 }
  fps: 24
  duration: 1.0
params:
  fps: 12
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1, g: 1, b: 1, a: 1 }
"#,
    )
    .expect("write manifest");
    let error = load_and_validate_manifest(&path).expect_err("fps param must be rejected in v2");
    assert!(
        format!("{error:#}").contains("param name 'fps' is reserved"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn v2_procedural_sources_evaluate_in_layer_local_time() {
    // time_offset shifts the layer clock; in version 2 procedural colors follow it.
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("local.vcr");
    fs::write(
        &path,
        r#"
version: 2
environment:
  resolution: { width: 4, height: 4 }
  fps: 10
  duration: 2.0
layers:
  - id: fade
    time_offset: -1.0
    procedural:
      kind: solid_color
      color: { r: "clamp(t, 0, 1)", g: 0, b: 0, a: 1 }
"#,
    )
    .expect("write manifest");
    let manifest = load_and_validate_manifest(&path).expect("load");
    // Global 1.0s == local 0.0s -> red channel 0.
    assert_eq!(render(&manifest, 10)[0], 0);
    // Global 2.0s would be past the clip; global 1.5s == local 0.5s -> ~half red.
    let red = render(&manifest, 15)[0];
    assert!((120..=136).contains(&red), "expected ~half red, got {red}");
}
