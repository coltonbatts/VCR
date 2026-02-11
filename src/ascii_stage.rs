use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};

const DEFAULT_FINAL_HOLD_MS: u64 = 700;
const MIN_WIDTH: u32 = 320;
const MIN_HEIGHT: u32 = 180;

const GEIST_PIXEL_FILES: [&str; 5] = [
    "GeistPixel-Line.ttf",
    "GeistPixel-Square.ttf",
    "GeistPixel-Grid.ttf",
    "GeistPixel-Circle.ttf",
    "GeistPixel-Triangle.ttf",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    Static,
    SlowZoom,
    Follow,
}

#[derive(Debug, Clone)]
pub struct AsciiStageRenderArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub theme: String,
    pub fps: u32,
    pub speed: f32,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub chrome: bool,
    pub camera_mode: CameraMode,
    pub font_scale: f32,
}

#[derive(Debug, Clone)]
pub struct AsciiStageRenderSummary {
    pub frame_count: u32,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    User(String),
    Assistant(String),
    Tool { name: String, body: String },
    System(String),
    Pause(u64),
    TypeSpeed(TypeSpeedMode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSpeedMode {
    Fast,
    Normal,
    Slow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    User,
    Assistant,
    System,
    ToolHeader,
    ToolBody,
    Spacer,
}

#[derive(Debug, Clone)]
enum Action {
    NewSection,
    AppendText {
        kind: LineKind,
        text: String,
        typed: bool,
        speed: TypeSpeedMode,
    },
    Pause(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EventOp {
    NewSection,
    AppendText { kind: LineKind, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimedEvent {
    at_ms: u64,
    op: EventOp,
}

#[derive(Debug, Clone)]
struct Theme {
    bg: [u8; 4],
    terminal_bg: [u8; 4],
    terminal_border: [u8; 4],
    header_bg: [u8; 4],
    title_text: [u8; 4],
    user_text: [u8; 4],
    assistant_text: [u8; 4],
    system_text: [u8; 4],
    tool_header_text: [u8; 4],
    tool_header_bg: [u8; 4],
    tool_body_text: [u8; 4],
    tool_body_bg: [u8; 4],
    scanline: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineAnimation {
    None,
    ToolPop,
}

#[derive(Debug, Clone)]
struct LineRecord {
    kind: LineKind,
    text: String,
    born_ms: u64,
    animation: LineAnimation,
}

#[derive(Debug, Clone)]
struct CurrentLine {
    kind: LineKind,
    text: String,
    len: usize,
    born_ms: u64,
    animation: LineAnimation,
}

#[derive(Debug, Clone)]
struct TerminalState {
    lines: Vec<LineRecord>,
    current: Option<CurrentLine>,
    max_cols: usize,
}

impl TerminalState {
    fn new(max_cols: usize) -> Self {
        Self {
            lines: Vec::new(),
            current: None,
            max_cols,
        }
    }

    fn apply(&mut self, op: &EventOp, at_ms: u64) {
        match op {
            EventOp::NewSection => self.insert_section_break(at_ms),
            EventOp::AppendText { kind, text } => self.append_text(*kind, text, at_ms),
        }
    }

    fn insert_section_break(&mut self, at_ms: u64) {
        self.flush_current();
        if self
            .lines
            .last()
            .map(|line| line.text.is_empty())
            .unwrap_or(false)
        {
            return;
        }
        if !self.lines.is_empty() {
            self.lines.push(LineRecord {
                kind: LineKind::Spacer,
                text: String::new(),
                born_ms: at_ms,
                animation: LineAnimation::None,
            });
        }
    }

    fn append_text(&mut self, kind: LineKind, text: &str, at_ms: u64) {
        let animation = match kind {
            LineKind::ToolHeader | LineKind::ToolBody => LineAnimation::ToolPop,
            _ => LineAnimation::None,
        };
        for ch in text.chars() {
            if ch == '\n' {
                self.flush_current();
                continue;
            }

            if self.current.is_none() {
                self.current = Some(CurrentLine {
                    kind,
                    text: String::new(),
                    len: 0,
                    born_ms: at_ms,
                    animation,
                });
            }

            let needs_flush = self
                .current
                .as_ref()
                .map(|current| current.kind != kind || current.len >= self.max_cols)
                .unwrap_or(false);
            if needs_flush {
                self.flush_current();
                self.current = Some(CurrentLine {
                    kind,
                    text: String::new(),
                    len: 0,
                    born_ms: at_ms,
                    animation,
                });
            }

            if let Some(current) = self.current.as_mut() {
                current.text.push(ch);
                current.len += 1;
            }
        }
    }

    fn flush_current(&mut self) {
        if let Some(current) = self.current.take() {
            self.lines.push(LineRecord {
                kind: current.kind,
                text: current.text,
                born_ms: current.born_ms,
                animation: current.animation,
            });
        }
    }

    fn snapshot_lines(&self) -> Vec<LineRecord> {
        let mut all = self.lines.clone();
        if let Some(current) = &self.current {
            all.push(LineRecord {
                kind: current.kind,
                text: current.text.clone(),
                born_ms: current.born_ms,
                animation: current.animation,
            });
        }
        all
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScrollAnimator {
    from: f32,
    to: f32,
    start_ms: u64,
    duration_ms: u64,
}

impl ScrollAnimator {
    fn new() -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            start_ms: 0,
            duration_ms: 1,
        }
    }

    fn value_at(self, now_ms: u64) -> f32 {
        if self.duration_ms == 0 || now_ms <= self.start_ms {
            return self.from;
        }
        let elapsed = now_ms.saturating_sub(self.start_ms);
        if elapsed >= self.duration_ms {
            return self.to;
        }
        let t = elapsed as f32 / self.duration_ms as f32;
        let eased = ease_out_cubic(t);
        self.from + (self.to - self.from) * eased
    }

    fn set_target(&mut self, now_ms: u64, target: f32, duration_ms: u64) {
        if (target - self.to).abs() < 0.001 {
            return;
        }
        let current = self.value_at(now_ms);
        self.from = current;
        self.to = target.max(0.0);
        self.start_ms = now_ms;
        self.duration_ms = duration_ms.max(1);
    }
}

#[derive(Debug, Clone)]
struct GlyphBitmap {
    width: usize,
    height: usize,
    bitmap: Vec<u8>,
}

struct TextPainter {
    font: Font,
    font_size: f32,
    glyph_cache: HashMap<fontdue::layout::GlyphRasterConfig, GlyphBitmap>,
}

impl TextPainter {
    fn new(font_path: &Path, font_size: f32) -> Result<Self> {
        let font_bytes = fs::read(font_path).with_context(|| {
            format!(
                "missing Geist Pixel font '{}'; install Geist Pixel variants in assets/fonts/geist_pixel",
                font_path.display()
            )
        })?;
        let font = Font::from_bytes(font_bytes, FontSettings::default()).map_err(|error| {
            anyhow!(
                "failed to parse Geist Pixel font {}: {error}",
                font_path.display()
            )
        })?;
        Ok(Self {
            font,
            font_size,
            glyph_cache: HashMap::new(),
        })
    }

    fn cell_width(&self) -> u32 {
        let metrics = self.font.metrics('M', self.font_size);
        metrics.advance_width.ceil().max(1.0) as u32
    }

    fn line_height(&self) -> u32 {
        (self.font_size * 1.45).round().max(1.0) as u32
    }

    fn draw_line(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: u32,
        y: u32,
        text: &str,
        color: [u8; 4],
    ) {
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: x as f32,
            y: y as f32,
            max_width: None,
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
                width,
                height,
                glyph.x.round() as i32,
                glyph.y.round() as i32,
                glyph_bitmap,
                color,
            );
        }
    }
}

struct FontPack {
    line: TextPainter,
    square: TextPainter,
    grid: TextPainter,
    circle: TextPainter,
    triangle: TextPainter,
    max_cell_width: u32,
    max_line_height: u32,
}

impl FontPack {
    fn load(manifest_root: &Path, font_size: f32) -> Result<Self> {
        let font_dir = manifest_root.join("assets/fonts/geist_pixel");
        let missing = GEIST_PIXEL_FILES
            .iter()
            .filter(|name| !font_dir.join(name).exists())
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "missing Geist Pixel font variants: {}. Install all five in '{}': GeistPixel-Line.ttf, GeistPixel-Square.ttf, GeistPixel-Grid.ttf, GeistPixel-Circle.ttf, GeistPixel-Triangle.ttf",
                missing.join(", "),
                font_dir.display()
            );
        }

        let mut line = TextPainter::new(&font_dir.join("GeistPixel-Line.ttf"), font_size)?;
        let square = TextPainter::new(&font_dir.join("GeistPixel-Square.ttf"), font_size)?;
        let grid = TextPainter::new(&font_dir.join("GeistPixel-Grid.ttf"), font_size)?;
        let circle = TextPainter::new(&font_dir.join("GeistPixel-Circle.ttf"), font_size)?;
        let triangle = TextPainter::new(&font_dir.join("GeistPixel-Triangle.ttf"), font_size)?;

        let max_cell_width = [
            line.cell_width(),
            square.cell_width(),
            grid.cell_width(),
            circle.cell_width(),
            triangle.cell_width(),
        ]
        .into_iter()
        .max()
        .unwrap_or(8)
        .max(8);
        let max_line_height = [
            line.line_height(),
            square.line_height(),
            grid.line_height(),
            circle.line_height(),
            triangle.line_height(),
        ]
        .into_iter()
        .max()
        .unwrap_or(16)
        .max(16);

        line.font_size = font_size;

        Ok(Self {
            line,
            square,
            grid,
            circle,
            triangle,
            max_cell_width,
            max_line_height,
        })
    }

    fn draw_line(
        &mut self,
        kind: LineKind,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: u32,
        y: u32,
        text: &str,
        color: [u8; 4],
    ) {
        let painter = match kind {
            LineKind::User => &mut self.triangle,
            LineKind::Assistant => &mut self.line,
            LineKind::System => &mut self.square,
            LineKind::ToolHeader => &mut self.grid,
            LineKind::ToolBody => &mut self.circle,
            LineKind::Spacer => &mut self.line,
        };
        painter.draw_line(frame, width, height, x, y, text, color);
    }
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

pub fn render_ascii_stage_video(args: &AsciiStageRenderArgs) -> Result<AsciiStageRenderSummary> {
    if args.fps == 0 {
        bail!("--fps must be > 0");
    }
    if !args.speed.is_finite() || args.speed <= 0.0 {
        bail!("--speed must be > 0");
    }
    if args.width < MIN_WIDTH || args.height < MIN_HEIGHT {
        bail!("--size must be at least {}x{}", MIN_WIDTH, MIN_HEIGHT);
    }

    let raw = fs::read_to_string(&args.input)
        .with_context(|| format!("failed to read ascii transcript {}", args.input.display()))?;
    let blocks = parse_ascii_stage_script(&raw)?;
    if blocks.is_empty() {
        bail!("vcr ascii stage: empty input transcript");
    }

    let theme = resolve_void_theme(&args.theme)?;
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let font_size =
        (((args.height as f32) / 54.0) * args.font_scale.clamp(0.75, 1.6)).clamp(14.0, 36.0);
    let mut fonts = FontPack::load(manifest_root, font_size)?;

    let actions = build_actions(&blocks);
    let (events, total_ms) = build_timeline(&actions, args.seed, args.speed);

    let frame_count_u64 = ((total_ms.saturating_mul(args.fps as u64) + 999) / 1000)
        .saturating_add(1)
        .max(1);
    let frame_count = u32::try_from(frame_count_u64).context("ascii stage frame count overflow")?;

    let mut writer =
        AsciiStageFfmpegWriter::spawn(args.width, args.height, args.fps, &args.output)?;

    let outer_pad = ((args.width.min(args.height) as f32) * 0.04).round() as u32;
    let outer_pad = outer_pad.clamp(24, 72);
    let terminal_rect = Rect {
        x: outer_pad,
        y: outer_pad,
        w: args.width.saturating_sub(outer_pad * 2),
        h: args.height.saturating_sub(outer_pad * 2),
    };
    let header_h = ((args.height as f32) * 0.045).round() as u32;
    let header_h = header_h.clamp(30, 54);
    let text_pad_x = ((args.width as f32) * 0.015).round() as u32;
    let text_pad_x = text_pad_x.clamp(14, 36);
    let text_pad_y = ((args.height as f32) * 0.02).round() as u32;
    let text_pad_y = text_pad_y.clamp(10, 30);
    let text_area = Rect {
        x: terminal_rect.x + text_pad_x,
        y: terminal_rect.y + header_h + text_pad_y,
        w: terminal_rect.w.saturating_sub(text_pad_x * 2),
        h: terminal_rect.h.saturating_sub(header_h + text_pad_y * 2),
    };

    let cell_width = fonts.max_cell_width;
    let line_height = fonts.max_line_height;
    let max_cols = ((text_area.w / cell_width).max(8)) as usize;
    let visible_rows = ((text_area.h / line_height).max(4)) as usize;

    let mut state = TerminalState::new(max_cols);
    let mut event_cursor = 0usize;
    let mut base_frame = vec![0_u8; (args.width * args.height * 4) as usize];
    draw_static_frame(
        &mut base_frame,
        args.width,
        args.height,
        terminal_rect,
        header_h,
        text_area,
        args.chrome,
        &theme,
    );
    let mut frame = vec![0_u8; base_frame.len()];
    let mut zoomed_frame = if args.camera_mode == CameraMode::SlowZoom {
        Some(vec![0_u8; base_frame.len()])
    } else {
        None
    };
    let scroll_duration_ms = ((8.0 * 1000.0) / args.fps.max(1) as f32).round().max(80.0) as u64;
    let tool_pop_duration_ms = ((8.0 * 1000.0) / args.fps.max(1) as f32).round() as u64;
    let mut scroll = ScrollAnimator::new();
    let text_area_bottom = text_area.y.saturating_add(text_area.h) as i32;

    for frame_index in 0..frame_count {
        frame.copy_from_slice(&base_frame);
        let t_ms = (u64::from(frame_index) * 1000) / u64::from(args.fps);

        while event_cursor < events.len() && events[event_cursor].at_ms <= t_ms {
            state.apply(&events[event_cursor].op, events[event_cursor].at_ms);
            event_cursor += 1;
        }

        let lines = state.snapshot_lines();
        let target_scroll = match args.camera_mode {
            CameraMode::Follow => {
                let focus_row = ((visible_rows as f32) * 0.72).round() as usize;
                lines
                    .len()
                    .saturating_sub(focus_row.saturating_add(1))
                    .saturating_sub(1) as f32
            }
            CameraMode::Static | CameraMode::SlowZoom => {
                lines.len().saturating_sub(visible_rows) as f32
            }
        };
        scroll.set_target(t_ms, target_scroll, scroll_duration_ms);
        let display_scroll = scroll.value_at(t_ms);
        let base_row = display_scroll.floor() as usize;
        let scroll_px = ((display_scroll - base_row as f32) * line_height as f32).round() as i32;
        let end_row = (base_row + visible_rows + 2).min(lines.len());

        for (row_idx, line) in lines[base_row..end_row].iter().enumerate() {
            let y = text_area.y as i32 + (row_idx as i32 * line_height as i32) - scroll_px;
            if y + line_height as i32 <= text_area.y as i32 || y >= text_area_bottom {
                continue;
            }
            let y_u32 = y.max(0) as u32;
            let (alpha_scale, slide_px) =
                line_animation_at(line, t_ms, tool_pop_duration_ms.max(60));
            let y_u32 = y_u32.saturating_sub(slide_px.max(0) as u32);

            match line.kind {
                LineKind::User => {
                    if line.text.starts_with("> ") {
                        fill_rect(
                            &mut frame,
                            args.width,
                            args.height,
                            text_area.x.saturating_sub(6),
                            y_u32.saturating_sub(1),
                            (cell_width.saturating_mul(2)).saturating_add(10),
                            line_height.saturating_sub(2),
                            scale_alpha(theme.tool_header_bg, alpha_scale),
                        );
                    }
                }
                LineKind::ToolHeader => {
                    let header_bg = scale_alpha(theme.tool_header_bg, alpha_scale);
                    fill_rect(
                        &mut frame,
                        args.width,
                        args.height,
                        text_area.x.saturating_sub(8),
                        y_u32.saturating_sub(2),
                        text_area.w.saturating_add(16),
                        line_height.saturating_add(2),
                        header_bg,
                    );
                    draw_rect_border(
                        &mut frame,
                        args.width,
                        args.height,
                        Rect {
                            x: text_area.x.saturating_sub(8),
                            y: y_u32.saturating_sub(2),
                            w: text_area.w.saturating_add(16),
                            h: line_height.saturating_add(2),
                        },
                        scale_alpha(theme.tool_header_text, alpha_scale),
                        1,
                    );
                }
                LineKind::ToolBody => {
                    let body_bg = scale_alpha(theme.tool_body_bg, alpha_scale);
                    fill_rect(
                        &mut frame,
                        args.width,
                        args.height,
                        text_area.x.saturating_sub(8),
                        y_u32.saturating_sub(2),
                        text_area.w.saturating_add(16),
                        line_height.saturating_add(2),
                        body_bg,
                    );
                    fill_rect(
                        &mut frame,
                        args.width,
                        args.height,
                        text_area.x.saturating_sub(8),
                        y_u32.saturating_sub(2),
                        2,
                        line_height.saturating_add(2),
                        scale_alpha(theme.tool_header_text, alpha_scale),
                    );
                    fill_rect(
                        &mut frame,
                        args.width,
                        args.height,
                        text_area.x + text_area.w.saturating_add(6),
                        y_u32.saturating_sub(2),
                        2,
                        line_height.saturating_add(2),
                        scale_alpha(theme.tool_header_text, alpha_scale),
                    );
                }
                _ => {}
            }

            let color = scale_alpha(
                match line.kind {
                    LineKind::User => theme.user_text,
                    LineKind::Assistant => theme.assistant_text,
                    LineKind::System => theme.system_text,
                    LineKind::ToolHeader => theme.tool_header_text,
                    LineKind::ToolBody => theme.tool_body_text,
                    LineKind::Spacer => theme.assistant_text,
                },
                alpha_scale,
            );

            fonts.draw_line(
                line.kind,
                &mut frame,
                args.width,
                args.height,
                text_area.x,
                y_u32,
                &line.text,
                color,
            );
        }

        if let Some(zoomed) = zoomed_frame.as_mut() {
            let progress = if frame_count <= 1 {
                1.0
            } else {
                frame_index as f32 / (frame_count - 1) as f32
            };
            let scale = 1.0 + (0.03 * progress);
            apply_center_zoom_nearest(&frame, zoomed, args.width, args.height, scale);
            writer.write_frame(zoomed)?;
        } else {
            writer.write_frame(&frame)?;
        }
    }

    writer.finish()?;

    Ok(AsciiStageRenderSummary {
        frame_count,
        duration_ms: total_ms,
        width: args.width,
        height: args.height,
    })
}

pub fn parse_ascii_stage_size(raw: &str) -> Result<(u32, u32)> {
    let value = raw.trim();
    let (width_raw, height_raw) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .ok_or_else(|| anyhow!("invalid --size '{}': expected WIDTHxHEIGHT", raw))?;
    let width = width_raw
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid --size '{}': width must be an integer", raw))?;
    let height = height_raw
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid --size '{}': height must be an integer", raw))?;
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        bail!(
            "invalid --size '{}': minimum supported size is {}x{}",
            raw,
            MIN_WIDTH,
            MIN_HEIGHT
        );
    }
    Ok((width, height))
}

pub fn parse_ascii_stage_script(raw: &str) -> Result<Vec<Block>> {
    let normalized = raw.replace("\r\n", "\n");
    if normalized.trim().is_empty() {
        bail!("vcr ascii stage: empty input transcript");
    }

    let mut blocks = Vec::new();
    let mut open: Option<OpenBlock> = None;

    for (idx, line) in normalized.lines().enumerate() {
        let line_number = idx + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if let Some(current) = open.as_mut() {
                current.lines.push(String::new());
            }
            continue;
        }

        if trimmed.starts_with("::") {
            flush_open_block(&mut open, &mut blocks);
            blocks.push(parse_directive(trimmed, line_number)?);
            continue;
        }

        if trimmed.starts_with('@') {
            flush_open_block(&mut open, &mut blocks);
            open = Some(parse_tag_header(trimmed, line_number)?);
            continue;
        }

        match open.as_mut() {
            Some(current) => current.lines.push(line.to_owned()),
            None => {
                bail!(
                    "invalid .vcrtxt format at line {}: content must be inside a tagged block. Use @user, @assistant, @tool <name>, or @system",
                    line_number
                )
            }
        }
    }

    flush_open_block(&mut open, &mut blocks);
    Ok(blocks)
}

#[derive(Debug, Clone)]
enum OpenKind {
    User,
    Assistant,
    System,
    Tool { name: String },
}

#[derive(Debug, Clone)]
struct OpenBlock {
    kind: OpenKind,
    lines: Vec<String>,
}

fn parse_tag_header(line: &str, line_number: usize) -> Result<OpenBlock> {
    let mut parts = line.split_whitespace();
    let tag = parts.next().ok_or_else(|| {
        anyhow!(
            "invalid .vcrtxt format at line {}: missing tag",
            line_number
        )
    })?;

    let kind = match tag {
        "@user" => OpenKind::User,
        "@assistant" => OpenKind::Assistant,
        "@system" => OpenKind::System,
        "@tool" => {
            let name = parts.collect::<Vec<_>>().join(" ").trim().to_owned();
            if name.is_empty() {
                bail!(
                    "invalid .vcrtxt format at line {}: @tool requires a tool name. Example: @tool figma-export",
                    line_number
                );
            }
            OpenKind::Tool { name }
        }
        _ => {
            bail!(
                "invalid .vcrtxt format at line {}: unknown tag '{}'. Use @user, @assistant, @tool <name>, or @system",
                line_number,
                tag
            )
        }
    };

    Ok(OpenBlock {
        kind,
        lines: Vec::new(),
    })
}

fn flush_open_block(open: &mut Option<OpenBlock>, blocks: &mut Vec<Block>) {
    let Some(block) = open.take() else {
        return;
    };

    let content = trim_outer_blank_lines(block.lines.join("\n"));
    if content.is_empty() {
        return;
    }

    let parsed = match block.kind {
        OpenKind::User => Block::User(content),
        OpenKind::Assistant => Block::Assistant(content),
        OpenKind::System => Block::System(content),
        OpenKind::Tool { name } => Block::Tool {
            name,
            body: content,
        },
    };
    blocks.push(parsed);
}

fn parse_directive(line: &str, line_number: usize) -> Result<Block> {
    let mut parts = line.split_whitespace();
    let directive = parts.next().ok_or_else(|| {
        anyhow!(
            "invalid .vcrtxt format at line {}: empty directive",
            line_number
        )
    })?;

    match directive {
        "::pause" => {
            let raw = parts.next().ok_or_else(|| {
                anyhow!(
                    "invalid .vcrtxt format at line {}: ::pause requires a duration (e.g. 400ms or 1.2s)",
                    line_number
                )
            })?;
            if parts.next().is_some() {
                bail!(
                    "invalid .vcrtxt format at line {}: ::pause accepts exactly one duration argument",
                    line_number
                );
            }
            let duration_ms = parse_duration_ms(raw).with_context(|| {
                format!(
                    "invalid .vcrtxt format at line {}: unable to parse duration '{}'",
                    line_number, raw
                )
            })?;
            Ok(Block::Pause(duration_ms))
        }
        "::type" => {
            let raw = parts.next().ok_or_else(|| {
                anyhow!(
                    "invalid .vcrtxt format at line {}: ::type requires fast, normal, or slow",
                    line_number
                )
            })?;
            if parts.next().is_some() {
                bail!(
                    "invalid .vcrtxt format at line {}: ::type accepts exactly one mode",
                    line_number
                );
            }
            let mode = match raw {
                "fast" => TypeSpeedMode::Fast,
                "normal" => TypeSpeedMode::Normal,
                "slow" => TypeSpeedMode::Slow,
                _ => {
                    bail!(
                        "invalid .vcrtxt format at line {}: unknown ::type mode '{}'. Use fast|normal|slow",
                        line_number,
                        raw
                    )
                }
            };
            Ok(Block::TypeSpeed(mode))
        }
        _ => {
            bail!(
                "invalid .vcrtxt format at line {}: unknown directive '{}'. Use ::pause or ::type",
                line_number,
                directive
            )
        }
    }
}

fn parse_duration_ms(raw: &str) -> Result<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("duration cannot be empty");
    }

