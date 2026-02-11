mod ascii;
mod ascii_atlas;
mod encoding;
mod manifest;
mod renderer;
mod schema;
mod timeline;

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use image::RgbaImage;

use crate::encoding::FfmpegPipe;
use crate::manifest::load_and_validate_manifest;
use crate::renderer::Renderer;
use crate::schema::{Duration as ManifestDuration, Environment};
use crate::timeline::{evaluate_manifest_layers_at_frame, RenderSceneData};

#[derive(Debug, Parser)]
#[command(name = "vcr")]
#[command(about = "VCR (Video Component Renderer)")]
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
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "end-frame")]
        end_frame: Option<u32>,
        #[arg(long = "frames")]
        frames: Option<u32>,
    },
    Check {
        manifest: PathBuf,
    },
    Lint {
        manifest: PathBuf,
    },
    Dump {
        manifest: PathBuf,
        #[arg(long = "frame")]
        frame: Option<u32>,
        #[arg(long = "time")]
        time: Option<f32>,
    },
    Preview {
        manifest: PathBuf,
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "frames")]
        frames: Option<u32>,
        #[arg(long = "scale", default_value_t = 0.5)]
        scale: f32,
        #[arg(long = "image-sequence")]
        image_sequence: bool,
    },
    RenderFrame {
        manifest: PathBuf,
        #[arg(long = "frame")]
        frame: u32,
        #[arg(short = 'o', long = "output")]
        output: PathBuf,
    },
    RenderFrames {
        manifest: PathBuf,
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "frames")]
        frames: u32,
        #[arg(short = 'o', long = "output-dir")]
        output_dir: PathBuf,
    },
    Watch {
        manifest: PathBuf,
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "frames")]
        frames: Option<u32>,
        #[arg(long = "scale", default_value_t = 0.5)]
        scale: f32,
        #[arg(long = "image-sequence")]
        image_sequence: bool,
        #[arg(long = "interval-ms", default_value_t = 300)]
        interval_ms: u64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build {
            manifest,
            output,
            start_frame,
            end_frame,
            frames,
        } => {
            let frame_window = FrameWindowArgs {
                start_frame,
                end_frame,
                frames,
            };
            run_build(&manifest, &output, frame_window)
        }
        Commands::Check { manifest } => run_check(&manifest),
        Commands::Lint { manifest } => run_lint(&manifest),
        Commands::Dump {
            manifest,
            frame,
            time,
        } => run_dump(&manifest, frame, time),
        Commands::Preview {
            manifest,
            output,
            start_frame,
            frames,
            scale,
            image_sequence,
        } => run_preview(
            &manifest,
            output.as_deref(),
            PreviewArgs {
                start_frame,
                frames,
                scale,
                image_sequence,
            },
        ),
        Commands::RenderFrame {
            manifest,
            frame,
            output,
        } => run_render_frame(&manifest, frame, &output),
        Commands::RenderFrames {
            manifest,
            start_frame,
            frames,
            output_dir,
        } => run_render_frames(&manifest, start_frame, frames, &output_dir),
        Commands::Watch {
            manifest,
            output,
            start_frame,
            frames,
            scale,
            image_sequence,
            interval_ms,
        } => run_watch(
            &manifest,
            output,
            PreviewArgs {
                start_frame,
                frames,
                scale,
                image_sequence,
            },
            interval_ms,
        ),
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

