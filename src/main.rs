use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use image::RgbaImage;
use serde::Serialize;
use vcr::agent_errors::{build_agent_error_report, AgentErrorType};

use vcr::ascii_capture::{
    build_ascii_capture_plan, parse_capture_size, run_ascii_capture, AsciiCaptureArgs,
    AsciiCaptureSource, SymbolRemapMode, DEFAULT_CAPTURE_DURATION_SECONDS,
    DEFAULT_CAPTURE_FIT_PADDING, DEFAULT_CAPTURE_FONT_SIZE, DEFAULT_CAPTURE_FPS,
};
use vcr::ascii_render::{
    render_ascii_luma_sequence, run_ascii_render, AsciiDitherMode, AsciiLabRenderArgs,
    AsciiLabSequenceResult, AsciiRenderArgs, AsciiTemporalMode, DEFAULT_HYSTERESIS_BAND,
};
use vcr::ascii_sources::render_ascii_sources;
use vcr::ascii_stage::{
    parse_ascii_stage_size, render_ascii_stage_video, AsciiStageRenderArgs, CameraMode,
};
use vcr::chat::{render_chat_video, ChatRenderArgs};
use vcr::encoding::FfmpegPipe;
use vcr::manifest::{load_and_validate_manifest_with_options, ManifestLoadOptions, ParamOverride};
use vcr::play::{run_play, PlayArgs};
use vcr::renderer::Renderer;
use vcr::schema::{
    AsciiFontVariant, Duration as ManifestDuration, Environment, Manifest, ParamType, ParamValue,
    Resolution,
};
use vcr::timeline::{
    ascii_overrides_from_flags, evaluate_manifest_layers_at_frame, resolve_bayer_dither_override,
    resolve_edge_boost_override, AsciiRuntimeOverrides, RenderSceneData,
};

const EXIT_CODES_HELP: &str = "Exit codes: 0=success, 2=usage/arg error, 3=manifest validation error, 4=missing dependency, 5=I/O error";

fn version_string() -> String {
    let base = env!("CARGO_PKG_VERSION");
    #[allow(dead_code)]
    let hash: Option<&str> = option_env!("VCR_GIT_HASH");
    match hash {
        Some(h) if !h.is_empty() => format!("{base} ({h})"),
        _ => base.to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AsciiEdgeBoostArg {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AsciiBayerDitherArg {
    On,
    Off,
}

/// Resolve ASCII runtime overrides from CLI flags and env. CLI wins over env.
fn resolve_ascii_overrides(
    cli_edge_boost: Option<AsciiEdgeBoostArg>,
    cli_bayer_dither: Option<AsciiBayerDitherArg>,
) -> Option<AsciiRuntimeOverrides> {
    let edge_boost = resolve_edge_boost_override(
        cli_edge_boost.map(|a| matches!(a, AsciiEdgeBoostArg::On)),
        std::env::var("VCR_ASCII_EDGE_BOOST").ok(),
    );
    let bayer_dither = resolve_bayer_dither_override(
        cli_bayer_dither.map(|a| matches!(a, AsciiBayerDitherArg::On)),
        std::env::var("VCR_ASCII_BAYER_DITHER").ok(),
    );
    ascii_overrides_from_flags(edge_boost, bayer_dither)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BackendArg {
    Auto,
    Software,
    Gpu,
}

#[derive(Debug, Parser)]
#[command(name = "vcr")]
#[command(about = "VCR (Video Component Renderer)")]
#[command(after_help = EXIT_CODES_HELP)]
#[command(version = None)]
struct Cli {
    #[arg(
        long = "quiet",
        global = true,
        help = "Suppress non-essential parameter and diff logs (errors are still shown)"
    )]
    quiet: bool,
    #[arg(
        long = "backend",
        value_enum,
        global = true,
        default_value_t = BackendArg::Auto,
        help = "Force render backend: auto (default), software (deterministic), gpu"
    )]
    backend: BackendArg,
    #[arg(
        long = "ascii-edge-boost",
        value_enum,
        global = true,
        help = "Enable/disable ASCII edge boost when ascii_post is enabled. Overrides VCR_ASCII_EDGE_BOOST env."
    )]
    ascii_edge_boost: Option<AsciiEdgeBoostArg>,
    #[arg(
        long = "ascii-bayer-dither",
        value_enum,
        global = true,
        help = "Enable/disable ASCII Bayer dither when ascii_post is enabled. Overrides VCR_ASCII_BAYER_DITHER env."
    )]
    ascii_bayer_dither: Option<AsciiBayerDitherArg>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Build {
        manifest: PathBuf,
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "end-frame")]
        end_frame: Option<u32>,
        #[arg(long = "frames")]
        frames: Option<u32>,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    Check {
        manifest: PathBuf,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    Lint {
        manifest: PathBuf,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    Dump {
        manifest: PathBuf,
        #[arg(long = "frame")]
        frame: Option<u32>,
        #[arg(long = "time")]
        time: Option<f32>,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    Params {
        manifest: PathBuf,
        #[arg(
            long = "json",
            help = "Emit machine-readable JSON output (stable key ordering)."
        )]
        json: bool,
    },
    Explain {
        manifest: PathBuf,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
        #[arg(
            long = "json",
            help = "Emit machine-readable JSON output (stable key ordering)."
        )]
        json: bool,
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
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    Play {
        manifest: PathBuf,
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "paused")]
        paused: bool,
    },
    RenderFrame {
        manifest: PathBuf,
        #[arg(long = "frame")]
        frame: u32,
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    RenderFrames {
        manifest: PathBuf,
        #[arg(long = "start-frame", default_value_t = 0)]
        start_frame: u32,
        #[arg(long = "frames")]
        frames: u32,
        #[arg(short = 'o', long = "output-dir")]
        output_dir: Option<PathBuf>,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
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
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime. Repeat flag for multiple overrides."
        )]
        set: Vec<String>,
    },
    Chat {
        #[command(subcommand)]
        command: ChatCommands,
    },
    Ascii {
        #[command(subcommand)]
        command: AsciiCommands,
    },
    Doctor,
    DeterminismReport {
        manifest: PathBuf,
        #[arg(long = "frame", default_value_t = 0)]
        frame: u32,
        #[arg(
            long = "set",
            value_name = "NAME=VALUE",
            action = clap::ArgAction::Append,
            help = "Override a manifest param at runtime."
        )]
        set: Vec<String>,
        #[arg(long = "json", help = "Emit machine-readable JSON output")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ChatCommands {
    Render {
        #[arg(long = "in", value_name = "SCRIPT")]
        input: PathBuf,
        #[arg(long = "out", value_name = "OUTPUT")]
        output: PathBuf,
        #[arg(long = "theme", default_value = "geist-pixel")]
        theme: String,
        #[arg(long = "fps", default_value_t = 30)]
        fps: u32,
        #[arg(long = "speed", default_value_t = 1.0)]
        speed: f32,
        #[arg(long = "seed", default_value_t = 0)]
        seed: u64,
    },
}