    if let Some(ms) = raw.strip_suffix("ms") {
        let value: f64 = ms.trim().parse().context("duration is not a number")?;
        if !value.is_finite() || value < 0.0 {
            bail!("duration must be a non-negative finite number");
        }
        return Ok(value.round() as u64);
    }

    if let Some(seconds) = raw.strip_suffix('s') {
        let value: f64 = seconds.trim().parse().context("duration is not a number")?;
        if !value.is_finite() || value < 0.0 {
            bail!("duration must be a non-negative finite number");
        }
        return Ok((value * 1000.0).round() as u64);
    }

    bail!("duration must use 'ms' or 's' suffix")
}

fn trim_outer_blank_lines(mut value: String) -> String {
    while value.starts_with('\n') {
        value.remove(0);
    }
    while value.ends_with('\n') {
        value.pop();
    }
    value
}

fn build_actions(blocks: &[Block]) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut speed_mode = TypeSpeedMode::Normal;

    for block in blocks {
        match block {
            Block::Pause(ms) => actions.push(Action::Pause(*ms)),
            Block::TypeSpeed(mode) => speed_mode = *mode,
            Block::User(text) => {
                actions.push(Action::NewSection);
                actions.push(Action::AppendText {
                    kind: LineKind::User,
                    text: format_user_prompt(text),
                    typed: false,
                    speed: TypeSpeedMode::Normal,
                });
                actions.push(Action::Pause(180));
            }
            Block::Assistant(text) => {
                actions.push(Action::NewSection);
                actions.push(Action::AppendText {
                    kind: LineKind::Assistant,
                    text: text.clone(),
                    typed: true,
                    speed: speed_mode,
                });
                actions.push(Action::Pause(220));
            }
            Block::System(text) => {
                actions.push(Action::NewSection);
                actions.push(Action::AppendText {
                    kind: LineKind::System,
                    text: format!("[system] {text}"),
                    typed: false,
                    speed: TypeSpeedMode::Normal,
                });
                actions.push(Action::Pause(180));
            }
            Block::Tool { name, body } => {
                actions.push(Action::NewSection);
                actions.push(Action::AppendText {
                    kind: LineKind::ToolHeader,
                    text: format!("[tool: {name}]"),
                    typed: false,
                    speed: TypeSpeedMode::Normal,
                });
                actions.push(Action::AppendText {
                    kind: LineKind::ToolBody,
                    text: body.clone(),
                    typed: false,
                    speed: TypeSpeedMode::Normal,
                });
                actions.push(Action::Pause(260));
            }
        }
    }

    actions
}

