use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};

use crate::ascii_frame::{AsciiFrame, AsciiFrameMetadata};
use crate::ascii_sources::{ascii_live_stream_names, ascii_live_stream_url, library_source_names};

pub const DEFAULT_CAPTURE_FPS: u32 = 30;
pub const DEFAULT_CAPTURE_DURATION_SECONDS: f32 = 5.0;
pub const DEFAULT_CAPTURE_COLS: u32 = 80;
pub const DEFAULT_CAPTURE_ROWS: u32 = 40;
pub const DEFAULT_CAPTURE_FONT_SIZE: f32 = 16.0;
pub const DEFAULT_CAPTURE_FIT_PADDING: f32 = 0.12;

const DEFAULT_CAPTURE_FONT_PATH_REL: &str = "assets/fonts/geist_pixel/GeistPixel-Line.ttf";
const SOURCE_RECV_POLL_MS: u64 = 20;
const SOURCE_SYMBOL_RAMP: &str =
    " .'`^\",:;Il!i~+_-?][}{1)(|\\/*tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";
const DEFAULT_TARGET_SYMBOL_RAMP: &str = " .,:;iltfrxnuvczXYUJCLQOZmwqpdbkhao*#MW&@$";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolRemapMode {
    None,
    Density,
    Equalize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiCaptureSource {
    AsciiLive { stream: String },
    Library { id: String },
    Chafa { input: PathBuf },
}

impl AsciiCaptureSource {
    pub fn parse(raw: &str) -> Result<Self> {
        let value = raw.trim();
        if let Some(stream) = value.strip_prefix("ascii-live:") {
            let stream = stream.trim();
            if stream.is_empty() {
                bail!("invalid --source '{}': missing ascii-live stream name", raw);
            }
            return Ok(Self::AsciiLive {
                stream: stream.to_owned(),
            });
        }

        if let Some(path) = value.strip_prefix("chafa:") {
            let path = path.trim();
            if path.is_empty() {
                bail!("invalid --source '{}': missing chafa input path", raw);
            }
            return Ok(Self::Chafa {
                input: PathBuf::from(path),
            });
        }

        if let Some(id) = value.strip_prefix("library:") {
            let id = id.trim().to_ascii_lowercase();
            if id.is_empty() {
                bail!("invalid --source '{}': missing library source id", raw);
            }
            return Ok(Self::Library { id });
        }

        bail!(
            "invalid --source '{}': expected 'ascii-live:<stream>', 'library:<id>', or 'chafa:<path>'",
            raw
        )
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::AsciiLive { stream } => format!("ascii-live:{stream}"),
            Self::Library { id } => format!("library:{id}"),
            Self::Chafa { input } => format!("chafa:{}", input.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AsciiCaptureArgs {
    pub source: AsciiCaptureSource,
    pub output: PathBuf,
    pub fps: u32,
    pub duration_seconds: f32,
    pub max_frames: Option<u32>,
    pub cols: u32,
    pub rows: u32,
    pub font_path: Option<PathBuf>,
    pub font_size: f32,
    pub tmp_dir: Option<PathBuf>,
    pub debug_txt_dir: Option<PathBuf>,
    pub symbol_remap: SymbolRemapMode,
    pub symbol_ramp: Option<String>,
    pub fit_padding: f32,
}

#[derive(Debug, Clone)]
pub struct AsciiCapturePlan {
    pub source_label: String,
    pub source_command: Vec<String>,
    pub output: PathBuf,
    pub fps: u32,
    pub duration_seconds: f32,
    pub frame_count: u32,
    pub cols: u32,
    pub rows: u32,
    pub font_path: PathBuf,
    pub font_size: f32,
    pub tmp_dir: Option<PathBuf>,
    pub parser_mode: &'static str,
    pub ffmpeg_encoder: &'static str,
    pub symbol_remap: SymbolRemapMode,
    pub symbol_ramp: String,
    pub fit_padding: f32,
}

#[derive(Debug, Clone)]
pub struct AsciiCaptureSummary {
    pub output: PathBuf,
    pub frame_count: u32,
    pub fps: u32,
    pub cols: u32,
    pub rows: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

pub fn parse_capture_size(raw: &str) -> Result<(u32, u32)> {
    let value = raw.trim();
    let (cols_raw, rows_raw) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .ok_or_else(|| anyhow!("invalid --size '{}': expected COLSxROWS", raw))?;
    let cols = cols_raw
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid --size '{}': cols must be an integer", raw))?;
    let rows = rows_raw
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid --size '{}': rows must be an integer", raw))?;
    if cols == 0 || rows == 0 {
        bail!("invalid --size '{}': cols/rows must be > 0", raw);
    }
    Ok((cols, rows))
}

pub fn build_ascii_capture_plan(args: &AsciiCaptureArgs) -> Result<AsciiCapturePlan> {
    validate_capture_args(args)?;
    let frame_count = resolve_target_frame_count(args.fps, args.duration_seconds, args.max_frames)?;
    let source_command = source_command_preview(&args.source, args.cols, args.rows)?;
    let font_path = resolve_font_path(args.font_path.as_deref())?;
    let symbol_ramp = resolve_target_symbol_ramp(args.symbol_ramp.as_deref())?;
    Ok(AsciiCapturePlan {
        source_label: args.source.display_label(),
        source_command,
        output: args.output.clone(),
        fps: args.fps,
        duration_seconds: args.duration_seconds,
        frame_count,
        cols: args.cols,
        rows: args.rows,
        font_path,
        font_size: args.font_size,
        tmp_dir: args.tmp_dir.clone(),
        parser_mode: "best-effort ANSI parser with sampled latest-frame fallback",
        ffmpeg_encoder: "ffmpeg -c:v prores_ks -profile:v 2 -pix_fmt yuv422p10le",
        symbol_remap: args.symbol_remap,
        symbol_ramp,
        fit_padding: args.fit_padding,
    })
}

pub fn run_ascii_capture(args: &AsciiCaptureArgs) -> Result<AsciiCaptureSummary> {
    let plan = build_ascii_capture_plan(args)?;
    let raw_frames = capture_ascii_frames(
        &args.source,
        plan.frame_count,
        plan.fps,
        plan.cols,
        plan.rows,
    )
    .with_context(|| format!("failed to capture source '{}'", plan.source_label))?;

    let frames = fit_frames_to_canvas(
        raw_frames,
        plan.cols as usize,
        plan.rows as usize,
        plan.fit_padding,
    );
    let frames = remap_frames_symbols(frames, plan.symbol_remap, &plan.symbol_ramp);

    if let Some(debug_dir) = &args.debug_txt_dir {
        write_debug_ascii_frames(debug_dir, &frames)?;
    }

    if let Some(parent) = plan.output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    if let Some(tmp_dir) = &plan.tmp_dir {
        fs::create_dir_all(tmp_dir)
            .with_context(|| format!("failed to create tmp directory {}", tmp_dir.display()))?;
    }

    let mut rasterizer = AsciiFrameRasterizer::new(
        &plan.font_path,
        plan.font_size,
        plan.cols as usize,
        plan.rows as usize,
    )?;
    let mut encoder = ProRes422Encoder::spawn(
        &plan.output,
        rasterizer.pixel_width(),
        rasterizer.pixel_height(),
        plan.fps,
        plan.tmp_dir.as_deref(),
    )?;

    for frame in &frames {
        let rgba = rasterizer.render(frame);
        encoder.write_frame(&rgba)?;
    }
    encoder.finish()?;

    Ok(AsciiCaptureSummary {
        output: plan.output,
        frame_count: plan.frame_count,
        fps: plan.fps,
        cols: plan.cols,
        rows: plan.rows,
        pixel_width: rasterizer.pixel_width(),
        pixel_height: rasterizer.pixel_height(),
    })
}

fn validate_capture_args(args: &AsciiCaptureArgs) -> Result<()> {
    if args.fps == 0 {
        bail!("--fps must be > 0");
    }
    if !args.duration_seconds.is_finite() || args.duration_seconds <= 0.0 {
        bail!("--duration must be > 0");
    }
    if let Some(max_frames) = args.max_frames {
        if max_frames == 0 {
            bail!("--frames must be > 0");
        }
    }
    if args.cols == 0 || args.rows == 0 {
        bail!("--size cols/rows must be > 0");
    }
    if !args.font_size.is_finite() || args.font_size <= 0.0 {
        bail!("--font-size must be > 0");
    }
    if !args.fit_padding.is_finite() || args.fit_padding < 0.0 || args.fit_padding >= 0.5 {
        bail!("--fit-padding must be in [0.0, 0.5)");
    }
    Ok(())
}

fn resolve_target_symbol_ramp(value: Option<&str>) -> Result<String> {
    let raw = value.unwrap_or(DEFAULT_TARGET_SYMBOL_RAMP);
    let ramp = raw
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if ramp.is_empty() {
        bail!("--symbol-ramp must include at least one non-space character");
    }
    Ok(ramp)
}

fn resolve_target_frame_count(
    fps: u32,
    duration_seconds: f32,
    max_frames: Option<u32>,
) -> Result<u32> {
    if let Some(max_frames) = max_frames {
        if max_frames == 0 {
            bail!("--frames must be > 0");
        }
        return Ok(max_frames);
    }
    let computed = (duration_seconds * fps as f32).ceil() as u32;
    Ok(computed.max(1))
}

fn resolve_font_path(font_path: Option<&Path>) -> Result<PathBuf> {
    let path = font_path.map(Path::to_path_buf).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_CAPTURE_FONT_PATH_REL)
    });
    if !path.exists() {
        bail!(
            "font file '{}' does not exist; provide --font-path or install default font at {}",
            path.display(),
            DEFAULT_CAPTURE_FONT_PATH_REL
        );
    }
    Ok(path)
}

fn source_command_preview(
    source: &AsciiCaptureSource,
    cols: u32,
    rows: u32,
) -> Result<Vec<String>> {
    match source {
        AsciiCaptureSource::AsciiLive { stream } => {
            let url = ascii_live_url(stream)?;
            Ok(vec![
                "curl".to_owned(),
                "-L".to_owned(),
                "--no-buffer".to_owned(),
                url.to_owned(),
            ])
        }
        AsciiCaptureSource::Chafa { input } => {
            let mut preview = vec![
                "chafa".to_owned(),
                format!("--size={}x{}", cols, rows),
                "--colors=none".to_owned(),
            ];
            preview.extend(chafa_optional_flags());
            preview.push(input.display().to_string());
            Ok(preview)
        }
        AsciiCaptureSource::Library { id } => Ok(vec![
            "builtin-library".to_owned(),
            "render".to_owned(),
            id.clone(),
        ]),
    }
}

fn capture_ascii_frames(
    source: &AsciiCaptureSource,
    frame_count: u32,
    fps: u32,
    cols: u32,
    rows: u32,
) -> Result<Vec<AsciiFrame>> {
    if let AsciiCaptureSource::Library { id } = source {
        return capture_library_frames(id, frame_count, fps, cols, rows);
    }

    let mut child = spawn_source_process(source, cols, rows)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture source stdout"))?;
    let (receiver, worker) = spawn_stdout_worker(stdout)?;
    let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let start = Instant::now();
    let mut parser = BestEffortAnsiFrameParser::new(cols as usize, rows as usize);
    let blank = AsciiFrame::blank(cols as usize, rows as usize);
    let mut frames = Vec::with_capacity(frame_count as usize);

    for frame_index in 0..frame_count {
        let target = start + Duration::from_secs_f64(frame_index as f64 / fps as f64);
        loop {
            let now = Instant::now();
            if now >= target {
                break;
            }
            let timeout = (target - now).min(Duration::from_millis(SOURCE_RECV_POLL_MS));
            match receiver.recv_timeout(timeout) {
                Ok(chunk) => parser.push_bytes(&chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        while let Ok(chunk) = receiver.try_recv() {
            parser.push_bytes(&chunk);
        }

        let mut frame = parser.latest_frame().unwrap_or_else(|| blank.clone());
        let timestamp_ms = ((frame_index as f64 / fps as f64) * 1000.0).round() as u64;
        frame.metadata = Some(AsciiFrameMetadata {
            source_frame_index: Some(frame_index as u64),
            source_timestamp_ms: Some(timestamp_ms),
        });
        frames.push(frame);

        let deadline = start + (frame_interval * (frame_index + 1));
        if Instant::now() < deadline {
            thread::sleep(deadline - Instant::now());
        }
    }

    let status = terminate_source_process(&mut child)?;
    drop(receiver);
    match worker.join() {
        Ok(result) => result?,
        Err(_) => return Err(anyhow!("source stdout worker panicked")),
    }

    if !status.success() && parser.latest_frame().is_none() {
        bail!("source process exited with non-zero status: {status}");
    }

    Ok(frames)
}

#[derive(Debug, Clone, Copy)]
struct ContentBounds {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

impl ContentBounds {
    fn width(self) -> usize {
        self.right.saturating_sub(self.left) + 1
    }

    fn height(self) -> usize {
        self.bottom.saturating_sub(self.top) + 1
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

fn fit_frames_to_canvas(
    frames: Vec<AsciiFrame>,
    cols: usize,
    rows: usize,
    fit_padding: f32,
) -> Vec<AsciiFrame> {
    let Some(bounds) =
        robust_content_bounds(&frames, cols, rows).or_else(|| global_content_bounds(&frames))
    else {
        return frames;
    };
    let region_width = bounds.width();
    let region_height = bounds.height();
    if region_width == cols && region_height == rows {
        // Still apply safe padding if requested.
        if fit_padding <= 0.0 {
            return frames;
        }
    }

    let usable_ratio = (1.0 - (fit_padding * 2.0)).clamp(0.1, 1.0);
    let scale_x = cols as f32 / region_width as f32;
    let scale_y = rows as f32 / region_height as f32;
    let scale = scale_x.min(scale_y) * usable_ratio;
    let scaled_width = ((region_width as f32 * scale).round() as usize)
        .max(1)
        .min(cols);
    let scaled_height = ((region_height as f32 * scale).round() as usize)
        .max(1)
        .min(rows);
    let offset_x = (cols - scaled_width) / 2;
    let offset_y = (rows - scaled_height) / 2;

    frames
        .into_iter()
        .map(|frame| {
            fit_single_frame_to_bounds(
                frame,
                bounds,
                scaled_width,
                scaled_height,
                offset_x,
                offset_y,
            )
        })
        .collect()
}

fn robust_content_bounds(frames: &[AsciiFrame], cols: usize, rows: usize) -> Option<ContentBounds> {
    if frames.is_empty() || cols == 0 || rows == 0 {
        return None;
    }
    let mut row_occupancy = vec![0_usize; rows];
    let mut col_occupancy = vec![0_usize; cols];

    for frame in frames {
        for (row, line) in frame.lines().iter().take(rows).enumerate() {
            for (col, byte) in line.as_bytes().iter().take(cols).enumerate() {
                if *byte != b' ' {
                    row_occupancy[row] += 1;
                    col_occupancy[col] += 1;
                }
            }
        }
    }

    let max_row = row_occupancy.iter().copied().max().unwrap_or(0);
    let max_col = col_occupancy.iter().copied().max().unwrap_or(0);
    if max_row == 0 || max_col == 0 {
        return None;
    }

    let row_threshold = ((max_row as f32) * 0.10).ceil() as usize;
    let col_threshold = ((max_col as f32) * 0.10).ceil() as usize;
    let row_threshold = row_threshold.max(1);
    let col_threshold = col_threshold.max(1);

    let top = row_occupancy
        .iter()
        .position(|count| *count >= row_threshold)?;
    let bottom = row_occupancy
        .iter()
        .rposition(|count| *count >= row_threshold)?;
    let left = col_occupancy
        .iter()
        .position(|count| *count >= col_threshold)?;
    let right = col_occupancy
        .iter()
        .rposition(|count| *count >= col_threshold)?;

    Some(ContentBounds {
        left,
        top,
        right,
        bottom,
    })
}

fn global_content_bounds(frames: &[AsciiFrame]) -> Option<ContentBounds> {
    frames
        .iter()
        .filter_map(frame_content_bounds)
        .reduce(|acc, bounds| acc.union(bounds))
}

fn frame_content_bounds(frame: &AsciiFrame) -> Option<ContentBounds> {
    let mut left = usize::MAX;
    let mut top = usize::MAX;
    let mut right = 0_usize;
    let mut bottom = 0_usize;
    let mut found = false;

    for (row_index, line) in frame.lines().iter().enumerate() {
        for (col_index, byte) in line.as_bytes().iter().enumerate() {
            if *byte != b' ' {
                found = true;
                left = left.min(col_index);
                top = top.min(row_index);
                right = right.max(col_index);
                bottom = bottom.max(row_index);
            }
        }
    }

    if found {
        Some(ContentBounds {
            left,
            top,
            right,
            bottom,
        })
    } else {
        None
    }
}

fn fit_single_frame_to_bounds(
    frame: AsciiFrame,
    bounds: ContentBounds,
    scaled_width: usize,
    scaled_height: usize,
    offset_x: usize,
    offset_y: usize,
) -> AsciiFrame {
    let cols = frame.width();
    let rows = frame.height();
    let region_width = bounds.width();
    let region_height = bounds.height();
    let lines = frame.lines();

    let mut output = vec![vec![b' '; cols]; rows];

    for out_y in 0..scaled_height {
        let src_y = (out_y * region_height / scaled_height).min(region_height.saturating_sub(1));
        let src_row = bounds.top + src_y;
        if src_row >= rows {
            continue;
        }
        let source_line = lines[src_row].as_bytes();
        for out_x in 0..scaled_width {
            let src_x = (out_x * region_width / scaled_width).min(region_width.saturating_sub(1));
            let src_col = bounds.left + src_x;
            if src_col >= cols {
                continue;
            }
            let dst_row = offset_y + out_y;
            let dst_col = offset_x + out_x;
            if dst_row < rows && dst_col < cols {
                output[dst_row][dst_col] = source_line[src_col];
            }
        }
    }

    let fitted_lines = output
        .into_iter()
        .map(|line| String::from_utf8(line).unwrap_or_else(|_| " ".repeat(cols)))
        .collect::<Vec<_>>();

    let mut fitted = AsciiFrame::from_lines(fitted_lines, cols, rows);
    if let Some(metadata) = frame.metadata {
        fitted = fitted.with_metadata(metadata);
    }
    fitted
}

fn remap_frames_symbols(
    frames: Vec<AsciiFrame>,
    mode: SymbolRemapMode,
    target_ramp: &str,
) -> Vec<AsciiFrame> {
    if mode == SymbolRemapMode::None {
        return frames;
    }
    let target = target_ramp.as_bytes().to_vec();
    if target.is_empty() {
        return frames;
    }

    frames
        .into_iter()
        .map(|frame| remap_single_frame_symbols(frame, mode, &target))
        .collect()
}

fn remap_single_frame_symbols(
    frame: AsciiFrame,
    mode: SymbolRemapMode,
    target_ramp: &[u8],
) -> AsciiFrame {
    let cols = frame.width();
    let rows = frame.height();
    let lines = frame.lines();
    let mut output = vec![vec![b' '; cols]; rows];
    let mut source_density = Vec::with_capacity(cols * rows);

    for (row, line) in lines.iter().enumerate().take(rows) {
        let bytes = line.as_bytes();
        for col in 0..cols {
            let ch = bytes.get(col).copied().unwrap_or(b' ');
            if ch == b' ' {
                continue;
            }
            source_density.push(((row, col), density_for_symbol(ch)));
        }
    }

    if source_density.is_empty() {
        return frame;
    }

    let mapped = match mode {
        SymbolRemapMode::None => unreachable!("early-returned"),
        SymbolRemapMode::Density => source_density
            .into_iter()
            .map(|(pos, value)| (pos, value))
            .collect::<Vec<_>>(),
        SymbolRemapMode::Equalize => equalize_density(source_density),
    };

    for ((row, col), value) in mapped {
        let clamped = value.clamp(0.0, 1.0);
        let idx = ((clamped * (target_ramp.len() - 1) as f32).round() as usize)
            .min(target_ramp.len() - 1);
        output[row][col] = target_ramp[idx];
    }

    let mapped_lines = output
        .into_iter()
        .map(|line| String::from_utf8(line).unwrap_or_else(|_| " ".repeat(cols)))
        .collect::<Vec<_>>();

    let mut mapped_frame = AsciiFrame::from_lines(mapped_lines, cols, rows);
    if let Some(metadata) = frame.metadata {
        mapped_frame = mapped_frame.with_metadata(metadata);
    }
    mapped_frame
}

fn equalize_density(values: Vec<((usize, usize), f32)>) -> Vec<((usize, usize), f32)> {
    let mut sorted = values.iter().map(|(_, value)| *value).collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let denom = (sorted.len().saturating_sub(1)).max(1) as f32;

    values
        .into_iter()
        .map(|(pos, value)| {
            let index = sorted.partition_point(|probe| *probe <= value);
            let rank = index.saturating_sub(1);
            (pos, rank as f32 / denom)
        })
        .collect()
}

fn density_for_symbol(symbol: u8) -> f32 {
    if symbol == b' ' {
        return 0.0;
    }
    let bytes = SOURCE_SYMBOL_RAMP.as_bytes();
    if let Some(index) = bytes.iter().position(|value| *value == symbol) {
        if bytes.len() <= 1 {
            return 1.0;
        }
        return index as f32 / (bytes.len() - 1) as f32;
    }

    if symbol.is_ascii_digit() {
        0.65
    } else if symbol.is_ascii_uppercase() {
        0.7
    } else if symbol.is_ascii_lowercase() {
        0.55
    } else {
        0.5
    }
}

fn write_debug_ascii_frames(dir: &Path, frames: &[AsciiFrame]) -> Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create debug txt directory {}", dir.display()))?;
    for (index, frame) in frames.iter().enumerate() {
        let path = dir.join(format!("frame_{index:06}.txt"));
        fs::write(&path, frame.to_text())
            .with_context(|| format!("failed to write debug frame {}", path.display()))?;
    }
    Ok(())
}

fn spawn_source_process(source: &AsciiCaptureSource, cols: u32, rows: u32) -> Result<Child> {
    match source {
        AsciiCaptureSource::AsciiLive { stream } => spawn_ascii_live_process(stream),
        AsciiCaptureSource::Library { .. } => bail!("library sources are generated in-process"),
        AsciiCaptureSource::Chafa { input } => spawn_chafa_process(input, cols, rows),
    }
}

fn spawn_ascii_live_process(stream: &str) -> Result<Child> {
    let url = ascii_live_url(stream)?;
    Command::new("curl")
        .arg("-L")
        .arg("--no-buffer")
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                anyhow!(
                    "curl was not found on PATH. Install curl before using --source ascii-live:{stream}"
                )
            } else {
                anyhow!("failed to spawn curl for ascii-live source: {error}")
            }
        })
}

fn spawn_chafa_process(input: &Path, cols: u32, rows: u32) -> Result<Child> {
    if !input.exists() {
        bail!("chafa input does not exist: {}", input.display());
    }
    let mut command = Command::new("chafa");
    command
        .arg(format!("--size={}x{}", cols, rows))
        .arg("--colors=none");
    for flag in chafa_optional_flags() {
        command.arg(flag);
    }
    command
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    command.spawn().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            anyhow!("chafa was not found on PATH. Install chafa before using --source chafa:<path>")
        } else {
            anyhow!("failed to spawn chafa source process: {error}")
        }
    })
}

fn ascii_live_url(stream: &str) -> Result<&'static str> {
    if let Some(url) = ascii_live_stream_url(stream) {
        return Ok(url);
    }
    let supported = ascii_live_stream_names().join(", ");
    bail!("unsupported ascii-live stream '{stream}': supported streams: {supported}")
}