#[derive(Debug, Subcommand)]
enum AsciiCommands {
    Sources,
    Stage {
        #[arg(long = "in", value_name = "TRANSCRIPT")]
        input: PathBuf,
        #[arg(long = "out", value_name = "OUTPUT")]
        output: PathBuf,
        #[arg(long = "fps")]
        fps: Option<u32>,
        #[arg(long = "size")]
        size: Option<String>,
        #[arg(long = "seed", default_value_t = 0)]
        seed: u64,
        #[arg(long = "speed")]
        speed: Option<f32>,
        #[arg(long = "theme")]
        theme: Option<String>,
        #[arg(
            long = "chrome",
            default_value_t = true,
            action = clap::ArgAction::Set
        )]
        chrome: bool,
        #[arg(long = "camera", default_value = "static")]
        camera: AsciiCameraArg,
        #[arg(long = "preset", default_value = "none")]
        preset: AsciiPresetArg,
    },
    Render {
        #[arg(long = "in", value_name = "VIDEO")]
        input: PathBuf,
        #[arg(long = "out", value_name = "OUTPUT")]
        output: PathBuf,
        #[arg(long = "size", default_value = "1280x720")]
        size: String,
        #[arg(long = "font", default_value = "geist-pixel-regular")]
        font: String,
        #[arg(long = "bg-alpha", default_value_t = 0.0)]
        bg_alpha: f32,
        #[arg(long = "sidecar", default_value_t = false)]
        sidecar: bool,
        #[arg(long = "expected-hash")]
        expected_hash: Option<String>,
        #[arg(long = "temporal-mode", value_enum, default_value_t = AsciiTemporalModeArg::None)]
        temporal_mode: AsciiTemporalModeArg,
        #[arg(long = "hysteresis-band", default_value_t = DEFAULT_HYSTERESIS_BAND)]
        hysteresis_band: u8,
        #[arg(long = "dither", value_enum, default_value_t = AsciiDitherModeArg::None)]
        dither_mode: AsciiDitherModeArg,
        #[arg(long = "debug-stage-hashes", default_value_t = false)]
        debug_stage_hashes: bool,
    },
    Capture {
        #[arg(long = "source", value_name = "SOURCE")]
        source: String,
        #[arg(long = "out", value_name = "OUTPUT")]
        output: PathBuf,
        #[arg(long = "fps", default_value_t = DEFAULT_CAPTURE_FPS)]
        fps: u32,
        #[arg(long = "duration", default_value_t = DEFAULT_CAPTURE_DURATION_SECONDS)]
        duration: f32,
        #[arg(long = "frames")]
        frames: Option<u32>,
        #[arg(long = "size", default_value = "80x40")]
        size: String,
        #[arg(long = "font-path", value_name = "FONT")]
        font_path: Option<PathBuf>,
        #[arg(long = "font-size", default_value_t = DEFAULT_CAPTURE_FONT_SIZE)]
        font_size: f32,
        #[arg(long = "tmp-dir", value_name = "DIR")]
        tmp_dir: Option<PathBuf>,
        #[arg(long = "debug-txt-dir", value_name = "DIR")]
        debug_txt_dir: Option<PathBuf>,
        #[arg(long = "symbol-remap", value_enum, default_value_t = AsciiSymbolRemapArg::Equalize)]
        symbol_remap: AsciiSymbolRemapArg,
        #[arg(long = "symbol-ramp", value_name = "CHARS")]
        symbol_ramp: Option<String>,
        #[arg(long = "fit-padding", default_value_t = DEFAULT_CAPTURE_FIT_PADDING)]
        fit_padding: f32,
        #[arg(long = "bg-alpha", default_value_t = 1.0)]
        bg_alpha: f32,
        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
    },
    Lab {
        #[arg(long = "export-dir", value_name = "DIR")]
        export_dir: Option<PathBuf>,
        #[arg(long = "debug-stage-hashes", default_value_t = false)]
        debug_stage_hashes: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AsciiCameraArg {
    Static,
    SlowZoom,
    Follow,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AsciiPresetArg {
    None,
    X,
    Yt,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AsciiTemporalModeArg {
    None,
    Hysteresis,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AsciiDitherModeArg {
    None,
    #[value(name = "floyd_steinberg_cell", alias = "floyd-steinberg-cell")]
    FloydSteinbergCell,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AsciiSymbolRemapArg {
    None,
    Density,
    Equalize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AsciiPresetDefaults {
    fps: u32,
    size: &'static str,
    speed: f32,
    theme: &'static str,
    font_scale: f32,
}

fn ascii_preset_defaults(preset: AsciiPresetArg) -> AsciiPresetDefaults {
    match preset {
        AsciiPresetArg::None => AsciiPresetDefaults {
            fps: 30,
            size: "1920x1080",
            speed: 1.0,
            theme: "void",
            font_scale: 1.0,
        },
        AsciiPresetArg::X => AsciiPresetDefaults {
            fps: 30,
            size: "1080x1920",
            speed: 1.2,
            theme: "void",
            font_scale: 1.18,
        },
        AsciiPresetArg::Yt => AsciiPresetDefaults {
            fps: 30,
            size: "1920x1080",
            speed: 1.0,
            theme: "void",
            font_scale: 1.0,
        },
    }
}

fn ascii_camera_mode(value: AsciiCameraArg) -> CameraMode {
    match value {
        AsciiCameraArg::Static => CameraMode::Static,
        AsciiCameraArg::SlowZoom => CameraMode::SlowZoom,
        AsciiCameraArg::Follow => CameraMode::Follow,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedAsciiStageOptions {
    fps: u32,
    size: String,
    speed: f32,
    theme: String,
    font_scale: f32,
}

fn resolve_ascii_stage_options(
    preset: AsciiPresetArg,
    fps: Option<u32>,
    size: Option<&str>,
    speed: Option<f32>,
    theme: Option<&str>,
) -> ResolvedAsciiStageOptions {
    let defaults = ascii_preset_defaults(preset);
    ResolvedAsciiStageOptions {
        fps: fps.unwrap_or(defaults.fps),
        size: size.unwrap_or(defaults.size).to_owned(),
        speed: speed.unwrap_or(defaults.speed),
        theme: theme.unwrap_or(defaults.theme).to_owned(),
        font_scale: defaults.font_scale,
    }
}

impl Commands {
    fn name(&self) -> &'static str {
        match self {
            Self::Build { .. } => "build",
            Self::Check { .. } => "check",
            Self::Lint { .. } => "lint",
            Self::Dump { .. } => "dump",
            Self::Params { .. } => "params",
            Self::Explain { .. } => "explain",
            Self::Preview { .. } => "preview",
            Self::Play { .. } => "play",
            Self::RenderFrame { .. } => "render-frame",
            Self::RenderFrames { .. } => "render-frames",
            Self::Watch { .. } => "watch",
            Self::Chat { .. } => "chat",
            Self::Ascii { .. } => "ascii",
            Self::Doctor => "doctor",
            Self::DeterminismReport { .. } => "determinism-report",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VcrExitCode {
    Success = 0,
    Usage = 2,
    ManifestValidation = 3,
    MissingDependency = 4,
    Io = 5,
}

impl VcrExitCode {
    fn to_exit_code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

fn main() -> ExitCode {
    // Handle --version before parse (avoids subcommand requirement)
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("vcr {}", version_string());
        return VcrExitCode::Success.to_exit_code();
    }
    let cli = Cli::parse();
    let command_name = cli.command.name();
    match run_cli(cli) {
        Ok(()) => VcrExitCode::Success.to_exit_code(),
        Err(error) => {
            let exit_code = classify_exit_code(&error);
            print_cli_error(command_name, &error, exit_code);
            exit_code.to_exit_code()
        }
    }
}

fn run_cli(cli: Cli) -> Result<()> {
    let quiet = cli.quiet;
    let ascii_overrides = resolve_ascii_overrides(cli.ascii_edge_boost, cli.ascii_bayer_dither);

    match cli.command {
        Commands::Build {
            manifest,
            output,
            start_frame,
            end_frame,
            frames,
            set,
        } => {
            let output = resolve_output_path(&manifest, output, "mov", None, quiet)?;
            let frame_window = FrameWindowArgs {
                start_frame,
                end_frame,
                frames,
            };
            run_build(
                &manifest,
                &output,
                frame_window,
                &set,
                ascii_overrides,
                cli.backend,
                quiet,
            )
        }
        Commands::Check { manifest, set } => run_check(&manifest, &set, quiet),
        Commands::Lint { manifest, set } => run_lint(&manifest, &set, quiet),
        Commands::Dump {
            manifest,
            frame,
            time,
            set,
        } => run_dump(&manifest, frame, time, &set, quiet),
        Commands::Params { manifest, json } => run_params(&manifest, json),
        Commands::Explain {
            manifest,
            set,
            json,
        } => run_explain(&manifest, &set, json),
        Commands::Preview {
            manifest,
            output,
            start_frame,
            frames,
            scale,
            image_sequence,
            set,
        } => {
            let resolved_output = if image_sequence {
                let default_dir = default_preview_sequence_output_dir(&manifest);
                Some(output.unwrap_or(default_dir))
            } else {
                Some(resolve_output_path(
                    &manifest,
                    output,
                    "mov",
                    Some("preview"),
                    quiet,
                )?)
            };
            run_preview(
                &manifest,
                resolved_output.as_deref(),
                PreviewArgs {
                    start_frame,
                    frames,
                    scale,
                    image_sequence,
                },
                &set,
                ascii_overrides.as_ref(),
                cli.backend,
                quiet,
            )
            .map(|_| ())
        }
        Commands::Play {
            manifest,
            start_frame,
            paused,
        } => run_play(
            &manifest,
            PlayArgs {
                start_frame,
                paused,
            },
        ),
        Commands::RenderFrame {
            manifest,
            frame,
            output,
            set,
        } => {
            let output = resolve_output_path(
                &manifest,
                output,
                "png",
                Some(&format!("frame_{frame:06}")),
                quiet,
            )?;
            run_render_frame(
                &manifest,
                frame,
                &output,
                &set,
                ascii_overrides.as_ref(),
                cli.backend,
                quiet,
            )
        }
        Commands::RenderFrames {
            manifest,
            start_frame,
            frames,
            output_dir,
            set,
        } => {
            let output_dir = output_dir.unwrap_or_else(|| {
                let stem = manifest.file_stem().unwrap_or_default().to_string_lossy();
                PathBuf::from(format!("renders/{}_frames", stem))
            });
            run_render_frames(
                &manifest,
                start_frame,
                frames,
                &output_dir,
                &set,
                ascii_overrides.as_ref(),
                cli.backend,
                quiet,
            )
        }
        Commands::Watch {
            manifest,
            output,
            start_frame,
            frames,
            scale,
            image_sequence,
            interval_ms,
            set,
        } => {
            let resolved_output = if image_sequence {
                let default_dir = default_preview_sequence_output_dir(&manifest);
                Some(output.unwrap_or(default_dir))
            } else {
                Some(resolve_output_path(
                    &manifest,
                    output,
                    "mov",
                    Some("preview"),
                    quiet,
                )?)
            };
            run_watch(
                &manifest,
                resolved_output,
                PreviewArgs {
                    start_frame,
                    frames,
                    scale,
                    image_sequence,
                },
                interval_ms,
                &set,
                ascii_overrides.as_ref(),
                cli.backend,
                quiet,
            )
        }
        Commands::Chat { command } => match command {
            ChatCommands::Render {
                input,
                output,
                theme,
                fps,
                speed,
                seed,
            } => run_chat_render(&input, &output, &theme, fps, speed, seed, quiet),
        },
        Commands::Ascii { command } => match command {
            AsciiCommands::Sources => {
                print!("{}", render_ascii_sources());
                Ok(())
            }
            AsciiCommands::Stage {
                input,
                output,
                fps,
                size,
                seed,
                speed,
                theme,
                chrome,
                camera,
                preset,
            } => run_ascii_stage_render(
                &input,
                &output,
                fps,
                size.as_deref(),
                speed,
                theme.as_deref(),
                seed,
                chrome,
                camera,
                preset,
                quiet,
            ),
            AsciiCommands::Render {
                input,
                output,
                size,
                font,
                bg_alpha,
                sidecar,
                expected_hash,
                temporal_mode,
                hysteresis_band,
                dither_mode,
                debug_stage_hashes,
            } => run_ascii_render_cli(
                &input,
                &output,
                &size,
                &font,
                bg_alpha,
                sidecar,
                expected_hash,
                temporal_mode,
                hysteresis_band,
                dither_mode,
                debug_stage_hashes,
                quiet,
            ),
            AsciiCommands::Capture {
                source,
                output,
                fps,
                duration,
                frames,
                size,
                font_path,
                font_size,
                tmp_dir,
                debug_txt_dir,
                symbol_remap,
                symbol_ramp,
                fit_padding,
                bg_alpha,
                dry_run,
            } => run_ascii_capture_cli(
                &source,
                &output,
                fps,
                duration,
                frames,
                &size,
                font_path.as_deref(),
                font_size,
                tmp_dir.as_deref(),
                debug_txt_dir.as_deref(),
                symbol_remap,
                symbol_ramp.as_deref(),
                fit_padding,
                bg_alpha,
                dry_run,
                quiet,
            ),
            AsciiCommands::Lab {
                export_dir,
                debug_stage_hashes,
            } => run_ascii_lab(export_dir.as_deref(), debug_stage_hashes),
        },
        Commands::Doctor => run_doctor(),
        Commands::DeterminismReport {
            manifest,
            frame,
            set,
            json,
        } => run_determinism_report(&manifest, frame, &set, json),
    }
}

fn print_cli_error(command_name: &str, error: &anyhow::Error, exit_code: VcrExitCode) {
    let head = single_line(error.to_string());
    let root = error
        .chain()
        .last()
        .map(|cause| single_line(cause.to_string()))
        .unwrap_or_else(|| head.clone());
    let summary = if root == head {
        head.clone()
    } else {
        format!("{head}. {root}")
    };

    if agent_mode_enabled() {
        let error_type = classify_agent_error_type(command_name, exit_code);
        let report = build_agent_error_report(error_type, &head, &summary, error);
        if let Ok(json) = serde_json::to_string_pretty(&report) {
            eprintln!("{json}");
        } else {
            eprintln!("vcr {command_name}: {}", report.summary);
        }
        return;
    }

    eprintln!("vcr {command_name}: {summary}");
    if std::env::var_os("VCR_ERROR_VERBOSE").is_some() {
        for cause in error.chain().skip(1) {
            eprintln!("detail: {}", single_line(cause.to_string()));
        }
    }
}

fn agent_mode_enabled() -> bool {
    std::env::var("VCR_AGENT_MODE")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn classify_agent_error_type(command_name: &str, exit_code: VcrExitCode) -> AgentErrorType {
    if command_name == "lint" {
        return AgentErrorType::Lint;
    }

    match exit_code {
        VcrExitCode::ManifestValidation => AgentErrorType::Validation,
        VcrExitCode::Usage => AgentErrorType::Usage,
        VcrExitCode::MissingDependency => AgentErrorType::MissingDependency,
        VcrExitCode::Io => AgentErrorType::Io,
        VcrExitCode::Success => AgentErrorType::Build,
    }
}

fn single_line(value: String) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn classify_exit_code(error: &anyhow::Error) -> VcrExitCode {
    if is_missing_dependency_error(error) {
        return VcrExitCode::MissingDependency;
    }
    if is_usage_error(error) {
        return VcrExitCode::Usage;
    }
    if is_io_error(error) {
        return VcrExitCode::Io;
    }
    VcrExitCode::ManifestValidation
}

fn is_missing_dependency_error(error: &anyhow::Error) -> bool {
    has_error_message_fragment(error, "ffmpeg was not found on path")
        || has_error_message_fragment(error, "curl was not found on path")
        || has_error_message_fragment(error, "chafa was not found on path")
        || has_error_message_fragment(error, "missing dependency")
        || has_error_message_fragment(error, "geist pixel font")
}

fn is_usage_error(error: &anyhow::Error) -> bool {
    has_error_message_fragment(error, "invalid --set")
        || has_error_message_fragment(error, "expected name=value")
        || has_error_message_fragment(error, "use either --frame or --time")
        || has_error_message_fragment(error, "use either --end-frame or --frames")
        || has_error_message_fragment(error, "--time must be")
        || has_error_message_fragment(error, "--frames must be > 0")
        || has_error_message_fragment(error, "--interval-ms must be > 0")
        || has_error_message_fragment(error, "preview --scale must be in")
        || has_error_message_fragment(error, "invalid .vcrchat format")
        || has_error_message_fragment(error, "invalid .vcrtxt format")
        || has_error_message_fragment(error, "empty input script")
        || has_error_message_fragment(error, "empty input transcript")
        || has_error_message_fragment(error, "unknown --theme")
        || has_error_message_fragment(error, "invalid --size")
        || has_error_message_fragment(error, "invalid --source")
        || has_error_message_fragment(error, "unsupported ascii-live stream")
        || has_error_message_fragment(error, "invalid --export-dir")
        || has_error_message_fragment(error, "--fps must be > 0")
        || has_error_message_fragment(error, "--duration must be > 0")
        || has_error_message_fragment(error, "--font-size must be > 0")
        || has_error_message_fragment(error, "--speed must be > 0")
        || has_error_message_fragment(error, "start frame")
        || has_error_message_fragment(error, "out of bounds")
}

fn is_io_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<std::io::Error>().is_some())
        || has_error_message_fragment(error, "failed to read")
        || has_error_message_fragment(error, "failed to write")
        || has_error_message_fragment(error, "failed waiting")
        || has_error_message_fragment(error, "failed to create")
}

fn has_error_message_fragment(error: &anyhow::Error, fragment: &str) -> bool {
    let needle = fragment.to_ascii_lowercase();
    error
        .chain()
        .map(|cause| cause.to_string().to_ascii_lowercase())
        .any(|message| message.contains(&needle))
}

fn resolve_output_path(
    manifest_path: &Path,
    provided_output: Option<PathBuf>,
    extension: &str,
    suffix: Option<&str>,
    quiet: bool,
) -> Result<PathBuf> {
    fs::create_dir_all("renders").context("failed to create 'renders' directory")?;
    let _renders_dir =
        fs::canonicalize("renders").context("failed to canonicalize 'renders' directory")?;

    let path = if let Some(p) = provided_output {
        if p.is_absolute() {
            bail!("Absolute output paths are restricted for security. Please use a relative path. Got: {}", p.display());
        }

        let _resolved = std::env::current_dir()?.join(&p);
        // We don't use canonicalize here because the file might not exist yet.
        // But we can check for ".." in the components.
        if p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            bail!(
                "Directory traversal in output path is not allowed: {}",
                p.display()
            );
        }
        p
    } else {
        let stem = manifest_path
            .file_stem()
            .context("manifest path has no filename")?
            .to_string_lossy();

        let filename = if let Some(s) = suffix {
            format!("{}_{}.{}", stem, s, extension)
        } else {
            format!("{}.{}", stem, extension)
        };
        PathBuf::from("renders").join(filename)
    };

    progress_log(quiet, format_args!("[VCR] Output path: {}", path.display()));
    Ok(path)
}

fn default_preview_sequence_output_dir(manifest_path: &Path) -> PathBuf {
    let stem = manifest_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("scene");
    PathBuf::from(format!("renders/{}_preview", stem))
}

fn run_doctor() -> Result<()> {
    println!("[VCR] Running system check...");
    let mut all_ok = true;
    let mut missing_dependencies = Vec::new();

    // 1. Check FFmpeg
    print!("- FFmpeg: ");
    match std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => println!("OK"),
        _ => {
            println!("MISSING (required for 'build' and 'preview' video output)");
            all_ok = false;
            missing_dependencies.push("ffmpeg");
        }
    }

    // 2. Check Fonts
    print!("- Fonts (Geist Pixel): ");
    let font_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/geist_pixel/GeistPixel-Line.ttf");
    if font_path.exists() {
        println!("OK");
    } else {
        println!("MISSING ({})", font_path.display());
        all_ok = false;
        missing_dependencies.push("fonts");
    }

    // 3. Check Backend
    print!("- Backend: ");
    let temp_env = Environment {
        resolution: Resolution {
            width: 16,
            height: 16,
        },
        fps: 24,
        duration: ManifestDuration::Frames { frames: 1 },
        color_space: Default::default(),
    };
    match pollster::block_on(Renderer::new_with_scene(
        &temp_env,
        &[],
        RenderSceneData::default(),
    )) {
        Ok(renderer) => {
            println!(
                "OK ({} - {})",
                renderer.backend_name(),
                renderer.backend_reason()
            );
        }
        Err(e) => {
            println!("ERROR ({e})");
            all_ok = false;
        }
    }

    if all_ok {
        println!("\n[VCR] Doctor: System is healthy. Ready to render.");
        Ok(())
    } else if missing_dependencies.is_empty() {
        println!("\n[VCR] Doctor: Some checks failed. Please address the issues above.");
        bail!("doctor check failed")
    } else {
        println!("\n[VCR] Doctor: Some checks failed. Please address the issues above.");
        bail!("missing dependency: {}", missing_dependencies.join(", "))
    }
}

fn run_determinism_report(
    manifest_path: &Path,
    frame: u32,
    set_values: &[String],
    json: bool,
) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
    let total_frames = manifest.environment.total_frames();
    if frame >= total_frames {
        bail!(
            "--frame {} is out of bounds for {} total frames",
            frame,
            total_frames
        );
    }

    let scene = RenderSceneData::from_manifest(&manifest);
    let mut renderer = Renderer::new_software(&manifest.environment, &manifest.layers, scene)?;
    let rgba = renderer.render_frame_rgba(frame)?;
    let hash = fnv1a64(&rgba);

    if json {
        #[derive(serde::Serialize)]
        struct Report {
            manifest: String,
            frame: u32,
            backend: &'static str,
            frame_hash: String,
        }
        let report = Report {
            manifest: manifest_path.display().to_string(),
            frame,
            backend: "software",
            frame_hash: format!("0x{hash:016x}"),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("manifest: {}", manifest_path.display());
        println!("frame: {}", frame);
        println!("backend: software");
        println!("frame_hash: 0x{hash:016x}");
    }
    Ok(())
}

fn run_chat_render(
    input_path: &Path,
    output_path: &Path,
    theme: &str,
    fps: u32,
    speed: f32,
    seed: u64,
    quiet: bool,
) -> Result<()> {
    let resolved_output = resolve_output_path(
        input_path,
        Some(output_path.to_path_buf()),
        "mp4",
        None,
        quiet,
    )?;
    if let Some(parent) = resolved_output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    progress_log(
        quiet,
        format_args!(
            "[VCR] Chat render: theme={}, fps={}, speed={:.2}, seed={}",
            theme, fps, speed, seed
        ),
    );

    let summary = render_chat_video(&ChatRenderArgs {
        input: input_path.to_path_buf(),
        output: resolved_output.clone(),
        theme: theme.to_owned(),
        fps,
        speed,
        seed,
    })?;

    println!("Wrote {}", resolved_output.display());
    progress_log(
        quiet,
        format_args!(
            "[VCR] Chat render complete: {}x{}, {} frames, {:.2}s",
            summary.width,
            summary.height,
            summary.frame_count,
            summary.duration_ms as f64 / 1000.0
        ),
    );
    Ok(())
}

fn run_ascii_stage_render(
    input_path: &Path,
    output_path: &Path,
    fps: Option<u32>,
    size: Option<&str>,
    speed: Option<f32>,
    theme: Option<&str>,
    seed: u64,
    chrome: bool,
    camera: AsciiCameraArg,
    preset: AsciiPresetArg,
    quiet: bool,
) -> Result<()> {
    let resolved = resolve_ascii_stage_options(preset, fps, size, speed, theme);
    let (width, height) = parse_ascii_stage_size(&resolved.size)?;
    let resolved_output = resolve_output_path(
        input_path,
        Some(output_path.to_path_buf()),
        "mp4",
        None,
        quiet,
    )?;
    if let Some(parent) = resolved_output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    progress_log(
        quiet,
        format_args!(
            "[VCR] ASCII stage: theme={}, fps={}, size={}x{}, speed={:.2}, seed={}, camera={:?}, preset={:?}",
            resolved.theme, resolved.fps, width, height, resolved.speed, seed, camera, preset
        ),
    );

    let summary = render_ascii_stage_video(&AsciiStageRenderArgs {
        input: input_path.to_path_buf(),
        output: resolved_output.clone(),
        theme: resolved.theme.clone(),
        fps: resolved.fps,
        speed: resolved.speed,
        seed,
        width,
        height,
        chrome,
        camera_mode: ascii_camera_mode(camera),
        font_scale: resolved.font_scale,
    })?;

    println!("Wrote {}", resolved_output.display());
    progress_log(
        quiet,
        format_args!(
            "[VCR] ASCII stage complete: {}x{}, {} frames, {:.2}s",
            summary.width,
            summary.height,
            summary.frame_count,
            summary.duration_ms as f64 / 1000.0
        ),
    );
    Ok(())
}

fn run_ascii_render_cli(
    input: &Path,
    output: &Path,
    size: &str,
    font: &str,
    bg_alpha: f32,
    sidecar: bool,
    expected_hash: Option<String>,
    temporal_mode: AsciiTemporalModeArg,
    hysteresis_band: u8,
    dither_mode: AsciiDitherModeArg,
    debug_stage_hashes: bool,
    quiet: bool,
) -> Result<()> {
    let (width, height) = parse_ascii_stage_size(size)?;
    let font_variant = match font.to_lowercase().as_str() {
        "geist-pixel-regular" | "regular" => AsciiFontVariant::GeistPixelRegular,
        "geist-pixel-medium" | "medium" => AsciiFontVariant::GeistPixelMedium,
        "geist-pixel-bold" | "bold" => AsciiFontVariant::GeistPixelBold,
        "geist-pixel-light" | "light" => AsciiFontVariant::GeistPixelLight,
        "geist-pixel-mono" | "mono" => AsciiFontVariant::GeistPixelMono,
        _ => bail!("unknown font variant '{}'", font),
    };

    let expected_hash = if let Some(h) = expected_hash {
        let h = h.trim();
        if let Some(hex) = h.strip_prefix("0x") {
            Some(u64::from_str_radix(hex, 16).context("invalid hex hash")?)
        } else {
            Some(h.parse::<u64>().context("invalid decimal hash")?)
        }
    } else {
        None
    };

    let temporal_mode = match temporal_mode {
        AsciiTemporalModeArg::None => AsciiTemporalMode::None,
        AsciiTemporalModeArg::Hysteresis => AsciiTemporalMode::Hysteresis {
            band: hysteresis_band,
        },
    };
    let dither_mode = match dither_mode {
        AsciiDitherModeArg::None => AsciiDitherMode::None,
        AsciiDitherModeArg::FloydSteinbergCell => AsciiDitherMode::FloydSteinbergCell,
    };

    if !quiet {
        println!(
            "[VCR] ASCII render: size={}x{}, font={:?}, bg_alpha={}, temporal={:?}, dither={:?}, debug_stage_hashes={}",
            width, height, font_variant, bg_alpha, temporal_mode, dither_mode, debug_stage_hashes
        );
    }

    run_ascii_render(AsciiRenderArgs {
        input,
        output,
        width,
        height,
        font_variant,
        bg_alpha,
        sidecar,
        expected_hash,
        temporal_mode,
        dither_mode,
        debug_stage_hashes,
    })
}

fn run_ascii_capture_cli(
    source: &str,
    output: &Path,
    fps: u32,
    duration: f32,
    frames: Option<u32>,
    size: &str,
    font_path: Option<&Path>,
    font_size: f32,
    tmp_dir: Option<&Path>,
    debug_txt_dir: Option<&Path>,
    symbol_remap: AsciiSymbolRemapArg,
    symbol_ramp: Option<&str>,
    fit_padding: f32,
    bg_alpha: f32,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    let (cols, rows) = parse_capture_size(size)?;
    let source = AsciiCaptureSource::parse(source)?;
    let resolved_output = resolve_output_path(
        Path::new("ascii_capture"),
        Some(output.to_path_buf()),
        "mov",
        None,
        quiet,
    )?;
    let args = AsciiCaptureArgs {
        source,
        output: resolved_output.clone(),
        fps,
        duration_seconds: duration,
        max_frames: frames,
        cols,
        rows,
        font_path: font_path.map(Path::to_path_buf),
        font_size,
        tmp_dir: tmp_dir.map(Path::to_path_buf),
        debug_txt_dir: debug_txt_dir.map(Path::to_path_buf),
        symbol_remap: match symbol_remap {
            AsciiSymbolRemapArg::None => SymbolRemapMode::None,
            AsciiSymbolRemapArg::Density => SymbolRemapMode::Density,
            AsciiSymbolRemapArg::Equalize => SymbolRemapMode::Equalize,
        },
        symbol_ramp: symbol_ramp.map(ToOwned::to_owned),
        fit_padding,
        bg_alpha,
    };

    let plan = build_ascii_capture_plan(&args)?;
    if dry_run {
        println!("Capture plan:");
        println!("  source: {}", plan.source_label);
        println!("  source_command: {}", plan.source_command.join(" "));
        println!("  output: {}", plan.output.display());
        println!("  frame_count: {}", plan.frame_count);
        println!("  fps: {}", plan.fps);
        println!("  duration_seconds: {:.3}", plan.duration_seconds);
        println!("  size: {}x{} cells", plan.cols, plan.rows);
        println!(
            "  font: {} @ {:.2}",
            plan.font_path.display(),
            plan.font_size
        );
        println!(
            "  tmp_dir: {}",
            plan.tmp_dir
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_owned())
        );
        println!("  parser: {}", plan.parser_mode);
        println!("  encoder: {}", plan.ffmpeg_encoder);
        println!("  symbol_remap: {:?}", plan.symbol_remap);
        println!("  symbol_ramp: {}", plan.symbol_ramp);
        println!("  fit_padding: {:.3}", plan.fit_padding);
        return Ok(());
    }

    progress_log(
        quiet,
        format_args!(
            "[VCR] ASCII capture: source={}, fps={}, duration={:.2}, frames={}, size={}x{}, font_size={:.2}, symbol_remap={:?}, fit_padding={:.2}",
            plan.source_label,
            plan.fps,
            plan.duration_seconds,
            plan.frame_count,
            plan.cols,
            plan.rows,
            plan.font_size,
            plan.symbol_remap,
            plan.fit_padding
        ),
    );
    progress_log(
        quiet,
        format_args!(
            "[VCR] ASCII capture parser='{}', encoder='{}'",
            plan.parser_mode, plan.ffmpeg_encoder
        ),
    );

    let summary = run_ascii_capture(&args)?;
    println!("Wrote {}", summary.output.display());
    progress_log(
        quiet,
        format_args!(
            "[VCR] ASCII capture complete: {} frames @ {} fps, grid={}x{}, output={}x{}",
            summary.frame_count,
            summary.fps,
            summary.cols,
            summary.rows,
            summary.pixel_width,
            summary.pixel_height
        ),
    );

    Ok(())
}

const ASCII_LAB_COLS: u32 = 80;
const ASCII_LAB_ROWS: u32 = 40;
const ASCII_LAB_DELIMITER: &str = "----------------------------------------";

#[derive(Debug, Clone)]
struct AsciiLabPattern {
    name: &'static str,
    slug: &'static str,
    frames: Vec<Vec<u8>>,
    is_motion: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct AsciiLabModeConfig {
    temporal: &'static str,
    dither: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    band: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
struct AsciiLabModeSpec {
    slug: &'static str,
    config: AsciiLabModeConfig,
    temporal_mode: AsciiTemporalMode,
    dither_mode: AsciiDitherMode,
}

#[derive(Debug, Serialize)]
struct AsciiLabStageHashMetadata {
    frame_index: u32,
    luma_grid_hash: String,
    mapped_grid_hash: String,
    frame_chars_hash: String,
}

#[derive(Debug, Serialize)]
struct AsciiLabExportMetadata {
    pattern: String,
    cols: u32,
    rows: u32,
    mode: AsciiLabModeConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_sequence_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage_hashes: Option<Vec<AsciiLabStageHashMetadata>>,
}

fn run_ascii_lab(export_dir: Option<&Path>, debug_stage_hashes: bool) -> Result<()> {
    let export_root = resolve_ascii_lab_export_dir(export_dir)?;
    let patterns = ascii_lab_patterns(ASCII_LAB_COLS, ASCII_LAB_ROWS);
    let modes = ascii_lab_modes();

    for pattern in patterns {
        println!("=== Pattern: {} ===", pattern.name);
        for mode in modes {
            let sequence = render_ascii_luma_sequence(AsciiLabRenderArgs {
                luma_frames: &pattern.frames,
                cols: ASCII_LAB_COLS,
                rows: ASCII_LAB_ROWS,
                font_variant: AsciiFontVariant::GeistPixelRegular,
                temporal_mode: mode.temporal_mode,
                dither_mode: mode.dither_mode,
                debug_stage_hashes,
            })?;

            let mode_output =
                ascii_lab_mode_output_text(&sequence, mode.config, pattern.is_motion)?;
            println!("{mode_output}");
            println!("{ASCII_LAB_DELIMITER}");

            if let Some(root) = &export_root {
                export_ascii_lab_mode(root, &pattern, mode, &sequence, debug_stage_hashes)?;
            }
        }
    }

    Ok(())
}

fn resolve_ascii_lab_export_dir(export_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let Some(dir) = export_dir else {
        return Ok(None);
    };

    if dir.is_absolute() {
        bail!(
            "invalid --export-dir '{}': absolute paths are restricted; use a relative path",
            dir.display()
        );
    }
    if dir
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!(
            "invalid --export-dir '{}': directory traversal is not allowed",
            dir.display()
        );
    }

    fs::create_dir_all(dir).with_context(|| {
        format!(
            "failed to create ascii lab export directory '{}'",
            dir.display()
        )
    })?;

    Ok(Some(dir.to_path_buf()))
}

fn ascii_lab_patterns(cols: u32, rows: u32) -> Vec<AsciiLabPattern> {
    vec![
        AsciiLabPattern {
            name: "Horizontal Gradient",
            slug: "horizontal_gradient",
            frames: vec![generate_horizontal_gradient(cols, rows)],
            is_motion: false,
        },
        AsciiLabPattern {
            name: "Radial Gradient",
            slug: "radial_gradient",
            frames: vec![generate_radial_gradient(cols, rows)],
            is_motion: false,
        },
        AsciiLabPattern {
            name: "Checkerboard",
            slug: "checkerboard",
            frames: vec![generate_checkerboard(cols, rows)],
            is_motion: false,
        },
        AsciiLabPattern {
            name: "Vertical Edge",
            slug: "vertical_edge",
            frames: vec![generate_vertical_edge(cols, rows)],
            is_motion: false,
        },
        AsciiLabPattern {
            name: "Moving Vertical Bar",
            slug: "moving_vertical_bar",
            frames: generate_moving_vertical_bar(cols, rows),
            is_motion: true,
        },
    ]
}

fn ascii_lab_modes() -> [AsciiLabModeSpec; 5] {
    [
        AsciiLabModeSpec {
            slug: "temporal_none__dither_none",
            config: AsciiLabModeConfig {
                temporal: "none",
                dither: "none",
                band: None,
            },
            temporal_mode: AsciiTemporalMode::None,
            dither_mode: AsciiDitherMode::None,
        },
        AsciiLabModeSpec {
            slug: "temporal_none__dither_fs",
            config: AsciiLabModeConfig {
                temporal: "none",
                dither: "FS",
                band: None,
            },
            temporal_mode: AsciiTemporalMode::None,
            dither_mode: AsciiDitherMode::FloydSteinbergCell,
        },
        AsciiLabModeSpec {
            slug: "temporal_hysteresis_band_8__dither_none",
            config: AsciiLabModeConfig {
                temporal: "hysteresis",
                dither: "none",
                band: Some(8),
            },
            temporal_mode: AsciiTemporalMode::Hysteresis { band: 8 },
            dither_mode: AsciiDitherMode::None,
        },
        AsciiLabModeSpec {
            slug: "temporal_hysteresis_band_16__dither_none",
            config: AsciiLabModeConfig {
                temporal: "hysteresis",
                dither: "none",
                band: Some(16),
            },
            temporal_mode: AsciiTemporalMode::Hysteresis { band: 16 },
            dither_mode: AsciiDitherMode::None,
        },
        AsciiLabModeSpec {
            slug: "temporal_hysteresis_band_8__dither_fs",
            config: AsciiLabModeConfig {
                temporal: "hysteresis",
                dither: "FS",
                band: Some(8),
            },
            temporal_mode: AsciiTemporalMode::Hysteresis { band: 8 },
            dither_mode: AsciiDitherMode::FloydSteinbergCell,
        },
    ]
}

fn generate_horizontal_gradient(cols: u32, rows: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity((cols * rows) as usize);
    let denom = cols.saturating_sub(1).max(1);

    for _row in 0..rows {
        for col in 0..cols {
            let value = (col.saturating_mul(255) + (denom / 2)) / denom;
            output.push(value as u8);
        }
    }

    output
}

fn generate_radial_gradient(cols: u32, rows: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity((cols * rows) as usize);
    let center_x2 = cols.saturating_sub(1);
    let center_y2 = rows.saturating_sub(1);
    let mut max_dist_sq =
        u64::from(center_x2) * u64::from(center_x2) + u64::from(center_y2) * u64::from(center_y2);
    if max_dist_sq == 0 {
        max_dist_sq = 1;
    }

    for row in 0..rows {
        for col in 0..cols {
            let dx = u64::from(col.saturating_mul(2).abs_diff(center_x2));
            let dy = u64::from(row.saturating_mul(2).abs_diff(center_y2));
            let dist_sq = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
            let falloff =
                ((dist_sq.saturating_mul(255) + (max_dist_sq / 2)) / max_dist_sq).min(255);
            let value = 255_u8.saturating_sub(falloff as u8);
            output.push(value);
        }
    }

    output
}

fn generate_checkerboard(cols: u32, rows: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let value = if (row + col) % 2 == 0 { 0 } else { 255 };
            output.push(value);
        }
    }
    output
}

fn generate_vertical_edge(cols: u32, rows: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity((cols * rows) as usize);
    let edge = cols / 2;
    for _row in 0..rows {
        for col in 0..cols {
            let value = if col < edge { 32 } else { 224 };
            output.push(value);
        }
    }
    output
}

fn generate_moving_vertical_bar(cols: u32, rows: u32) -> Vec<Vec<u8>> {
    let bar_width = (cols / 10).max(1);
    let centers = [cols / 4, cols / 2, cols.saturating_mul(3) / 4];
    centers
        .into_iter()
        .map(|center| generate_vertical_bar_frame(cols, rows, center, bar_width))
        .collect()
}

fn generate_vertical_bar_frame(cols: u32, rows: u32, center: u32, width: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity((cols * rows) as usize);
    let half_width = width / 2;
    for _row in 0..rows {
        for col in 0..cols {
            let value = if col.abs_diff(center) <= half_width {
                240
            } else {
                24
            };
            output.push(value);
        }
    }
    output
}

fn ascii_lab_mode_line(mode: AsciiLabModeConfig) -> String {
    match mode.band {
        Some(band) => format!(
            "Mode: temporal={}, dither={}, band={band}",
            mode.temporal, mode.dither
        ),
        None => format!("Mode: temporal={}, dither={}", mode.temporal, mode.dither),
    }
}

fn ascii_lab_mode_output_text(
    sequence: &AsciiLabSequenceResult,
    mode: AsciiLabModeConfig,
    is_motion: bool,
) -> Result<String> {
    let mut output = String::new();
    output.push_str(&ascii_lab_mode_line(mode));
    output.push('\n');

    if is_motion {
        for (index, frame) in sequence.frames.iter().enumerate() {
            output.push_str(&format!(
                "Frame {index} Hash: {}\n",
                format_ascii_hash(frame.frame_hash)
            ));
            output.push_str(&ascii_frame_chars_to_text(
                &frame.frame_chars,
                sequence.cols,
                sequence.rows,
            )?);
            output.push('\n');
        }
        output.push_str(&format!(
            "Canonical Sequence Hash: {}",
            format_ascii_hash(sequence.sequence_hash)
        ));
    } else {
        let frame = sequence
            .frames
            .first()
            .context("ascii lab expected a single frame for static pattern")?;
        output.push_str(&format!("Hash: {}\n", format_ascii_hash(frame.frame_hash)));
        output.push_str(&ascii_frame_chars_to_text(
            &frame.frame_chars,
            sequence.cols,
            sequence.rows,
        )?);
    }

    Ok(output)
}

fn ascii_frame_chars_to_text(frame_chars: &[u8], cols: u32, rows: u32) -> Result<String> {
    let cols = cols as usize;
    let rows = rows as usize;
    let expected_len = cols
        .checked_mul(rows)
        .context("ascii lab frame dimensions overflow")?;
    if frame_chars.len() != expected_len {
        bail!(
            "ascii lab frame char count mismatch (expected {}, got {})",
            expected_len,
            frame_chars.len()
        );
    }

    let mut output = String::with_capacity(frame_chars.len() + rows.saturating_sub(1));
    for row in 0..rows {
        let start = row * cols;
        let end = start + cols;
        let row_text = std::str::from_utf8(&frame_chars[start..end])
            .context("ascii lab frame contains non-utf8 characters")?;
        output.push_str(row_text);
        if row + 1 < rows {
            output.push('\n');
        }
    }
    Ok(output)
}

fn export_ascii_lab_mode(
    export_root: &Path,
    pattern: &AsciiLabPattern,
    mode: AsciiLabModeSpec,
    sequence: &AsciiLabSequenceResult,
    debug_stage_hashes: bool,
) -> Result<()> {
    let output_text = ascii_lab_mode_output_text(sequence, mode.config, pattern.is_motion)?;
    let file_stem = format!("{}_{}", pattern.slug, mode.slug);
    let txt_path = export_root.join(format!("{file_stem}.txt"));
    let json_path = export_root.join(format!("{file_stem}.json"));

    fs::write(&txt_path, format!("{output_text}\n"))
        .with_context(|| format!("failed to write ascii lab output {}", txt_path.display()))?;

    let frame_hashes = sequence
        .frames
        .iter()
        .map(|frame| format_ascii_hash(frame.frame_hash))
        .collect::<Vec<_>>();
    let stage_hashes = collect_ascii_lab_stage_hashes(sequence, debug_stage_hashes)?;

    let metadata = AsciiLabExportMetadata {
        pattern: pattern.name.to_owned(),
        cols: sequence.cols,
        rows: sequence.rows,
        mode: mode.config,
        frame_hash: if pattern.is_motion {
            None
        } else {
            frame_hashes.first().cloned()
        },
        frame_hashes: if pattern.is_motion {
            Some(frame_hashes)
        } else {
            None
        },
        canonical_sequence_hash: if pattern.is_motion {
            Some(format_ascii_hash(sequence.sequence_hash))
        } else {
            None
        },
        stage_hashes,
    };

    let json =
        serde_json::to_string_pretty(&metadata).context("failed to encode ascii lab metadata")?;
    fs::write(&json_path, format!("{json}\n"))
        .with_context(|| format!("failed to write ascii lab metadata {}", json_path.display()))?;

    Ok(())
}

fn collect_ascii_lab_stage_hashes(
    sequence: &AsciiLabSequenceResult,
    debug_stage_hashes: bool,
) -> Result<Option<Vec<AsciiLabStageHashMetadata>>> {
    if !debug_stage_hashes {
        return Ok(None);
    }

    let mut output = Vec::with_capacity(sequence.frames.len());
    for (index, frame) in sequence.frames.iter().enumerate() {
        let Some(stage) = frame.stage_hashes else {
            bail!("ascii lab stage hashes missing for frame {index}");
        };
        let frame_index = u32::try_from(index).context("ascii lab frame index overflow")?;
        output.push(AsciiLabStageHashMetadata {
            frame_index,
            luma_grid_hash: format_ascii_hash(stage.luma_grid_hash),
            mapped_grid_hash: format_ascii_hash(stage.mapped_grid_hash),
            frame_chars_hash: format_ascii_hash(stage.frame_chars_hash),
        });
    }

    Ok(Some(output))
}

fn format_ascii_hash(hash: u64) -> String {
    format!("0x{hash:016x}")
}

fn run_check(manifest_path: &Path, set_values: &[String], quiet: bool) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;

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
    print_active_params(&manifest, quiet);
    Ok(())
}

fn run_lint(manifest_path: &Path, set_values: &[String], quiet: bool) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
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
        print_active_params(&manifest, quiet);
        println!("Lint OK: no issues found in {}", manifest_path.display());
        return Ok(());
    }

    if agent_mode_enabled() {
        bail!("{}", issues.join(" "));
    }

    eprintln!("Lint found {} issue(s):", issues.len());
    for issue in &issues {
        eprintln!("- {issue}");
    }
    bail!("lint failed for {}", manifest_path.display())
}

fn run_dump(
    manifest_path: &Path,
    frame: Option<u32>,
    time: Option<f32>,
    set_values: &[String],
    quiet: bool,
) -> Result<()> {
    if frame.is_some() && time.is_some() {
        bail!("use either --frame or --time, not both");
    }

    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
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
    print_active_params(&manifest, quiet);
    Ok(())
}

fn run_params(manifest_path: &Path, json: bool) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, &[])?;
    if json {
        let params = manifest
            .param_definitions
            .iter()
            .map(|(name, definition)| {
                (
                    name.clone(),
                    ParamDefinitionJson {
                        param_type: param_type_label(definition.param_type),
                        default: definition.default.clone(),
                        min: definition.min,
                        max: definition.max,
                        description: definition.description.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let payload = ParamsJsonOutput {
            manifest: manifest_path.display().to_string(),
            params,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).context("failed to encode params json")?
        );
        return Ok(());
    }

    if manifest.param_definitions.is_empty() {
        println!("No params defined in {}", manifest_path.display());
        return Ok(());
    }

    println!("Params for {}:", manifest_path.display());
    for (name, definition) in &manifest.param_definitions {
        let mut parts = Vec::new();
        parts.push(format!("type={}", param_type_label(definition.param_type)));
        parts.push(format!(
            "default={}",
            format_param_value(&definition.default)
        ));
        if let Some(min) = definition.min {
            parts.push(format!("min={min}"));
        }
        if let Some(max) = definition.max {
            parts.push(format!("max={max}"));
        }
        if let Some(description) = &definition.description {
            parts.push(format!("description={description}"));
        }
        println!("- {}: {}", name, parts.join(", "));
    }

    Ok(())
}

fn run_explain(manifest_path: &Path, set_values: &[String], json: bool) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
    if json {
        let payload = ExplainJsonOutput {
            manifest: manifest_path.display().to_string(),
            manifest_hash: manifest.manifest_hash.clone(),
            environment: ExplainEnvironmentJson {
                width: manifest.environment.resolution.width,
                height: manifest.environment.resolution.height,
                fps: manifest.environment.fps,
                frames: manifest.environment.total_frames(),
            },
            overrides: manifest.applied_param_overrides.clone(),
            resolved_params: manifest.resolved_params.clone(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).context("failed to encode explain json")?
        );
        return Ok(());
    }

    println!("Explain {}", manifest_path.display());
    println!("- manifest_hash={}", manifest.manifest_hash);
    println!(
        "- environment={}x{} @ {} fps, {} frames",
        manifest.environment.resolution.width,
        manifest.environment.resolution.height,
        manifest.environment.fps,
        manifest.environment.total_frames()
    );
    let non_default_overrides = manifest
        .applied_param_overrides
        .iter()
        .filter(|(name, value)| {
            manifest
                .param_definitions
                .get(*name)
                .map_or(true, |definition| definition.default != **value)
        })
        .collect::<Vec<_>>();
    if non_default_overrides.is_empty() {
        println!("- overrides=<none>");
    } else {
        println!("- overrides (non-default):");
        for (name, value) in non_default_overrides {
            println!("  {}={}", name, format_param_value(value));
        }
    }
    let non_default_resolved = manifest
        .resolved_params
        .iter()
        .filter(|(name, value)| {
            manifest
                .param_definitions
                .get(*name)
                .map_or(true, |definition| definition.default != **value)
        })
        .collect::<Vec<_>>();
    if non_default_resolved.is_empty() {
        println!("- resolved_non_default_params=<none>");
    } else {
        println!("- resolved_non_default_params:");
    }
    for (name, value) in non_default_resolved {
        println!("  {}={}", name, format_param_value(value));
    }
    println!("- resolved_param_total={}", manifest.resolved_params.len());
    Ok(())
}

fn create_renderer(
    environment: &Environment,
    layers: &[vcr::schema::Layer],
    scene: RenderSceneData,
    backend: BackendArg,
) -> Result<Renderer> {
    match backend {
        BackendArg::Software => Renderer::new_software(environment, layers, scene),
        BackendArg::Gpu | BackendArg::Auto => {
            pollster::block_on(Renderer::new_with_scene(environment, layers, scene))
        }
    }
}

fn run_build(
    manifest_path: &Path,
    output_path: &Path,
    args: FrameWindowArgs,
    set_values: &[String],
    ascii_overrides: Option<AsciiRuntimeOverrides>,
    backend: BackendArg,
    quiet: bool,
) -> Result<()> {
    let parse_start = Instant::now();
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
    let parse_elapsed = parse_start.elapsed();
    let total_frames = manifest.environment.total_frames();
    let window = resolve_frame_window(total_frames, args)?;

    let layout_start = Instant::now();
    let mut scene = RenderSceneData::from_manifest(&manifest);
    if let Some(overrides) = ascii_overrides {
        scene = scene.with_ascii_overrides(overrides);
    }
    let mut renderer = create_renderer(&manifest.environment, &manifest.layers, scene, backend)?;
    let layout_elapsed = layout_start.elapsed();
    progress_log(
        quiet,
        format_args!(
            "[VCR] Build: {}x{}, {} fps, {} frames",
            manifest.environment.resolution.width,
            manifest.environment.resolution.height,
            manifest.environment.fps,
            window.count
        ),
    );
    print_active_params(&manifest, quiet);
    progress_log(
        quiet,
        format_args!(
            "[VCR] Backend: {} ({})",
            renderer.backend_name(),
            renderer.backend_reason()
        ),
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
            progress_log(
                quiet,
                format_args!("rendered frame {}/{}", offset + 1, window.count),
            );
        }
    }

    ffmpeg.finish()?;
    println!("Wrote {}", output_path.display());
    let metadata_path = metadata_sidecar_for_file(output_path);
    emit_render_metadata(
        &metadata_path,
        &manifest,
        &manifest.environment,
        renderer.backend_name(),
        renderer.backend_reason(),
        window,
    )?;
    println!("Wrote {}", metadata_path.display());
    print_timing_summary(
        quiet,
        RenderTimingSummary {
            parse: parse_elapsed,
            layout: layout_elapsed,
            render: render_elapsed,
            encode: encode_elapsed,
        },
    );
    Ok(())
}

fn run_preview(
    manifest_path: &Path,
    output: Option<&Path>,
    args: PreviewArgs,
    set_values: &[String],
    ascii_overrides: Option<&AsciiRuntimeOverrides>,
    backend: BackendArg,
    quiet: bool,
) -> Result<ResolvedInputsSnapshot> {
    if !(0.0..=1.0).contains(&args.scale) || args.scale == 0.0 {
        bail!("preview --scale must be in (0, 1], got {}", args.scale);
    }

    let parse_start = Instant::now();
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
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
    let mut scene = RenderSceneData::from_manifest(&manifest);
    if let Some(overrides) = ascii_overrides {
        scene = scene.with_ascii_overrides(overrides.clone());
    }
    let mut renderer = create_renderer(&preview_environment, &manifest.layers, scene, backend)?;
    let layout_elapsed = layout_start.elapsed();

    progress_log(
        quiet,
        format_args!(
            "[VCR] Preview: {}x{}, {} fps, frames {}..{} ({} total)",
            preview_environment.resolution.width,
            preview_environment.resolution.height,
            preview_environment.fps,
            window.start_frame,
            window.start_frame + window.count.saturating_sub(1),
            window.count
        ),
    );
    print_active_params(&manifest, quiet);
    progress_log(
        quiet,
        format_args!(
            "[VCR] Backend: {} ({})",
            renderer.backend_name(),
            renderer.backend_reason()
        ),
    );

    let mut render_elapsed = Duration::ZERO;
    let mut encode_elapsed = Duration::ZERO;
    let metadata_path = if args.image_sequence {
        let output_dir = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_preview_sequence_output_dir(manifest_path));
        metadata_sidecar_for_directory(&output_dir, "preview")
    } else {
        let output_path = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("preview.mov"));
        metadata_sidecar_for_file(&output_path)
    };
    if args.image_sequence {
        let output_dir = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_preview_sequence_output_dir(manifest_path));
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

    emit_render_metadata(
        &metadata_path,
        &manifest,
        &preview_environment,
        renderer.backend_name(),
        renderer.backend_reason(),
        window,
    )?;
    println!("Wrote {}", metadata_path.display());

    print_timing_summary(
        quiet,
        RenderTimingSummary {
            parse: parse_elapsed,
            layout: layout_elapsed,
            render: render_elapsed,
            encode: encode_elapsed,
        },
    );
    Ok(ResolvedInputsSnapshot::from_manifest(&manifest))
}