fn build_timeline(actions: &[Action], seed: u64, speed: f32) -> (Vec<TimedEvent>, u64) {
    let mut events = Vec::new();
    let mut current_ms = 0_u64;

    for (action_index, action) in actions.iter().enumerate() {
        match action {
            Action::NewSection => events.push(TimedEvent {
                at_ms: current_ms,
                op: EventOp::NewSection,
            }),
            Action::Pause(ms) => current_ms = current_ms.saturating_add(*ms),
            Action::AppendText {
                kind,
                text,
                typed,
                speed: mode,
            } => {
                if *typed {
                    let content_len = text.chars().count();
                    for (char_index, ch) in text.chars().enumerate() {
                        events.push(TimedEvent {
                            at_ms: current_ms,
                            op: EventOp::AppendText {
                                kind: *kind,
                                text: ch.to_string(),
                            },
                        });
                        let delay = typing_delay_ms(
                            *mode,
                            speed,
                            seed,
                            action_index,
                            char_index,
                            content_len,
                            ch,
                        );
                        current_ms = current_ms.saturating_add(delay);
                    }
                } else {
                    events.push(TimedEvent {
                        at_ms: current_ms,
                        op: EventOp::AppendText {
                            kind: *kind,
                            text: text.clone(),
                        },
                    });
                }
            }
        }
    }

    current_ms = current_ms.saturating_add(DEFAULT_FINAL_HOLD_MS);
    (events, current_ms)
}

