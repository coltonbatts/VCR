//! GPU vs software backend parity for the canonical color pipeline
//! (docs/COLOR_PIPELINE.md).
//!
//! Tolerance: every channel of every pixel must agree within `TOLERANCE` code values after
//! weighting color by alpha (`rgb * a / 255`, alpha compared directly). The weighting keeps
//! near-transparent pixels, whose straight RGB is `premultiplied / alpha` and therefore
//! ill-conditioned, from dominating. The remaining budget covers:
//! - float16 accumulation on the GPU vs float32 on the CPU (< 0.5 code), and
//! - GPU texture-filter sub-texel precision next to high-contrast texel edges under
//!   bilinear scaling/rotation (observed up to 3 codes).
//!
//! Skips (passes with a note) when no GPU adapter is available. Set `VCR_REQUIRE_GPU=1` to
//! turn a missing adapter into a failure on machines that are expected to have one.

use std::fs;
use std::path::Path;

use tempfile::tempdir;
use vcr::manifest::load_and_validate_manifest;
use vcr::renderer::{Renderer, RendererGpuContext};
use vcr::schema::Manifest;
use vcr::timeline::RenderSceneData;

const TOLERANCE: u8 = 3;

const PARITY_SCENE: &str = r##"
version: 2
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: 1.0
layers:
  - id: bg
    z_index: 0
    procedural:
      kind: gradient
      start_color: { r: 0.05, g: 0.08, b: 0.20, a: 1.0 }
      end_color: { r: 0.90, g: 0.55, b: 0.10, a: 1.0 }
      direction: horizontal
  - id: veil
    z_index: 1
    opacity: 0.6
    procedural:
      kind: gradient
      start_color: { r: 1.0, g: 1.0, b: 1.0, a: 0.0 }
      end_color: { r: 0.1, g: 0.9, b: 0.4, a: 0.8 }
      direction: vertical
  - id: circle
    z_index: 2
    opacity: { keyframes: [ { time: 0, value: 0.3 }, { time: 1, value: 0.9, easing: [0.4, 0, 0.2, 1] } ] }
    procedural:
      kind: circle
      center: [0.25, 0.35]
      radius: 0.11
      color: { r: 0.95, g: 0.2, b: 0.35, a: 0.75 }
  - id: ring
    z_index: 3
    procedural:
      kind: ring
      center: [0.72, 0.30]
      outer_radius: 0.09
      inner_radius: 0.05
      color: { r: 0.2, g: 0.8, b: 1.0, a: 1.0 }
  - id: panel
    z_index: 4
    opacity: 0.85
    procedural:
      kind: rounded_rect
      center: [0.5, 0.78]
      size: [0.5, 0.22]
      corner_radius: 0.03
      color: { r: 0.02, g: 0.02, b: 0.03, a: 0.7 }
  - id: slash
    z_index: 5
    procedural:
      kind: line
      start: [0.08, 0.92]
      end: [0.45, 0.55]
      thickness: 0.012
      color: { r: 1.0, g: 0.95, b: 0.4, a: 0.9 }
  - id: hexagon
    z_index: 6
    procedural:
      kind: polygon
      center: [0.88, 0.72]
      radius: 0.07
      sides: 6
      color: { r: 0.6, g: 0.3, b: 0.9, a: 1.0 }
  - id: spinner
    z_index: 7
    anchor: center
    position: [160, 90]
    rotation_degrees: { keyframes: [ { time: 0, value: 0 }, { time: 1, value: 37 } ] }
    scale: [0.8, 0.8]
    opacity: 0.7
    procedural:
      kind: triangle
      p0: [0.45, 0.25]
      p1: [0.60, 0.60]
      p2: [0.35, 0.65]
      color: { r: 0.1, g: 1.0, b: 0.7, a: 1.0 }
  - id: sprite_scaled
    z_index: 8
    position: [12.5, 20.25]
    scale: [1.75, 1.5]
    image:
      path: "sprite.png"
  - id: sprite_rotated
    z_index: 9
    anchor: center
    position: [250, 120]
    rotation_degrees: -23
    scale: [1.2, 1.2]
    opacity: 0.8
    image:
      path: "sprite.png"
  - id: label
    z_index: 10
    position: [100, 150]
    opacity: 0.9
    text:
      content: "VCR 0.5"
      font_size: 18
      color: { r: 1.0, g: 0.9, b: 0.8, a: 1.0 }
  - id: grid
    z_index: 11
    position: [8, 150]
    opacity: 0.8
    ascii:
      grid: { rows: 2, columns: 6 }
      cell: { width: 6, height: 10 }
      font_variant: geist_pixel_regular
      foreground: { r: 0.3, g: 1.0, b: 0.5, a: 1.0 }
      background: { r: 0.0, g: 0.0, b: 0.0, a: 0.4 }
      inline:
        - "#@*+.:"
        - "=-%&$ "
"##;