fn run_render_frame(
    manifest_path: &Path,
    frame_index: u32,
    output_path: &Path,
    set_values: &[String],
    ascii_overrides: Option<&AsciiRuntimeOverrides>,
    backend: BackendArg,
    quiet: bool,
) -> Result<()> {
    let parse_start = Instant::now();
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
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
    let mut scene = RenderSceneData::from_manifest(&manifest);
    if let Some(overrides) = ascii_overrides {
        scene = scene.with_ascii_overrides(overrides.clone());
    }
    let mut renderer = create_renderer(&manifest.environment, &manifest.layers, scene, backend)?;
    let layout_elapsed = layout_start.elapsed();
    progress_log(
        quiet,
        format_args!(
            "[VCR] Backend: {} ({})",
            renderer.backend_name(),
            renderer.backend_reason()
        ),
    );
    print_active_params(&manifest, quiet);

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
    let window = FrameWindow {
        start_frame: frame_index,
        count: 1,
    };
    let metadata_path = metadata_sidecar_for_file(output_path);
    emit_render_metadata(
        &metadata_path,
        &manifest,
        &manifest.environment,
        renderer.backend_name(),
        renderer.backend_reason(),
        window,
    )?;
    println!("Wrote {}", metadata_path.display());
    print_timing_summary(
        quiet,
        RenderTimingSummary {
            parse: parse_elapsed,
            layout: layout_elapsed,
            render: render_elapsed,
            encode: encode_elapsed,
        },
    );
    Ok(())
}