fn typing_delay_ms(
    mode: TypeSpeedMode,
    speed: f32,
    seed: u64,
    action_index: usize,
    char_index: usize,
    content_len: usize,
    character: char,
) -> u64 {
    let base_ms = match mode {
        TypeSpeedMode::Fast => 16_u64,
        TypeSpeedMode::Normal => 28_u64,
        TypeSpeedMode::Slow => 42_u64,
    };
    let scaled = ((base_ms as f64) / speed.max(0.05) as f64).round() as i64;
    let mut delay = scaled.clamp(4, 250);

    let jitter_seed = hash_u64(
        seed ^ ((action_index as u64) << 32) ^ (char_index as u64) ^ ((content_len as u64) << 17),
    );
    let jitter = (jitter_seed % 7) as i64 - 3;
    delay = (delay + jitter).max(3);
    delay += punctuation_pause_ms(character, speed) as i64;
    delay = delay.min(500);

    delay as u64
}

fn punctuation_pause_ms(character: char, speed: f32) -> u64 {
    let base = match character {
        '.' | '!' | '?' => 95_u64,
        ':' => 75_u64,
        _ => 0_u64,
    };
    if base == 0 {
        return 0;
    }
    let scaled = ((base as f64) / speed.max(0.05) as f64).round() as i64;
    scaled.clamp(12, 220) as u64
}