fn run_lint(manifest_path: &Path) -> Result<()> {
    let manifest = load_and_validate_manifest(manifest_path)?;
    let total_frames = manifest.environment.total_frames();
    let sample_count = total_frames.min(240).max(1);
    let sample_step = (total_frames / sample_count).max(1);

    let mut visible = manifest
        .layers
        .iter()
        .map(|layer| (layer.id().to_owned(), false))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut frame = 0_u32;
    while frame < total_frames {
        let states = evaluate_manifest_layers_at_frame(&manifest, frame)?;
        for state in states {
            if state.visible && state.opacity > 0.0 {
                visible.insert(state.id, true);
            }
        }
        frame = frame.saturating_add(sample_step);
    }
    if total_frames > 1 {
        let last_states = evaluate_manifest_layers_at_frame(&manifest, total_frames - 1)?;
        for state in last_states {
            if state.visible && state.opacity > 0.0 {
                visible.insert(state.id, true);
            }
        }
    }

    let mut issues = Vec::new();
    for layer in &manifest.layers {
        let id = layer.id();
        if !visible.get(id).copied().unwrap_or(false) {
            issues.push(format!(
                "Layer '{id}' appears unreachable (never visible across sampled frames).",
            ));
        }
    }

    if issues.is_empty() {
        println!("Lint OK: no issues found in {}", manifest_path.display());
        return Ok(());
    }

    eprintln!("Lint found {} issue(s):", issues.len());
    for issue in &issues {
        eprintln!("- {issue}");
    }
    bail!("lint failed for {}", manifest_path.display())
}

fn run_dump(manifest_path: &Path, frame: Option<u32>, time: Option<f32>) -> Result<()> {
    if frame.is_some() && time.is_some() {
        bail!("use either --frame or --time, not both");
    }

    let manifest = load_and_validate_manifest(manifest_path)?;
    let total_frames = manifest.environment.total_frames();
    let selected_frame = if let Some(frame) = frame {
        frame
    } else if let Some(time) = time {
        if !time.is_finite() || time < 0.0 {
            bail!("--time must be a finite non-negative value");
        }
        (time * manifest.environment.fps as f32).round() as u32
    } else {
        0
    };

    if selected_frame >= total_frames {
        bail!(
            "frame {} is out of range for {} total frames",
            selected_frame,
            total_frames
        );
    }

    let states = evaluate_manifest_layers_at_frame(&manifest, selected_frame)?;
    println!(
        "Resolved scene at frame {} (t={:.3}s):",
        selected_frame,
        selected_frame as f32 / manifest.environment.fps as f32
    );
    for state in states {
        println!(
            "- id={} name={} stable_id={} z={} visible={} pos=({:.3}, {:.3}) scale=({:.3}, {:.3}) rot={:.3} opacity={:.3}",
            state.id,
            state.name.as_deref().unwrap_or("<none>"),
            state.stable_id.as_deref().unwrap_or("<none>"),
            state.z_index,
            state.visible,
            state.position.x,
            state.position.y,
            state.scale.x,
            state.scale.y,
            state.rotation_degrees,
            state.opacity
        );
    }
    Ok(())
}

fn run_build(manifest_path: &Path, output_path: &Path, args: FrameWindowArgs) -> Result<()> {
    let parse_start = Instant::now();
    let manifest = load_and_validate_manifest(manifest_path)?;
    let parse_elapsed = parse_start.elapsed();
    let total_frames = manifest.environment.total_frames();
    let window = resolve_frame_window(total_frames, args)?;

    let layout_start = Instant::now();
    let scene = RenderSceneData::from_manifest(&manifest);
    let mut renderer = pollster::block_on(Renderer::new_with_scene(
        &manifest.environment,
        &manifest.layers,
        scene,
    ))?;
    let layout_elapsed = layout_start.elapsed();
    eprintln!(
        "[VCR] Backend: {} ({})",
        renderer.backend_name(),
        renderer.backend_reason()
    );
    let ffmpeg = FfmpegPipe::spawn(&manifest.environment, output_path)?;
    let mut render_elapsed = Duration::ZERO;
    let mut encode_elapsed = Duration::ZERO;

    for (offset, frame_index) in window.frame_indices().enumerate() {
        let render_start = Instant::now();
        let rgba = renderer.render_frame_rgba(frame_index)?;
        render_elapsed += render_start.elapsed();

        let encode_start = Instant::now();
        ffmpeg.write_frame(rgba)?;
        encode_elapsed += encode_start.elapsed();

        if frame_index % manifest.environment.fps == 0 {
            eprintln!("rendered frame {}/{}", offset + 1, window.count);
        }
    }

    ffmpeg.finish()?;
    println!("Wrote {}", output_path.display());
    print_timing_summary(RenderTimingSummary {
        parse: parse_elapsed,
        layout: layout_elapsed,
        render: render_elapsed,
        encode: encode_elapsed,
    });
    Ok(())
}