fn run_render_frames(
    manifest_path: &Path,
    start_frame: u32,
    frames: u32,
    output_dir: &Path,
    set_values: &[String],
    ascii_overrides: Option<&AsciiRuntimeOverrides>,
    backend: BackendArg,
    quiet: bool,
) -> Result<()> {
    if frames == 0 {
        bail!("--frames must be > 0");
    }

    let parse_start = Instant::now();
    let manifest = load_manifest_with_overrides(manifest_path, set_values)?;
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
    let mut scene = RenderSceneData::from_manifest(&manifest);
    if let Some(overrides) = ascii_overrides {
        scene = scene.with_ascii_overrides(overrides.clone());
    }
    let mut renderer = create_renderer(&manifest.environment, &manifest.layers, scene, backend)?;
    let layout_elapsed = layout_start.elapsed();
    progress_log(
        quiet,
        format_args!(
            "[VCR] Backend: {} ({})",
            renderer.backend_name(),
            renderer.backend_reason()
        ),
    );
    print_active_params(&manifest, quiet);

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
    let metadata_path = metadata_sidecar_for_directory(output_dir, "frames");
    emit_render_metadata(
        &metadata_path,
        &manifest,
        &manifest.environment,
        renderer.backend_name(),
        renderer.backend_reason(),
        window,
    )?;
    println!("Wrote {}", metadata_path.display());
    print_timing_summary(
        quiet,
        RenderTimingSummary {
            parse: parse_elapsed,
            layout: layout_elapsed,
            render: render_elapsed,
            encode: encode_elapsed,
        },
    );
    Ok(())
}