fn capture_library_frames(
    id: &str,
    frame_count: u32,
    fps: u32,
    cols: u32,
    rows: u32,
) -> Result<Vec<AsciiFrame>> {
    let mut frames = Vec::with_capacity(frame_count as usize);
    let cols = cols as usize;
    let rows = rows as usize;

    if !library_source_names().iter().any(|value| *value == id) {
        let supported = library_source_names().join(", ");
        bail!("unsupported library source '{id}': supported ids: {supported}");
    }

    for frame_index in 0..frame_count {
        let t = frame_index as f32 / fps as f32;
        let lines = match id {
            "geist-wave" => library_frame_geist_wave(t, cols, rows),
            "geist-scan" => library_frame_geist_scan(t, cols, rows),
            "geist-blocks" => library_frame_geist_blocks(t, cols, rows),
            _ => unreachable!("validated above"),
        };
        let timestamp_ms = ((frame_index as f64 / fps as f64) * 1000.0).round() as u64;
        frames.push(
            AsciiFrame::from_lines(lines, cols, rows).with_metadata(AsciiFrameMetadata {
                source_frame_index: Some(frame_index as u64),
                source_timestamp_ms: Some(timestamp_ms),
            }),
        );
    }

    Ok(frames)
}

fn library_frame_geist_wave(t: f32, cols: usize, rows: usize) -> Vec<String> {
    let ramp: Vec<char> = " etaoinshrdlucmfwypvbgkjqxz#@".chars().collect();
    let mut lines = vec![" ".repeat(cols); rows];
    for (row_idx, line) in lines.iter_mut().enumerate() {
        let mut row = vec![b' '; cols];
        let y = row_idx as f32 / rows as f32;
        for (col_idx, cell) in row.iter_mut().enumerate() {
            let x = col_idx as f32 / cols as f32;
            let wave = ((x * 13.0 + t * 2.5).sin() + (y * 11.0 + t * 1.4).cos()) * 0.5;
            let density = (wave * 0.5 + 0.5).clamp(0.0, 1.0);
            let idx = (density * (ramp.len().saturating_sub(1)) as f32).round() as usize;
            *cell = ramp[idx] as u8;
        }
        *line = String::from_utf8(row).unwrap_or_else(|_| " ".repeat(cols));
    }
    lines
}

