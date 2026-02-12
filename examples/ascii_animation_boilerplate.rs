use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use image::RgbaImage;
use vcr::animation_engine::{
    AnimationImportOptions, AnimationLayer, AnimationManager, AsciiCellMetrics, BoutiqueFilter,
    FitOptions, PlaybackOptions, DEFAULT_ANIMATIONS_ROOT,
};
use vcr::encoding::FfmpegPipe;
use vcr::renderer::Renderer;
use vcr::schema::{Duration as ManifestDuration, Environment, Resolution};
use vcr::timeline::RenderSceneData;

fn main() -> Result<()> {
    let animation_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ascii_co_uk_3d_tunnel".to_owned());
    let source_fps = std::env::args()
        .nth(2)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(12);

    let mut manager = AnimationManager::new();
    manager.load_from_assets_root(
        DEFAULT_ANIMATIONS_ROOT,
        &animation_name,
        AnimationImportOptions {
            source_fps,
            strip_ansi_escape_codes: true,
        },
    )?;

    for credit in manager.credits_manifest() {
        println!("credit: {}", credit.credit_line);
    }

    terminal_preview_loop(&manager, &animation_name)?;
    render_overlay_outputs(&manager, &animation_name)?;

    Ok(())
}

fn terminal_preview_loop(manager: &AnimationManager, animation_name: &str) -> Result<()> {
    let output_fps = 24;
    let playback = PlaybackOptions::default();

    println!("Playing {} in terminal preview...", animation_name);
    for frame in 0..24 {
        let text = manager
            .sample_frame_text(animation_name, frame, output_fps, playback)
            .with_context(|| format!("failed to sample terminal frame {frame}"))?;
        print!("\x1B[2J\x1B[H");
        println!("frame {} @ {}fps\n{}", frame, output_fps, text);
        thread::sleep(Duration::from_millis(1000 / u64::from(output_fps)));
    }

    Ok(())
}

fn render_overlay_outputs(manager: &AnimationManager, animation_name: &str) -> Result<()> {
    let frame_count = 96;
    let environment = Environment {
        resolution: Resolution {
            width: 640,
            height: 360,
        },
        fps: 24,
        duration: ManifestDuration::Frames {
            frames: frame_count,
        },
        color_space: Default::default(),
    };
    let scene = RenderSceneData::default();
    let mut renderer = Renderer::new_software(&environment, &[], scene)?;

    let mut layer = AnimationLayer::new(animation_name);
    layer.playback = PlaybackOptions::default();
    layer.cell = AsciiCellMetrics {
        width: 9,
        height: 14,
        pixel_aspect_ratio: 1.0,
    };
    layer.colors.foreground = [255, 255, 255, 255];
    layer.colors.background = [0, 0, 0, 0];
    layer.fit = FitOptions {
        padding_px: 24,
        anchor_x: 0.5,
        anchor_y: 0.5,
    };
    layer.filter = BoutiqueFilter {
        seed: 7,
        drop_frame_probability: 0.0,
        brightness_jitter: 0.0,
        horizontal_shift_px: 0,
    };

    let first_frame_rgba = renderer.render_frame_rgba_with_animation_layer(0, manager, &layer)?;
    let first_frame_image = RgbaImage::from_raw(
        environment.resolution.width,
        environment.resolution.height,
        first_frame_rgba,
    )
    .ok_or_else(|| anyhow::anyhow!("failed to build output image from RGBA frame"))?;

    let output_still = PathBuf::from("renders/ascii_animation_overlay_demo.png");
    if let Some(parent) = output_still.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    first_frame_image
        .save(&output_still)
        .with_context(|| format!("failed to write {}", output_still.display()))?;
    println!("Wrote {}", output_still.display());

    let output_mov = PathBuf::from("renders/ascii_animation_overlay_demo.mov");
    let ffmpeg = FfmpegPipe::spawn(&environment, &output_mov)?;
    for frame_index in 0..frame_count {
        let rgba = renderer.render_frame_rgba_with_animation_layer(frame_index, manager, &layer)?;
        ffmpeg.write_frame(rgba)?;
    }
    ffmpeg.finish()?;
    println!("Wrote {}", output_mov.display());

    Ok(())
}