fn run_watch(
    manifest_path: &Path,
    output: Option<PathBuf>,
    preview_args: PreviewArgs,
    interval_ms: u64,
    set_values: &[String],
    ascii_overrides: Option<&AsciiRuntimeOverrides>,
    backend: BackendArg,
    quiet: bool,
) -> Result<()> {
    if interval_ms == 0 {
        bail!("--interval-ms must be > 0");
    }

    progress_log(
        quiet,
        format_args!(
            "[VCR] watch: monitoring {} every {}ms (Ctrl-C to stop)",
            manifest_path.display(),
            interval_ms
        ),
    );

    let mut last_stamp = read_file_stamp(manifest_path)?;
    let mut last_inputs = match run_preview(
        manifest_path,
        output.as_deref(),
        preview_args.clone(),
        set_values,
        ascii_overrides,
        backend,
        quiet,
    ) {
        Ok(inputs) => Some(inputs),
        Err(error) => {
            eprintln!("[VCR] watch: initial render failed: {error:#}");
            None
        }
    };

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
            progress_log(
                quiet,
                format_args!(
                    "[VCR] watch: change detected in {}, rebuilding preview...",
                    manifest_path.display()
                ),
            );
            last_stamp = stamp;
            match run_preview(
                manifest_path,
                output.as_deref(),
                preview_args.clone(),
                set_values,
                ascii_overrides,
                backend,
                quiet,
            ) {
                Ok(current_inputs) => {
                    if should_emit_nonessential_logs(quiet) {
                        if let Some(previous_inputs) = &last_inputs {
                            print_inputs_diff(previous_inputs, &current_inputs);
                        }
                    }
                    last_inputs = Some(current_inputs);
                }
                Err(error) => {
                    eprintln!("[VCR] watch: rebuild failed: {error:#}");
                }
            }
        }
    }
}