fn run_preview(manifest_path: &Path, output: Option<&Path>, args: PreviewArgs) -> Result<()> {
    if !(0.0..=1.0).contains(&args.scale) || args.scale == 0.0 {
        bail!("preview --scale must be in (0, 1], got {}", args.scale);
    }

    let parse_start = Instant::now();
    let manifest = load_and_validate_manifest(manifest_path)?;
    let parse_elapsed = parse_start.elapsed();

    let total_frames = manifest.environment.total_frames();
    let default_preview_frames = (manifest.environment.fps.saturating_mul(3)).max(1);
    let window = resolve_frame_window(
        total_frames,
        FrameWindowArgs {
            start_frame: args.start_frame,
            end_frame: None,
            frames: Some(args.frames.unwrap_or(default_preview_frames)),
        },
    )?;

    let preview_environment = scaled_environment(&manifest.environment, args.scale);
    let layout_start = Instant::now();
    let scene = RenderSceneData::from_manifest(&manifest);
    let mut renderer = pollster::block_on(Renderer::new_with_scene(
        &preview_environment,
        &manifest.layers,
        scene,
    ))?;
    let layout_elapsed = layout_start.elapsed();

    eprintln!(
        "[VCR] Backend: {} ({})",
        renderer.backend_name(),
        renderer.backend_reason()
    );
    eprintln!(
        "[VCR] Preview: {}x{}, frames {}..{} ({} total)",
        preview_environment.resolution.width,
        preview_environment.resolution.height,
        window.start_frame,
        window.start_frame + window.count.saturating_sub(1),
        window.count
    );

    let mut render_elapsed = Duration::ZERO;
    let mut encode_elapsed = Duration::ZERO;
    if args.image_sequence {
        let output_dir = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("preview_frames"));
        fs::create_dir_all(&output_dir).with_context(|| {
            format!(
                "failed to create preview output directory {}",
                output_dir.display()
            )
        })?;

        for frame_index in window.frame_indices() {
            let render_start = Instant::now();
            let rgba = renderer.render_frame_rgba(frame_index)?;
            render_elapsed += render_start.elapsed();

            let encode_start = Instant::now();
            let name = format!("frame_{frame_index:06}.png");
            let path = output_dir.join(name);
            save_rgba_png(
                &path,
                preview_environment.resolution.width,
                preview_environment.resolution.height,
                rgba,
            )?;
            encode_elapsed += encode_start.elapsed();
        }

        println!("Wrote preview frame sequence to {}", output_dir.display());
    } else {
        let output_path = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("preview.mov"));
        let ffmpeg = FfmpegPipe::spawn(&preview_environment, &output_path)?;

        for frame_index in window.frame_indices() {
            let render_start = Instant::now();
            let rgba = renderer.render_frame_rgba(frame_index)?;
            render_elapsed += render_start.elapsed();

            let encode_start = Instant::now();
            ffmpeg.write_frame(rgba)?;
            encode_elapsed += encode_start.elapsed();
        }

        ffmpeg.finish()?;
        println!("Wrote {}", output_path.display());
    }

    print_timing_summary(RenderTimingSummary {
        parse: parse_elapsed,
        layout: layout_elapsed,
        render: render_elapsed,
        encode: encode_elapsed,
    });
    Ok(())
}