fn library_frame_geist_scan(t: f32, cols: usize, rows: usize) -> Vec<String> {
    let banner = "GEIST PIXEL VCR DEV MODE";
    let mut lines = vec![" ".repeat(cols); rows];
    let scan = ((t * 9.0) as usize) % rows.max(1);
    for (row_idx, line) in lines.iter_mut().enumerate() {
        let mut row = vec![b' '; cols];
        if row_idx == scan || row_idx == (scan + 1).min(rows.saturating_sub(1)) {
            for value in &mut row {
                *value = b':';
            }
        }
        if row_idx % 3 == 0 {
            let offset = ((t * 7.0) as usize + row_idx * 2) % cols.max(1);
            for (index, ch) in banner.as_bytes().iter().enumerate() {
                let col = (offset + index) % cols.max(1);
                row[col] = ch.to_ascii_uppercase();
            }
        }
        *line = String::from_utf8(row).unwrap_or_else(|_| " ".repeat(cols));
    }
    lines
}

fn library_frame_geist_blocks(t: f32, cols: usize, rows: usize) -> Vec<String> {
    let mut lines = vec![" ".repeat(cols); rows];
    let block_w = (cols / 10).max(2);
    let block_h = (rows / 6).max(2);
    let x1 = ((t * 12.0) as usize) % cols.max(1);
    let y1 = ((t * 7.0) as usize) % rows.max(1);
    let x2 = ((t * 9.0 + 13.0) as usize) % cols.max(1);
    let y2 = ((t * 5.0 + 5.0) as usize) % rows.max(1);

    for (row_idx, line) in lines.iter_mut().enumerate() {
        let mut row = vec![b' '; cols];
        for (col_idx, cell) in row.iter_mut().enumerate() {
            let in_block_1 = col_idx >= x1.saturating_sub(block_w / 2)
                && col_idx < (x1 + block_w / 2).min(cols)
                && row_idx >= y1.saturating_sub(block_h / 2)
                && row_idx < (y1 + block_h / 2).min(rows);
            let in_block_2 = col_idx >= x2.saturating_sub(block_w / 2)
                && col_idx < (x2 + block_w / 2).min(cols)
                && row_idx >= y2.saturating_sub(block_h / 2)
                && row_idx < (y2 + block_h / 2).min(rows);
            *cell = if in_block_1 || in_block_2 { b'#' } else { b'.' };
        }
        *line = String::from_utf8(row).unwrap_or_else(|_| " ".repeat(cols));
    }
    lines
}