fn load_manifest_with_overrides(manifest_path: &Path, set_values: &[String]) -> Result<Manifest> {
    let overrides = set_values
        .iter()
        .map(|entry| {
            ParamOverride::parse(entry)
                .with_context(|| format!("invalid --set override '{}'", entry))
        })
        .collect::<Result<Vec<_>>>()?;
    load_and_validate_manifest_with_options(manifest_path, &ManifestLoadOptions { overrides })
}

fn should_emit_nonessential_logs(quiet: bool) -> bool {
    !quiet
}

fn progress_log(quiet: bool, args: std::fmt::Arguments<'_>) {
    if should_emit_nonessential_logs(quiet) {
        eprintln!("{args}");
    }
}

fn print_active_params(manifest: &Manifest, quiet: bool) {
    if !should_emit_nonessential_logs(quiet) {
        return;
    }
    if manifest.resolved_params.is_empty() {
        eprintln!("[VCR] Params: <none>");
        return;
    }

    eprintln!("[VCR] Params (resolved):");
    for (name, value) in &manifest.resolved_params {
        eprintln!("  {} = {}", name, format_param_value(value));
    }

    if manifest.applied_param_overrides.is_empty() {
        eprintln!("[VCR] Param overrides: <none>");
    } else {
        eprintln!("[VCR] Param overrides:");
        for (name, value) in &manifest.applied_param_overrides {
            eprintln!("  {} = {}", name, format_param_value(value));
        }
    }
}