fn hash_u64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^= value >> 33;
    value
}

fn format_user_prompt(text: &str) -> String {
    let mut formatted = String::new();
    for (idx, line) in text.lines().enumerate() {
        if idx > 0 {
            formatted.push('\n');
        }
        if idx == 0 {
            formatted.push_str("> ");
        } else {
            formatted.push_str("  ");
        }
        formatted.push_str(line);
    }
    if formatted.is_empty() {
        "> ".to_owned()
    } else {
        formatted
    }
}

fn draw_static_frame(
    frame: &mut [u8],
    width: u32,
    height: u32,
    terminal: Rect,
    header_h: u32,
    text_area: Rect,
    chrome: bool,
    theme: &Theme,
) {
    fill_rect(frame, width, height, 0, 0, width, height, theme.bg);

    if chrome {
        let corner_radius = (terminal.h / 32).clamp(12, 24);
        fill_rounded_rect(
            frame,
            width,
            height,
            Rect {
                x: terminal.x.saturating_add(8),
                y: terminal.y.saturating_add(10),
                w: terminal.w,
                h: terminal.h,
            },
            corner_radius,
            [0, 0, 0, 140],
        );
        fill_rounded_rect(
            frame,
            width,
            height,
            terminal,
            corner_radius,
            theme.terminal_bg,
        );
        draw_rounded_rect_border(
            frame,
            width,
            height,
            terminal,
            corner_radius,
            theme.terminal_border,
            2,
        );

        fill_rounded_rect(
            frame,
            width,
            height,
            Rect {
                x: terminal.x + 1,
                y: terminal.y + 1,
                w: terminal.w.saturating_sub(2),
                h: header_h,
            },
            corner_radius.saturating_sub(1),
            theme.header_bg,
        );

        let dot_size = (header_h / 3).max(8);
        let dot_y = terminal.y + header_h / 2 - dot_size / 2;
        fill_rounded_rect(
            frame,
            width,
            height,
            Rect {
                x: terminal.x + 14,
                y: dot_y,
                w: dot_size,
                h: dot_size,
            },
            dot_size / 2,
            [95, 95, 95, 255],
        );
        fill_rounded_rect(
            frame,
            width,
            height,
            Rect {
                x: terminal.x + 14 + dot_size + 8,
                y: dot_y,
                w: dot_size,
                h: dot_size,
            },
            dot_size / 2,
            [130, 130, 130, 255],
        );
        fill_rounded_rect(
            frame,
            width,
            height,
            Rect {
                x: terminal.x + 14 + (dot_size + 8) * 2,
                y: dot_y,
                w: dot_size,
                h: dot_size,
            },
            dot_size / 2,
            [95, 95, 95, 255],
        );

        let title_width = terminal.w.min(300);
        fill_rect(
            frame,
            width,
            height,
            terminal.x + terminal.w / 2 - title_width / 2,
            terminal.y + header_h / 2 - 1,
            title_width,
            2,
            theme.title_text,
        );
        apply_inner_vignette(frame, width, height, terminal, 10, 18);
    } else {
        fill_rect(
            frame,
            width,
            height,
            terminal.x.saturating_add(6),
            terminal.y.saturating_add(8),
            terminal.w,
            terminal.h,
            [0, 0, 0, 120],
        );
        fill_rect(
            frame,
            width,
            height,
            terminal.x,
            terminal.y,
            terminal.w,
            terminal.h,
            theme.terminal_bg,
        );
        draw_rect_border(frame, width, height, terminal, theme.terminal_border, 2);
    }

    for y in (text_area.y..text_area.y.saturating_add(text_area.h)).step_by(4) {
        fill_rect(
            frame,
            width,
            height,
            text_area.x.saturating_sub(8),
            y,
            text_area.w.saturating_add(16),
            1,
            theme.scanline,
        );
    }
}

