use std::path::PathBuf;

use vcr::renderer::Renderer;
use vcr::schema::Manifest;
use vcr::timeline::RenderSceneData;

#[test]
fn wgpu_shader_layer_renders_non_empty_rgba() {
    let shader_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("shaders")
        .join("wgpu_shader_test.wgsl");
    let shader_path_yaml = shader_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");

    let manifest_yaml = format!(
        r#"
version: 1
environment:
  resolution: {{ width: 64, height: 64 }}
  fps: 30
  duration: {{ frames: 1 }}
layers:
  - id: smoke
    z_index: 0
    opacity: 1.0
    wgpu_shader:
      shader_path: "{shader_path_yaml}"
      width: 64
      height: 64
      time_mode: seconds
"#
    );

    let manifest: Manifest = serde_yaml::from_str(&manifest_yaml).expect("manifest should parse");
    let scene = RenderSceneData::from_manifest(&manifest);

    let renderer_result = pollster::block_on(Renderer::new_with_scene(
        &manifest.environment,
        &manifest.layers,
        scene,
    ));

    let mut renderer = match renderer_result {
        Ok(r) => r,
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("no suitable GPU adapter found") {
                eprintln!("Skipping test: no GPU adapter found");
                return;
            }
            panic!("renderer failed to initialize: {e:?}");
        }
    };

    let rgba = renderer
        .render_frame_rgba(0)
        .expect("render_frame_rgba should succeed");

    assert_eq!(rgba.len(), 64 * 64 * 4);
    assert!(
        rgba.chunks_exact(4).any(|px| px[3] > 0),
        "expected at least one non-transparent pixel"
    );
}