fn format_param_value(value: &ParamValue) -> String {
    match value {
        ParamValue::Float(number) => format!("{number:.6}"),
        ParamValue::Int(number) => number.to_string(),
        ParamValue::Bool(flag) => flag.to_string(),
        ParamValue::Vec2(vec) => format!("[{:.6}, {:.6}]", vec.x, vec.y),
        ParamValue::Color(color) => format!(
            "{{r: {:.6}, g: {:.6}, b: {:.6}, a: {:.6}}}",
            color.r, color.g, color.b, color.a
        ),
    }
}

fn param_type_label(param_type: ParamType) -> &'static str {
    match param_type {
        ParamType::Float => "float",
        ParamType::Int => "int",
        ParamType::Color => "color",
        ParamType::Vec2 => "vec2",
        ParamType::Bool => "bool",
    }
}

#[derive(Debug, Serialize)]
struct ParamDefinitionJson {
    #[serde(rename = "type")]
    param_type: &'static str,
    default: ParamValue,
    min: Option<f32>,
    max: Option<f32>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct ParamsJsonOutput {
    manifest: String,
    params: BTreeMap<String, ParamDefinitionJson>,
}

#[derive(Debug, Serialize)]
struct ExplainEnvironmentJson {
    width: u32,
    height: u32,
    fps: u32,
    frames: u32,
}

#[derive(Debug, Serialize)]
struct ExplainJsonOutput {
    manifest: String,
    manifest_hash: String,
    environment: ExplainEnvironmentJson,
    overrides: BTreeMap<String, ParamValue>,
    resolved_params: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Clone)]