fn fill_rounded_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    radius: u32,
    color: [u8; 4],
) {
    let x0 = rect.x.min(width);
    let y0 = rect.y.min(height);
    let x1 = x0.saturating_add(rect.w).min(width);
    let y1 = y0.saturating_add(rect.h).min(height);
    let radius = radius.min(rect.w / 2).min(rect.h / 2);

    for yy in y0..y1 {
        let row_start = (yy * width * 4) as usize;
        for xx in x0..x1 {
            if rounded_rect_contains(xx, yy, rect, radius) {
                let idx = row_start + (xx * 4) as usize;
                blend_pixel(frame, idx, color);
            }
        }
    }
}

fn draw_rounded_rect_border(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    radius: u32,
    color: [u8; 4],
    thickness: u32,
) {
    let thickness = thickness.min(rect.w / 2).min(rect.h / 2).max(1);
    let inner = Rect {
        x: rect.x.saturating_add(thickness),
        y: rect.y.saturating_add(thickness),
        w: rect.w.saturating_sub(thickness * 2),
        h: rect.h.saturating_sub(thickness * 2),
    };
    let outer_radius = radius.min(rect.w / 2).min(rect.h / 2);
    let inner_radius = outer_radius.saturating_sub(thickness);

    let x0 = rect.x.min(width);
    let y0 = rect.y.min(height);
    let x1 = x0.saturating_add(rect.w).min(width);
    let y1 = y0.saturating_add(rect.h).min(height);

    for yy in y0..y1 {
        let row_start = (yy * width * 4) as usize;
        for xx in x0..x1 {
            if !rounded_rect_contains(xx, yy, rect, outer_radius) {
                continue;
            }
            if inner.w > 0 && inner.h > 0 && rounded_rect_contains(xx, yy, inner, inner_radius) {
                continue;
            }
            let idx = row_start + (xx * 4) as usize;
            blend_pixel(frame, idx, color);
        }
    }
}