fn run_render_frame(manifest_path: &Path, frame_index: u32, output_path: &Path) -> Result<()> {
    let parse_start = Instant::now();
    let manifest = load_and_validate_manifest(manifest_path)?;
    let parse_elapsed = parse_start.elapsed();
    let total_frames = manifest.environment.total_frames();
    if frame_index >= total_frames {
        bail!(
            "--frame {} is out of bounds for {} total frames",
            frame_index,
            total_frames
        );
    }

    let layout_start = Instant::now();
    let scene = RenderSceneData::from_manifest(&manifest);
    let mut renderer = pollster::block_on(Renderer::new_with_scene(
        &manifest.environment,
        &manifest.layers,
        scene,
    ))?;
    let layout_elapsed = layout_start.elapsed();
    eprintln!(
        "[VCR] Backend: {} ({})",
        renderer.backend_name(),
        renderer.backend_reason()
    );

    let render_start = Instant::now();
    let rgba = renderer.render_frame_rgba(frame_index)?;
    let render_elapsed = render_start.elapsed();

    let encode_start = Instant::now();
    save_rgba_png(
        output_path,
        manifest.environment.resolution.width,
        manifest.environment.resolution.height,
        rgba,
    )?;
    let encode_elapsed = encode_start.elapsed();

    println!("Wrote {}", output_path.display());
    print_timing_summary(RenderTimingSummary {
        parse: parse_elapsed,
        layout: layout_elapsed,
        render: render_elapsed,
        encode: encode_elapsed,
    });
    Ok(())
}

fn run_render_frames(
    manifest_path: &Path,
    start_frame: u32,
    frames: u32,
    output_dir: &Path,
) -> Result<()> {
    if frames == 0 {
        bail!("--frames must be > 0");
    }

    let parse_start = Instant::now();
    let manifest = load_and_validate_manifest(manifest_path)?;
    let parse_elapsed = parse_start.elapsed();
    let total_frames = manifest.environment.total_frames();
    let window = resolve_frame_window(
        total_frames,
        FrameWindowArgs {
            start_frame,
            end_frame: None,
            frames: Some(frames),
        },
    )?;

    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;

    let layout_start = Instant::now();
    let scene = RenderSceneData::from_manifest(&manifest);
    let mut renderer = pollster::block_on(Renderer::new_with_scene(
        &manifest.environment,
        &manifest.layers,
        scene,
    ))?;
    let layout_elapsed = layout_start.elapsed();
    eprintln!(
        "[VCR] Backend: {} ({})",
        renderer.backend_name(),
        renderer.backend_reason()
    );

    let mut render_elapsed = Duration::ZERO;
    let mut encode_elapsed = Duration::ZERO;
    for frame_index in window.frame_indices() {
        let render_start = Instant::now();
        let rgba = renderer.render_frame_rgba(frame_index)?;
        render_elapsed += render_start.elapsed();

        let encode_start = Instant::now();
        let path = output_dir.join(format!("frame_{frame_index:06}.png"));
        save_rgba_png(
            &path,
            manifest.environment.resolution.width,
            manifest.environment.resolution.height,
            rgba,
        )?;
        encode_elapsed += encode_start.elapsed();
    }

    println!("Wrote {} frames to {}", window.count, output_dir.display());
    print_timing_summary(RenderTimingSummary {
        parse: parse_elapsed,
        layout: layout_elapsed,
        render: render_elapsed,
        encode: encode_elapsed,
    });
    Ok(())
}

fn run_watch(
    manifest_path: &Path,
    output: Option<PathBuf>,
    preview_args: PreviewArgs,
    interval_ms: u64,
) -> Result<()> {
    if interval_ms == 0 {
        bail!("--interval-ms must be > 0");
    }

    eprintln!(
        "[VCR] watch: monitoring {} every {}ms (Ctrl-C to stop)",
        manifest_path.display(),
        interval_ms
    );

    let mut last_stamp = read_file_stamp(manifest_path)?;
    if let Err(error) = run_preview(manifest_path, output.as_deref(), preview_args) {
        eprintln!("[VCR] watch: initial render failed: {error:#}");
    }

    loop {
        thread::sleep(Duration::from_millis(interval_ms));
        let stamp = match read_file_stamp(manifest_path) {
            Ok(stamp) => stamp,
            Err(error) => {
                eprintln!("[VCR] watch: failed to read manifest: {error:#}");
                continue;
            }
        };

        if stamp != last_stamp {
            eprintln!(
                "[VCR] watch: change detected in {}, rebuilding preview...",
                manifest_path.display()
            );
            last_stamp = stamp;
            if let Err(error) = run_preview(manifest_path, output.as_deref(), preview_args) {
                eprintln!("[VCR] watch: rebuild failed: {error:#}");
            }
        }
    }
}