fn chafa_optional_flags() -> Vec<String> {
    let output = Command::new("chafa").arg("--help").output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let mut flags = Vec::new();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    if text.contains("--symbols") {
        flags.push("--symbols=ascii".to_owned());
    }
    if text.contains("--animate") {
        flags.push("--animate=on".to_owned());
    }
    if text.contains("--clear") {
        flags.push("--clear".to_owned());
    }
    flags
}

fn terminate_source_process(child: &mut Child) -> Result<ExitStatus> {
    if let Some(status) = child.try_wait().context("failed querying source status")? {
        return Ok(status);
    }
    let _ = child.kill();
    child
        .wait()
        .context("failed waiting for source process to terminate")
}

fn spawn_stdout_worker(
    mut stdout: impl Read + Send + 'static,
) -> Result<(mpsc::Receiver<Vec<u8>>, JoinHandle<Result<()>>)> {
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    let worker = thread::Builder::new()
        .name("vcr-ascii-capture-stdout".to_owned())
        .spawn(move || {
            loop {
                let mut chunk = vec![0_u8; 8 * 1024];
                let read = stdout
                    .read(&mut chunk)
                    .context("failed reading source stdout")?;
                if read == 0 {
                    break;
                }
                chunk.truncate(read);
                if sender.send(chunk).is_err() {
                    break;
                }
            }
            Ok(())
        })
        .context("failed to spawn capture stdout worker thread")?;
    Ok((receiver, worker))
}