fn rounded_rect_contains(x: u32, y: u32, rect: Rect, radius: u32) -> bool {
    if rect.w == 0 || rect.h == 0 {
        return false;
    }
    if radius == 0 {
        return x >= rect.x
            && x < rect.x.saturating_add(rect.w)
            && y >= rect.y
            && y < rect.y.saturating_add(rect.h);
    }

    let left = rect.x;
    let right = rect.x + rect.w - 1;
    let top = rect.y;
    let bottom = rect.y + rect.h - 1;

    let rx = radius as i32;
    let xi = x as i32;
    let yi = y as i32;

    if x >= left.saturating_add(radius)
        && x <= right.saturating_sub(radius)
        && y >= top
        && y <= bottom
    {
        return true;
    }
    if y >= top.saturating_add(radius)
        && y <= bottom.saturating_sub(radius)
        && x >= left
        && x <= right
    {
        return true;
    }

    let corners = [
        (
            left.saturating_add(radius) as i32,
            top.saturating_add(radius) as i32,
        ),
        (
            right.saturating_sub(radius) as i32,
            top.saturating_add(radius) as i32,
        ),
        (
            left.saturating_add(radius) as i32,
            bottom.saturating_sub(radius) as i32,
        ),
        (
            right.saturating_sub(radius) as i32,
            bottom.saturating_sub(radius) as i32,
        ),
    ];

    let radius_sq = rx * rx;
    corners.into_iter().any(|(cx, cy)| {
        let dx = xi - cx;
        let dy = yi - cy;
        dx * dx + dy * dy <= radius_sq
    })
}

fn apply_inner_vignette(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    layers: u32,
    max_alpha: u8,
) {
    let layers = layers.max(1);
    for i in 0..layers {
        let progress = i as f32 / layers as f32;
        let alpha = ((1.0 - progress) * max_alpha as f32).round() as u8;
        if alpha == 0 {
            continue;
        }
        let inset = i;
        let layer_rect = Rect {
            x: rect.x.saturating_add(inset),
            y: rect.y.saturating_add(inset),
            w: rect.w.saturating_sub(inset * 2),
            h: rect.h.saturating_sub(inset * 2),
        };
        if layer_rect.w < 2 || layer_rect.h < 2 {
            break;
        }
        draw_rect_border(frame, width, height, layer_rect, [0, 0, 0, alpha], 1);
    }
}

fn draw_rect_border(
    frame: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    color: [u8; 4],
    thickness: u32,
) {
    fill_rect(
        frame, width, height, rect.x, rect.y, rect.w, thickness, color,
    );
    fill_rect(
        frame,
        width,
        height,
        rect.x,
        rect.y + rect.h.saturating_sub(thickness),
        rect.w,
        thickness,
        color,
    );
    fill_rect(
        frame, width, height, rect.x, rect.y, thickness, rect.h, color,
    );
    fill_rect(
        frame,
        width,
        height,
        rect.x + rect.w.saturating_sub(thickness),
        rect.y,
        thickness,
        rect.h,
        color,
    );
}

fn fill_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    let x0 = x.min(width);
    let y0 = y.min(height);
    let x1 = x0.saturating_add(w).min(width);
    let y1 = y0.saturating_add(h).min(height);

    for yy in y0..y1 {
        let row_start = (yy * width * 4) as usize;
        for xx in x0..x1 {
            let idx = row_start + (xx * 4) as usize;
            blend_pixel(frame, idx, color);
        }
    }
}

