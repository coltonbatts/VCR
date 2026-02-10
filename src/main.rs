mod encoding;
mod manifest;
mod renderer;
mod schema;

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::encoding::FfmpegPipe;
use crate::manifest::load_and_validate_manifest;
use crate::renderer::Renderer;

#[derive(Debug, Parser)]
#[command(name = "ftc")]
#[command(about = "Film Transform Compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Build {
        manifest: PathBuf,
        #[arg(short = 'o', long = "output")]
        output: PathBuf,
    },
    Check {
        manifest: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { manifest, output } => run_build(&manifest, &output),
        Commands::Check { manifest } => run_check(&manifest),
    }
}

fn run_check(manifest_path: &Path) -> Result<()> {
    let manifest = load_and_validate_manifest(manifest_path)?;

    println!(
        "OK: {} ({}x{}, {} fps, {} frames, {:?})",
        manifest_path.display(),
        manifest.environment.resolution.width,
        manifest.environment.resolution.height,
        manifest.environment.fps,
        manifest.environment.total_frames(),
        manifest.environment.color_space
    );
    println!("Layers: {}", manifest.layers.len());
    Ok(())
}

fn run_build(manifest_path: &Path, output_path: &Path) -> Result<()> {
    let manifest = load_and_validate_manifest(manifest_path)?;
    let total_frames = manifest.environment.total_frames();

    let mut renderer = pollster::block_on(Renderer::new(&manifest.environment, &manifest.layers))?;
    let ffmpeg = FfmpegPipe::spawn(&manifest.environment, output_path)?;

    for frame_index in 0..total_frames {
        let rgba = renderer.render_frame_rgba(frame_index)?;
        ffmpeg.write_frame(rgba)?;

        if frame_index % manifest.environment.fps == 0 {
            eprintln!("rendered frame {}/{}", frame_index + 1, total_frames);
        }
    }

    ffmpeg.finish()?;
    println!("Wrote {}", output_path.display());
    Ok(())
}