#[derive(Debug, Clone)]
struct GlyphBitmap {
    width: usize,
    height: usize,
    bitmap: Vec<u8>,
}

struct AsciiFrameRasterizer {
    font: Font,
    font_size: f32,
    cols: usize,
    rows: usize,
    cell_width: u32,
    line_height: u32,
    pixel_width: u32,
    pixel_height: u32,
    glyph_cache: HashMap<fontdue::layout::GlyphRasterConfig, GlyphBitmap>,
}

impl AsciiFrameRasterizer {
    fn new(font_path: &Path, font_size: f32, cols: usize, rows: usize) -> Result<Self> {
        let font_bytes = fs::read(font_path)
            .with_context(|| format!("failed to read font file {}", font_path.display()))?;
        let font = Font::from_bytes(font_bytes, FontSettings::default())
            .map_err(|error| anyhow!("failed to parse font {}: {error}", font_path.display()))?;
        let cell_width = font.metrics('M', font_size).advance_width.ceil().max(1.0) as u32;
        let line_height = (font_size * 1.45).round().max(1.0) as u32;
        let pixel_width = (cols as u32).saturating_mul(cell_width).max(2);
        let pixel_height = (rows as u32).saturating_mul(line_height).max(2);
        Ok(Self {
            font,
            font_size,
            cols,
            rows,
            cell_width,
            line_height,
            pixel_width,
            pixel_height,
            glyph_cache: HashMap::new(),
        })
    }