fn apply_center_zoom_nearest(src: &[u8], dst: &mut [u8], width: u32, height: u32, scale: f32) {
    let width_f = width as f32;
    let height_f = height as f32;
    let cx = (width_f - 1.0) * 0.5;
    let cy = (height_f - 1.0) * 0.5;
    let inv_scale = 1.0 / scale.max(0.01);
    let max_x = width.saturating_sub(1) as f32;
    let max_y = height.saturating_sub(1) as f32;

    for y in 0..height {
        let y_f = y as f32;
        let src_y = ((y_f - cy) * inv_scale + cy).clamp(0.0, max_y).round() as u32;
        for x in 0..width {
            let x_f = x as f32;
            let src_x = ((x_f - cx) * inv_scale + cx).clamp(0.0, max_x).round() as u32;
            let src_idx = ((src_y * width + src_x) * 4) as usize;
            let dst_idx = ((y * width + x) * 4) as usize;
            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
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

fn scale_alpha(color: [u8; 4], factor: f32) -> [u8; 4] {
    let alpha = ((color[3] as f32) * factor.clamp(0.0, 1.0)).round() as u8;
    [color[0], color[1], color[2], alpha]
}

fn ease_out_cubic(t: f32) -> f32 {
    let p = t.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(3)
}

fn line_animation_at(line: &LineRecord, now_ms: u64, duration_ms: u64) -> (f32, i32) {
    if line.animation != LineAnimation::ToolPop {
        return (1.0, 0);
    }
    if now_ms <= line.born_ms {
        return (0.0, 8);
    }
    let elapsed = now_ms.saturating_sub(line.born_ms);
    if elapsed >= duration_ms {
        return (1.0, 0);
    }
    let t = elapsed as f32 / duration_ms.max(1) as f32;
    let eased = ease_out_cubic(t);
    let slide = ((1.0 - eased) * 8.0).round() as i32;
    (eased, slide)
}

fn resolve_void_theme(raw: &str) -> Result<Theme> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized != "void" && !normalized.is_empty() {
        bail!("unknown --theme '{}'. Supported: void", raw);
    }

    Ok(Theme {
        bg: [3, 3, 4, 255],
        terminal_bg: [8, 8, 10, 246],
        terminal_border: [64, 64, 68, 255],
        header_bg: [15, 15, 18, 255],
        title_text: [78, 78, 84, 255],
        user_text: [232, 240, 244, 255],
        assistant_text: [214, 218, 224, 255],
        system_text: [132, 136, 140, 255],
        tool_header_text: [120, 230, 187, 255],
        tool_header_bg: [24, 38, 33, 220],
        tool_body_text: [190, 224, 209, 255],
        tool_body_bg: [16, 24, 22, 220],
        scanline: [255, 255, 255, 14],
    })
}

struct AsciiStageFfmpegWriter {
    child: Child,
    stdin: ChildStdin,
    frame_size: usize,
}

impl AsciiStageFfmpegWriter {
    fn spawn(width: u32, height: u32, fps: u32, output_path: &Path) -> Result<Self> {
        let frame_size = usize::try_from(width)
            .ok()
            .and_then(|w| {
                usize::try_from(height)
                    .ok()
                    .map(|h| w.saturating_mul(h).saturating_mul(4))
            })
            .context("ascii stage frame size overflow")?;

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
            .arg("-an");

        match output_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "mov" => {
                command
                    .arg("-c:v")
                    .arg("prores_ks")
                    .arg("-profile:v")
                    .arg("4444")
                    .arg("-pix_fmt")
                    .arg("yuva444p10le")
                    .arg("-map_metadata")
                    .arg("-1")
                    .arg("-metadata")
                    .arg("creation_time=1970-01-01T00:00:00Z");
            }
            _ => {
                command
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-preset")
                    .arg("medium")
                    .arg("-crf")
                    .arg("18")
                    .arg("-pix_fmt")
                    .arg("yuv420p")
                    .arg("-threads")
                    .arg("1")
                    .arg("-fflags")
                    .arg("+bitexact")
                    .arg("-flags:v")
                    .arg("+bitexact")
                    .arg("-map_metadata")
                    .arg("-1")
                    .arg("-metadata")
                    .arg("creation_time=1970-01-01T00:00:00Z")
                    .arg("-movflags")
                    .arg("+faststart");
            }
        }

        command
            .arg(output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        let mut child = command.spawn().map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                anyhow!(
                    "ffmpeg was not found on PATH. Install ffmpeg and verify `ffmpeg -version` works before running `vcr ascii stage`."
                )
            } else {
                anyhow!("failed to spawn ffmpeg sidecar process: {error}")
            }
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to capture ffmpeg stdin"))?;

        Ok(Self {
            child,
            stdin,
            frame_size,
        })
    }

    fn write_frame(&mut self, frame: &[u8]) -> Result<()> {
        if frame.len() != self.frame_size {
            bail!(
                "ascii stage frame size mismatch: expected {} bytes, got {}",
                self.frame_size,
                frame.len()
            );
        }
        self.stdin
            .write_all(frame)
            .context("failed to write ascii stage frame to ffmpeg stdin")
    }

    fn finish(mut self) -> Result<()> {
        self.stdin
            .flush()
            .context("failed to flush ffmpeg stdin for ascii stage render")?;
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

#[cfg(test)]
mod tests {
    use super::{parse_ascii_stage_script, parse_ascii_stage_size, Block, TypeSpeedMode};

    #[test]
    fn parse_tagged_transcript_with_directives() {
        let input = r#"@user
Make a logo in ASCII for VCR.

@assistant
Here you go:
 __     __
/ /__  / /_
/ / _ \/ __/
/_/\___/\__/

::pause 600ms

@tool figma-export
exported: hero-logo.svg
warnings: 0

::type slow

@assistant
Clean export. Want a glitch version too?
"#;

        let blocks = parse_ascii_stage_script(input).expect("transcript should parse");
        assert_eq!(
            blocks,
            vec![
                Block::User("Make a logo in ASCII for VCR.".to_owned()),
                Block::Assistant(
                    "Here you go:\n __     __\n/ /__  / /_\n/ / _ \\/ __/\n/_/\\___/\\__/"
                        .to_owned()
                ),
                Block::Pause(600),
                Block::Tool {
                    name: "figma-export".to_owned(),
                    body: "exported: hero-logo.svg\nwarnings: 0".to_owned(),
                },
                Block::TypeSpeed(TypeSpeedMode::Slow),
                Block::Assistant("Clean export. Want a glitch version too?".to_owned()),
            ]
        );
    }

    #[test]
    fn parse_size_accepts_wxh_format() {
        assert_eq!(
            parse_ascii_stage_size("1920x1080").expect("size should parse"),
            (1920, 1080)
        );
    }

    #[test]
    fn timeline_is_deterministic_for_same_seed() {
        let blocks = vec![Block::Assistant("Deterministic typing.".to_owned())];
        let actions = super::build_actions(&blocks);

        let (first_events, first_total) = super::build_timeline(&actions, 7, 1.0);
        let (second_events, second_total) = super::build_timeline(&actions, 7, 1.0);
        assert_eq!(first_total, second_total);
        assert_eq!(first_events, second_events);

        let (third_events, third_total) = super::build_timeline(&actions, 9, 1.0);
        assert!(
            first_total != third_total || first_events != third_events,
            "different seed should affect timing jitter"
        );
    }

    #[test]
    fn typing_delay_adds_punctuation_pause() {
        let plain = super::typing_delay_ms(TypeSpeedMode::Normal, 1.0, 0, 0, 0, 10, 'a');
        let period = super::typing_delay_ms(TypeSpeedMode::Normal, 1.0, 0, 0, 0, 10, '.');
        let colon = super::typing_delay_ms(TypeSpeedMode::Normal, 1.0, 0, 0, 0, 10, ':');
        assert!(period > plain, "period should add pause");
        assert!(colon > plain, "colon should add pause");
        assert!(period <= 500, "delay should stay bounded");
    }
}