/// 32x24 straight-alpha sprite with a soft alpha ramp, hard color steps, and a fully
/// transparent 2px border (so rotated quad edges fall on transparent texels).
fn write_sprite(path: &Path) {
    let (width, height) = (32_u32, 24_u32);
    let mut image = image::RgbaImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let border = x < 2 || y < 2 || x >= width - 2 || y >= height - 2;
        let alpha = if border {
            0
        } else {
            (60 + (x * 195) / (width - 1)) as u8
        };
        let red = if (x / 4 + y / 4) % 2 == 0 { 250 } else { 40 };
        *pixel = image::Rgba([red, (y * 10) as u8, 255 - (x * 7) as u8, alpha]);
    }
    image.save(path).expect("write sprite png");
}

fn load_scene(dir: &Path) -> Manifest {
    write_sprite(&dir.join("sprite.png"));
    let path = dir.join("parity.vcr");
    fs::write(&path, PARITY_SCENE).expect("write manifest");
    load_and_validate_manifest(&path).expect("parity manifest should load")
}

fn gpu_available() -> bool {
    match pollster::block_on(RendererGpuContext::headless()) {
        Ok(_) => true,
        Err(error) => {
            if std::env::var_os("VCR_REQUIRE_GPU").is_some() {
                panic!("VCR_REQUIRE_GPU is set but no GPU adapter is available: {error:#}");
            }
            eprintln!("skipping GPU parity test: {error:#}");
            false
        }
    }
}

/// Largest alpha-weighted per-channel difference and where it happened.
fn weighted_max_diff(software: &[u8], gpu: &[u8], width: usize) -> (u8, (usize, usize)) {
    let weight = |pixel: &[u8], channel: usize| -> f32 {
        if channel == 3 {
            pixel[3] as f32
        } else {
            pixel[channel] as f32 * pixel[3] as f32 / 255.0
        }
    };
    let mut worst = (0_u8, (0, 0));
    for (index, (a, b)) in software
        .chunks_exact(4)
        .zip(gpu.chunks_exact(4))
        .enumerate()
    {
        for channel in 0..4 {
            let diff = (weight(a, channel) - weight(b, channel)).abs().round() as u8;
            if diff > worst.0 {
                worst = (diff, (index % width, index / width));
            }
        }
    }
    worst
}

#[test]
fn gpu_and_software_backends_agree_within_tolerance() {
    if !gpu_available() {
        return;
    }
    let dir = tempdir().expect("tempdir");
    let manifest = load_scene(dir.path());
    let width = manifest.environment.resolution.width as usize;

    let mut software = Renderer::new_software(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(&manifest),
    )
    .expect("software renderer");
    let mut gpu = pollster::block_on(Renderer::new_with_scene(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(&manifest),
    ))
    .expect("gpu renderer");
    assert!(
        gpu.is_gpu_backend(),
        "adapter found but GPU backend not used"
    );
    assert!(software.warnings().is_empty() && gpu.warnings().is_empty());

    for frame in [0_u32, 7, 12, 23] {
        let cpu_frame = software.render_frame_rgba(frame).expect("software frame");
        let gpu_frame = gpu.render_frame_rgba(frame).expect("gpu frame");
        assert_eq!(cpu_frame.len(), gpu_frame.len());
        let (diff, (x, y)) = weighted_max_diff(&cpu_frame, &gpu_frame, width);
        eprintln!("frame {frame}: max alpha-weighted channel diff {diff} at ({x}, {y})");
        assert!(
            diff <= TOLERANCE,
            "frame {frame}: backends differ by {diff} code values at ({x}, {y}) (tolerance {TOLERANCE})"
        );
    }
}

#[test]
fn gpu_backend_is_deterministic_across_renderer_instances() {
    if !gpu_available() {
        return;
    }
    let dir = tempdir().expect("tempdir");
    let manifest = load_scene(dir.path());
    let render = |frame: u32| {
        let mut renderer = pollster::block_on(Renderer::new_with_scene(
            &manifest.environment,
            &manifest.layers,
            RenderSceneData::from_manifest(&manifest),
        ))
        .expect("gpu renderer");
        renderer.render_frame_rgba(frame).expect("gpu frame")
    };
    assert_eq!(render(12), render(12), "GPU output must be bit-identical");
}

#[test]
fn software_backend_flags_shader_layers_it_cannot_render() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("shader.vcr");
    fs::write(
        &path,
        r#"
version: 2
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: 1.0
layers:
  - id: glow
    shader:
      fragment: |
        fn shade(uv: vec2<f32>, uniforms: ShaderUniforms) -> vec4<f32> {
          return vec4<f32>(uv.x, uv.y, 0.0, 1.0);
        }
"#,
    )
    .expect("write manifest");
    let manifest = load_and_validate_manifest(&path).expect("load");
    let renderer = Renderer::new_software(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(&manifest),
    )
    .expect("software renderer");
    assert_eq!(renderer.warnings().len(), 1);
    assert!(renderer.warnings()[0].contains("layer 'glow'"));
}