    fn pixel_width(&self) -> u32 {
        self.pixel_width
    }

    fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    fn render(&mut self, frame: &AsciiFrame) -> Vec<u8> {
        let mut rgba = vec![0_u8; (self.pixel_width * self.pixel_height * 4) as usize];
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }

        for (row_index, line) in frame.lines().iter().take(self.rows).enumerate() {
            let y = (row_index as u32).saturating_mul(self.line_height);
            self.draw_line(&mut rgba, 0, y, line.trim_end());
        }

        rgba
    }

    fn draw_line(&mut self, frame: &mut [u8], x: u32, y: u32, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: x as f32,
            y: y as f32,
            max_width: Some((self.cols as u32 * self.cell_width) as f32),
            max_height: None,
            horizontal_align: fontdue::layout::HorizontalAlign::Left,
            vertical_align: fontdue::layout::VerticalAlign::Top,
            line_height: 1.0,
            wrap_style: fontdue::layout::WrapStyle::Letter,
            wrap_hard_breaks: true,
        });
        layout.append(&[&self.font], &TextStyle::new(text, self.font_size, 0));

        for glyph in layout.glyphs() {
            if glyph.width == 0 || glyph.height == 0 {
                continue;
            }
            let glyph_bitmap = self.glyph_cache.entry(glyph.key).or_insert_with(|| {
                let (_, bitmap) = self.font.rasterize_config(glyph.key);
                GlyphBitmap {
                    width: glyph.width,
                    height: glyph.height,
                    bitmap,
                }
            });
            blend_glyph(
                frame,
                self.pixel_width,
                self.pixel_height,
                glyph.x.round() as i32,
                glyph.y.round() as i32,
                glyph_bitmap,
                [255, 255, 255, 255],
            );
        }
    }
}

struct ProRes422Encoder {
    child: Child,
    stdin: ChildStdin,
}