struct ResolvedInputsSnapshot {
    manifest_hash: String,
    resolved_params: BTreeMap<String, ParamValue>,
    applied_overrides: BTreeMap<String, ParamValue>,
}

impl ResolvedInputsSnapshot {
    fn from_manifest(manifest: &Manifest) -> Self {
        Self {
            manifest_hash: manifest.manifest_hash.clone(),
            resolved_params: manifest.resolved_params.clone(),
            applied_overrides: manifest.applied_param_overrides.clone(),
        }
    }
}

fn print_inputs_diff(previous: &ResolvedInputsSnapshot, current: &ResolvedInputsSnapshot) {
    if previous.manifest_hash != current.manifest_hash {
        eprintln!(
            "[VCR] watch: manifest hash changed {} -> {}",
            previous.manifest_hash, current.manifest_hash
        );
    }

    let mut keys = BTreeSet::new();
    keys.extend(previous.resolved_params.keys().cloned());
    keys.extend(current.resolved_params.keys().cloned());

    let mut changed = false;
    for key in keys {
        let before = previous.resolved_params.get(&key);
        let after = current.resolved_params.get(&key);
        if before != after {
            if !changed {
                eprintln!("[VCR] watch: param diff:");
                changed = true;
            }
            let before_label = before
                .map(format_param_value)
                .unwrap_or_else(|| "<unset>".to_owned());
            let after_label = after
                .map(format_param_value)
                .unwrap_or_else(|| "<unset>".to_owned());
            eprintln!("  {}: {} -> {}", key, before_label, after_label);
        }
    }
    if previous.applied_overrides != current.applied_overrides {
        eprintln!("[VCR] watch: active --set overrides changed");
    }
}

#[derive(Debug, Serialize)]
struct RenderMetadata {
    manifest_hash: String,
    resolved_manifest_hash: String,
    vcr_version: String,
    backend: String,
    backend_reason: String,
    resolution: RenderMetadataResolution,
    fps: u32,
    frame_count: u32,
    start_frame: u32,
    end_frame: u32,
    resolved_params: BTreeMap<String, ParamValue>,
    overrides: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Serialize)]
struct RenderMetadataResolution {
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
struct RenderManifestHashMaterial<'a> {
    resolved_manifest_hash: &'a str,
    resolved_params: &'a BTreeMap<String, ParamValue>,
    overrides: &'a BTreeMap<String, ParamValue>,
    start_frame: u32,
    frame_count: u32,
    end_frame: u32,
}

fn compute_render_manifest_hash(
    resolved_manifest_hash: &str,
    resolved_params: &BTreeMap<String, ParamValue>,
    overrides: &BTreeMap<String, ParamValue>,
    window: FrameWindow,
) -> Result<String> {
    let material = RenderManifestHashMaterial {
        resolved_manifest_hash,
        resolved_params,
        overrides,
        start_frame: window.start_frame,
        frame_count: window.count,
        end_frame: window.start_frame + window.count.saturating_sub(1),
    };
    let encoded =
        serde_json::to_vec(&material).context("failed to serialize metadata hash material")?;
    Ok(format!("{:016x}", fnv1a64(&encoded)))
}

fn emit_render_metadata(
    metadata_path: &Path,
    manifest: &Manifest,
    environment: &Environment,
    backend_name: &str,
    backend_reason: &str,
    window: FrameWindow,
) -> Result<()> {
    if let Some(parent) = metadata_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create metadata directory {}", parent.display()))?;
    }

    let resolved_manifest_hash = manifest.manifest_hash.clone();
    let render_manifest_hash = compute_render_manifest_hash(
        &resolved_manifest_hash,
        &manifest.resolved_params,
        &manifest.applied_param_overrides,
        window,
    )?;

    let metadata = RenderMetadata {
        manifest_hash: render_manifest_hash,
        resolved_manifest_hash,
        vcr_version: env!("CARGO_PKG_VERSION").to_owned(),
        backend: backend_name.to_owned(),
        backend_reason: backend_reason.to_owned(),
        resolution: RenderMetadataResolution {
            width: environment.resolution.width,
            height: environment.resolution.height,
        },
        fps: environment.fps,
        frame_count: window.count,
        start_frame: window.start_frame,
        end_frame: window.start_frame + window.count.saturating_sub(1),
        resolved_params: manifest.resolved_params.clone(),
        overrides: manifest.applied_param_overrides.clone(),
    };

    let payload =
        serde_json::to_string_pretty(&metadata).context("failed to encode render metadata")?;
    fs::write(metadata_path, format!("{payload}\n"))
        .with_context(|| format!("failed to write metadata {}", metadata_path.display()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

fn metadata_sidecar_for_file(output_path: &Path) -> PathBuf {
    let parent = output_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = output_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "render".to_owned());
    parent.join(format!("{file_name}.metadata.json"))
}

fn metadata_sidecar_for_directory(output_dir: &Path, label: &str) -> PathBuf {
    output_dir.join(format!("{label}.metadata.json"))
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
        resolution: Resolution {
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

fn print_timing_summary(quiet: bool, timing: RenderTimingSummary) {
    if !should_emit_nonessential_logs(quiet) {
        return;
    }
    let total = timing.parse + timing.layout + timing.render + timing.encode;
    eprintln!(
        "[VCR] timing parse={:.2?} layout={:.2?} render={:.2?} encode={:.2?} total={:.2?}",
        timing.parse, timing.layout, timing.render, timing.encode, total
    );
}

#[cfg(test)]
mod tests {
    use super::{
        classify_exit_code, resolve_ascii_stage_options, should_emit_nonessential_logs,
        AsciiPresetArg, VcrExitCode,
    };
    use anyhow::anyhow;

    #[test]
    fn quiet_mode_log_gate_is_predictable() {
        assert!(should_emit_nonessential_logs(false));
        assert!(!should_emit_nonessential_logs(true));
    }

    #[test]
    fn exit_code_classifier_maps_usage_errors() {
        let error = anyhow!("invalid --set for param 'speed': expected float, got 'fast'");
        assert_eq!(classify_exit_code(&error), VcrExitCode::Usage);
    }

    #[test]
    fn ascii_preset_defaults_apply_but_explicit_flags_win() {
        let x_defaults = resolve_ascii_stage_options(AsciiPresetArg::X, None, None, None, None);
        assert_eq!(x_defaults.fps, 30);
        assert_eq!(x_defaults.size, "1080x1920");
        assert!((x_defaults.speed - 1.2).abs() < f32::EPSILON);

        let explicit = resolve_ascii_stage_options(
            AsciiPresetArg::X,
            Some(24),
            Some("1280x720"),
            Some(0.9),
            Some("void"),
        );
        assert_eq!(explicit.fps, 24);
        assert_eq!(explicit.size, "1280x720");
        assert!((explicit.speed - 0.9).abs() < f32::EPSILON);
        assert_eq!(explicit.theme, "void");
    }
}
