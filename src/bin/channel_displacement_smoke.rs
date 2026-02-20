use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use vcr::channel_displacement::SeedLockedChannelDisplacement;
use vcr::encoding::FfmpegPipe;
use vcr::schema::{
    ColorSpace, Duration as ManifestDuration, EncodingConfig, Environment, ProResProfile,
    Resolution,
};

fn main() -> Result<()> {
    let width: u32 = 640;
    let height: u32 = 360;
    let fps: u32 = 60;
    let frame_count: u32 = 180;
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("renders/channel_displacement_alpha_smoke.mov"));

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create output dir {}", parent.display()))?;
        }
    }

    let environment = Environment {
        resolution: Resolution { width, height },
        fps,
        duration: ManifestDuration::Frames {
            frames: frame_count,
        },
        color_space: ColorSpace::Rec709,
        encoding: EncodingConfig {
            prores_profile: ProResProfile::Prores4444,
            ..EncodingConfig::default()
        },
    };
    environment
        .validate()
        .context("smoke environment validation failed")?;

    let displacement = SeedLockedChannelDisplacement {
        seed: 0xC0DE_BAAD_F00D_1234,
        luma_threshold: 92,
        max_offset: 11,
    };

    let ffmpeg = FfmpegPipe::spawn(&environment, &output_path).with_context(|| {
        format!(
            "failed to initialize ffmpeg pipe for {}",
            output_path.display()
        )
    })?;

    let width_usize = width as usize;
    let height_usize = height as usize;
    for frame_index in 0..frame_count {
        let mut rgba = build_frame(frame_index, width_usize, height_usize);
        displacement
            .apply(&mut rgba, width_usize, height_usize)
            .with_context(|| format!("displacement failed on frame {frame_index}"))?;
        ffmpeg
            .write_frame(rgba)
            .with_context(|| format!("failed writing frame {frame_index}"))?;
    }
    ffmpeg.finish().context("ffmpeg finalize failed")?;

    let absolute = output_path
        .canonicalize()
        .unwrap_or_else(|_| output_path.clone());
    println!("{}", absolute.display());
    Ok(())
}

fn build_frame(frame_index: u32, width: usize, height: usize) -> Vec<u8> {
    let mut rgba = vec![0_u8; width * height * 4];

    let t = frame_index as i32;
    let cx = (width as i32 / 2) + ((t * 3).rem_euclid(180) - 90);
    let cy = (height as i32 / 2) + ((t * 2).rem_euclid(120) - 60);
    let min_dim = width.min(height) as i32;
    let inner = (min_dim / 6).pow(2);
    let outer = (min_dim / 3).pow(2);

    for y in 0..height {
        let yi = y as i32;
        for x in 0..width {
            let xi = x as i32;
            let idx = ((y * width) + x) << 2;

            // Integer-only animated pattern with broad luma variation.
            let r = (((xi * 11 + yi * 3 + t * 17) ^ (yi * 7 - t * 5)).rem_euclid(256)) as u8;
            let g = ((xi * 5 + yi * 13 + t * 9).rem_euclid(256)) as u8;
            let b = (((xi * 19 - yi * 7 + t * 11) ^ (xi * 3 + t * 2)).rem_euclid(256)) as u8;

            // Soft circular alpha matte over transparent background.
            let dx = xi - cx;
            let dy = yi - cy;
            let d2 = dx * dx + dy * dy;
            let a = if d2 <= inner {
                255_u8
            } else if d2 >= outer {
                0_u8
            } else {
                (((outer - d2) * 255) / (outer - inner)) as u8
            };

            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = a;
        }
    }

    rgba
}