impl ProRes422Encoder {
    fn spawn(
        output_path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        tmp_dir: Option<&Path>,
    ) -> Result<Self> {
        let mut command = Command::new("ffmpeg");
        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-s:v")
            .arg(format!("{}x{}", width, height))
            .arg("-r")
            .arg(fps.to_string())
            .arg("-i")
            .arg("-")
            .arg("-an")
            .arg("-c:v")
            .arg("prores_ks")
            .arg("-profile:v")
            .arg("2")
            .arg("-pix_fmt")
            .arg("yuv422p10le")
            .arg(output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        if let Some(tmp_dir) = tmp_dir {
            command.current_dir(tmp_dir);
        }

        let mut child = command.spawn().map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                anyhow!(
                    "ffmpeg was not found on PATH. Install ffmpeg and verify `ffmpeg -version` works before running `vcr ascii capture`."
                )
            } else {
                anyhow!("failed to spawn ffmpeg sidecar process: {error}")
            }
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to capture ffmpeg stdin"))?;
        Ok(Self { child, stdin })
    }

    fn write_frame(&mut self, rgba: &[u8]) -> Result<()> {
        self.stdin
            .write_all(rgba)
            .context("failed to write capture frame to ffmpeg stdin")
    }

    fn finish(mut self) -> Result<()> {
        self.stdin
            .flush()
            .context("failed to flush ffmpeg stdin for ascii capture")?;
        drop(self.stdin);
        let status = self
            .child
            .wait()
            .context("failed waiting for ffmpeg process")?;
        if !status.success() {
            bail!("ffmpeg failed with status {status}");
        }
        Ok(())
    }
}

fn blend_glyph(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    glyph: &GlyphBitmap,
    color: [u8; 4],
) {
    for row in 0..glyph.height {
        let py = y + row as i32;
        if py < 0 || py >= frame_height as i32 {
            continue;
        }
        for col in 0..glyph.width {
            let px = x + col as i32;
            if px < 0 || px >= frame_width as i32 {
                continue;
            }
            let mask = glyph.bitmap[row * glyph.width + col];
            if mask == 0 {
                continue;
            }
            let alpha = ((u16::from(mask) * u16::from(color[3])) / 255) as u8;
            let idx = ((py as u32 * frame_width + px as u32) * 4) as usize;
            blend_pixel(frame, idx, [color[0], color[1], color[2], alpha]);
        }
    }
}

fn blend_pixel(frame: &mut [u8], idx: usize, src: [u8; 4]) {
    let alpha = u16::from(src[3]);
    if alpha == 0 {
        return;
    }
    let inv_alpha = 255_u16.saturating_sub(alpha);
    for channel in 0..3 {
        let dst = u16::from(frame[idx + channel]);
        let src_c = u16::from(src[channel]);
        frame[idx + channel] = ((src_c * alpha + dst * inv_alpha + 127) / 255) as u8;
    }
    frame[idx + 3] = 255;
}

struct BestEffortAnsiFrameParser {
    cols: usize,
    rows: usize,
    screen: Vec<char>,
    cursor_col: usize,
    cursor_row: usize,
    state: ParseState,
    saw_text_since_boundary: bool,
    saw_any_text: bool,
    latest_complete_frame: Option<AsciiFrame>,
}

enum ParseState {
    Plain,
    Escape,
    Csi(Vec<u8>),
    Osc { saw_esc: bool },
}

impl BestEffortAnsiFrameParser {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            screen: vec![' '; cols * rows],
            cursor_col: 0,
            cursor_row: 0,
            state: ParseState::Plain,
            saw_text_since_boundary: false,
            saw_any_text: false,
            latest_complete_frame: None,
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state = match std::mem::replace(&mut self.state, ParseState::Plain) {
                ParseState::Plain => self.consume_plain_byte(*byte),
                ParseState::Escape => self.consume_escape_byte(*byte),
                ParseState::Csi(mut buffer) => {
                    buffer.push(*byte);
                    if (0x40..=0x7e).contains(byte) || buffer.len() > 64 {
                        self.consume_csi_sequence(&buffer);
                        ParseState::Plain
                    } else {
                        ParseState::Csi(buffer)
                    }
                }
                ParseState::Osc { mut saw_esc } => {
                    if *byte == 0x07 || (saw_esc && *byte == b'\\') {
                        ParseState::Plain
                    } else {
                        saw_esc = *byte == 0x1b;
                        ParseState::Osc { saw_esc }
                    }
                }
            };
        }
    }

    fn latest_frame(&self) -> Option<AsciiFrame> {
        if let Some(frame) = &self.latest_complete_frame {
            return Some(frame.clone());
        }
        if self.saw_any_text {
            return Some(self.snapshot_frame());
        }
        None
    }

    fn consume_plain_byte(&mut self, byte: u8) -> ParseState {
        match byte {
            0x1b => ParseState::Escape,
            b'\n' => {
                self.cursor_col = 0;
                self.cursor_row = (self.cursor_row + 1).min(self.rows.saturating_sub(1));
                ParseState::Plain
            }
            b'\r' => {
                self.cursor_col = 0;
                ParseState::Plain
            }
            b'\t' => {
                for _ in 0..4 {
                    self.write_char(' ');
                }
                ParseState::Plain
            }
            0x20..=0x7e => {
                self.write_char(byte as char);
                ParseState::Plain
            }
            _ => ParseState::Plain,
        }
    }

    fn consume_escape_byte(&mut self, byte: u8) -> ParseState {
        match byte {
            b'[' => ParseState::Csi(Vec::new()),
            b']' => ParseState::Osc { saw_esc: false },
            _ => ParseState::Plain,
        }
    }

    fn consume_csi_sequence(&mut self, sequence: &[u8]) {
        let Some((&command, params_raw)) = sequence.split_last() else {
            return;
        };
        let params = String::from_utf8_lossy(params_raw);
        match command as char {
            'H' | 'f' => {
                let (row, col) = parse_cursor_position(&params);
                if row == 1 && col == 1 {
                    self.mark_frame_boundary();
                }
                self.cursor_row = row.saturating_sub(1).min(self.rows.saturating_sub(1));
                self.cursor_col = col.saturating_sub(1).min(self.cols.saturating_sub(1));
            }
            'J' => {
                let mode = params.trim();
                if mode.is_empty() || mode == "2" || mode == "3" {
                    self.mark_frame_boundary();
                    self.clear_screen();
                }
            }
            'K' => {
                self.clear_line_from_cursor();
            }
            'A' => {
                let delta = parse_csi_number(&params).max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(delta);
            }
            'B' => {
                let delta = parse_csi_number(&params).max(1) as usize;
                self.cursor_row = (self.cursor_row + delta).min(self.rows.saturating_sub(1));
            }
            'C' => {
                let delta = parse_csi_number(&params).max(1) as usize;
                self.cursor_col = (self.cursor_col + delta).min(self.cols.saturating_sub(1));
            }
            'D' => {
                let delta = parse_csi_number(&params).max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(delta);
            }
            'm' => {}
            _ => {}
        }
    }

    fn write_char(&mut self, ch: char) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let row = self.cursor_row.min(self.rows - 1);
        let col = self.cursor_col.min(self.cols - 1);
        self.screen[row * self.cols + col] = ch;
        self.saw_text_since_boundary = true;
        self.saw_any_text = true;

        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row = (self.cursor_row + 1).min(self.rows - 1);
        }
    }

    fn clear_screen(&mut self) {
        self.screen.fill(' ');
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    fn clear_line_from_cursor(&mut self) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let row = self.cursor_row.min(self.rows - 1);
        let start_col = self.cursor_col.min(self.cols - 1);
        for col in start_col..self.cols {
            self.screen[row * self.cols + col] = ' ';
        }
    }

    fn mark_frame_boundary(&mut self) {
        if !self.saw_text_since_boundary {
            return;
        }
        self.latest_complete_frame = Some(self.snapshot_frame());
        self.saw_text_since_boundary = false;
    }

    fn snapshot_frame(&self) -> AsciiFrame {
        let mut lines = Vec::with_capacity(self.rows);
        for row in 0..self.rows {
            let mut line = String::with_capacity(self.cols);
            for col in 0..self.cols {
                line.push(self.screen[row * self.cols + col]);
            }
            lines.push(line);
        }
        AsciiFrame::from_lines(lines, self.cols, self.rows)
    }
}