fn save_rgba_png(path: &Path, width: u32, height: u32, rgba: Vec<u8>) -> Result<()> {
    let image = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| {
        anyhow::anyhow!(
            "failed to construct image buffer for {}x{} RGBA frame",
            width,
            height
        )
    })?;
    image
        .save(path)
        .with_context(|| format!("failed to write png {}", path.display()))
}

fn scaled_environment(environment: &Environment, scale: f32) -> Environment {
    let scaled_width = ((environment.resolution.width as f32) * scale)
        .round()
        .max(1.0) as u32;
    let scaled_height = ((environment.resolution.height as f32) * scale)
        .round()
        .max(1.0) as u32;

    Environment {
        resolution: crate::schema::Resolution {
            width: scaled_width,
            height: scaled_height,
        },
        fps: environment.fps,
        duration: ManifestDuration::Frames {
            frames: environment.total_frames(),
        },
        color_space: environment.color_space,
    }
}

#[derive(Debug, Clone, Copy)]
struct FrameWindowArgs {
    start_frame: u32,
    end_frame: Option<u32>,
    frames: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct PreviewArgs {
    start_frame: u32,
    frames: Option<u32>,
    scale: f32,
    image_sequence: bool,
}

#[derive(Debug, Clone, Copy)]
struct FrameWindow {
    start_frame: u32,
    count: u32,
}

impl FrameWindow {
    fn frame_indices(self) -> impl Iterator<Item = u32> {
        let end = self.start_frame.saturating_add(self.count);
        self.start_frame..end
    }
}

fn resolve_frame_window(total_frames: u32, args: FrameWindowArgs) -> Result<FrameWindow> {
    if total_frames == 0 {
        bail!("manifest has no renderable frames");
    }
    if args.start_frame >= total_frames {
        bail!(
            "start frame {} is out of bounds ({} frames)",
            args.start_frame,
            total_frames
        );
    }
    if args.end_frame.is_some() && args.frames.is_some() {
        bail!("use either --end-frame or --frames, not both");
    }

    let count = if let Some(end_frame) = args.end_frame {
        if end_frame < args.start_frame {
            bail!(
                "end frame {} must be >= start frame {}",
                end_frame,
                args.start_frame
            );
        }
        let bounded_end = end_frame.min(total_frames - 1);
        bounded_end - args.start_frame + 1
    } else if let Some(frames) = args.frames {
        if frames == 0 {
            bail!("--frames must be > 0");
        }
        let remaining = total_frames - args.start_frame;
        frames.min(remaining)
    } else {
        total_frames - args.start_frame
    };

    Ok(FrameWindow {
        start_frame: args.start_frame,
        count,
    })
}

#[derive(Debug, Clone, Copy)]
struct RenderTimingSummary {
    parse: Duration,
    layout: Duration,
    render: Duration,
    encode: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

fn read_file_stamp(path: &Path) -> Result<FileStamp> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    let modified = metadata.modified().ok();
    Ok(FileStamp {
        len: metadata.len(),
        modified,
    })
}

fn print_timing_summary(timing: RenderTimingSummary) {
    let total = timing.parse + timing.layout + timing.render + timing.encode;
    eprintln!(
        "[VCR] timing parse={:.2?} layout={:.2?} render={:.2?} encode={:.2?} total={:.2?}",
        timing.parse, timing.layout, timing.render, timing.encode, total
    );
}