fn parse_csi_number(raw: &str) -> u32 {
    raw.split(';')
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(1)
}

fn parse_cursor_position(raw: &str) -> (usize, usize) {
    let mut parts = raw.split(';');
    let row = parts
        .next()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let col = parts
        .next()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    (row, col)
}

#[cfg(test)]
mod tests {
    use super::{
        density_for_symbol, fit_frames_to_canvas, parse_capture_size, remap_frames_symbols,
        resolve_target_frame_count, BestEffortAnsiFrameParser, SymbolRemapMode,
    };
    use crate::ascii_frame::AsciiFrame;

    #[test]
    fn parse_size_supports_cols_x_rows() {
        let (cols, rows) = parse_capture_size("120x45").expect("size should parse");
        assert_eq!(cols, 120);
        assert_eq!(rows, 45);
    }

    #[test]
    fn frame_count_prefers_explicit_max() {
        let frame_count = resolve_target_frame_count(30, 5.0, Some(3)).expect("frame count");
        assert_eq!(frame_count, 3);
    }

    #[test]
    fn parser_snapshots_after_clear_and_home_boundaries() {
        let mut parser = BestEffortAnsiFrameParser::new(4, 2);
        parser.push_bytes(b"ABCD\nWXYZ\x1b[H\x1b[2J");
        let frame = parser.latest_frame().expect("frame should exist");
        assert_eq!(frame.lines()[0], "ABCD");
        assert_eq!(frame.lines()[1], "WXYZ");
    }

    #[test]
    fn fit_to_canvas_recenters_top_left_content() {
        let frame = AsciiFrame::from_lines(["AB    ", "CD    ", "      ", "      "], 6, 4);
        let fitted = fit_frames_to_canvas(vec![frame], 6, 4, 0.0);
        assert_eq!(fitted[0].lines()[0], " AABB ");
        assert_eq!(fitted[0].lines()[1], " AABB ");
        assert_eq!(fitted[0].lines()[2], " CCDD ");
        assert_eq!(fitted[0].lines()[3], " CCDD ");
    }

    #[test]
    fn fit_padding_reserves_visual_margin() {
        let frame = AsciiFrame::from_lines(["AAAAAA", "AAAAAA", "AAAAAA", "AAAAAA"], 6, 4);
        let fitted = fit_frames_to_canvas(vec![frame], 6, 4, 0.25);
        assert_eq!(fitted[0].lines()[0], "      ");
        assert_eq!(fitted[0].lines()[3], "      ");
    }

    #[test]
    fn density_for_symbol_orders_darker_symbols_higher() {
        assert!(density_for_symbol(b'.') < density_for_symbol(b'+'));
        assert!(density_for_symbol(b'+') < density_for_symbol(b'@'));
    }

    #[test]
    fn symbol_remap_equalize_spreads_symbols_across_target_ramp() {
        let frame = AsciiFrame::from_lines(["..++@@", "..++@@"], 6, 2);
        let remapped = remap_frames_symbols(vec![frame], SymbolRemapMode::Equalize, ".:-=+*#%@");
        let joined = remapped[0].to_text();
        let unique = joined
            .bytes()
            .filter(|byte| *byte != b' ' && *byte != b'\n')
            .collect::<std::collections::BTreeSet<_>>();
        assert!(unique.len() >= 3);
        assert!(unique.contains(&b'@'));
    }
}
