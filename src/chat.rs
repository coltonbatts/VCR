use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::font_assets::{
    ensure_supported_codepoints, font_path, read_verified_font_bytes, verify_geist_pixel_bundle,
};
use anyhow::{anyhow, bail, Context, Result};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_FONT_SIZE: f32 = 20.0;
const FINAL_HOLD_MS: u64 = 700;

#[derive(Debug, Clone)]
pub struct ChatRenderArgs {
    pub input: PathBuf,
    pub output: PathBuf,
    pub theme: String,
    pub fps: u32,
    pub speed: f32,
    pub seed: u64,
}

#[derive(Debug, Clone)]
pub struct ChatRenderSummary {
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

#[derive(Debug, Clone)]
enum EventOp {
    NewSection,
    AppendText { kind: LineKind, text: String },
    SetCursor(bool),
}

#[derive(Debug, Clone)]
struct TimedEvent {
    at_ms: u64,
    op: EventOp,
}

#[derive(Debug, Clone)]
struct Theme {
    font_file: &'static str,
    bg_top: [u8; 4],
    bg_bottom: [u8; 4],
    terminal_bg: [u8; 4],
    terminal_header: [u8; 4],
    terminal_border: [u8; 4],
    user_text: [u8; 4],
    assistant_text: [u8; 4],
    system_text: [u8; 4],
    tool_header_text: [u8; 4],
    tool_body_text: [u8; 4],
    tool_body_bg: [u8; 4],
    cursor: [u8; 4],
}

#[derive(Debug, Clone)]
struct LineRecord {
    kind: LineKind,
    text: String,
}

#[derive(Debug, Clone)]
struct CurrentLine {
    kind: LineKind,
    text: String,
    len: usize,
}

#[derive(Debug, Clone)]
struct TerminalState {
    lines: Vec<LineRecord>,
    current: Option<CurrentLine>,
    max_cols: usize,
    cursor_active: bool,
}

impl TerminalState {
    fn new(max_cols: usize) -> Self {
        Self {
            lines: Vec::new(),
            current: None,
            max_cols,
            cursor_active: false,
        }
    }

    fn apply(&mut self, op: &EventOp) {
        match op {
            EventOp::NewSection => self.insert_section_break(),
            EventOp::AppendText { kind, text } => self.append_text(*kind, text),
            EventOp::SetCursor(active) => self.cursor_active = *active,
        }
    }

    fn insert_section_break(&mut self) {
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
            });
        }
    }

    fn append_text(&mut self, kind: LineKind, text: &str) {
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
            });
        }
    }

    fn snapshot_lines(&self) -> Vec<LineRecord> {
        let mut all = self.lines.clone();
        if let Some(current) = &self.current {
            all.push(LineRecord {
                kind: current.kind,
                text: current.text.clone(),
            });
        }
        all
    }

    fn cursor_position(
        &self,
        visible_rows: usize,
        text_area_x: u32,
        text_area_y: u32,
        cell_w: u32,
        line_h: u32,
    ) -> Option<(u32, u32)> {
        if !self.cursor_active {
            return None;
        }

        let lines = self.snapshot_lines();
        if lines.is_empty() {
            return Some((text_area_x, text_area_y));
        }

        let start = lines.len().saturating_sub(visible_rows);
        let visible = &lines[start..];

        let current = visible
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.kind == LineKind::Assistant)
            .or_else(|| visible.iter().enumerate().rev().next())?;

        let row = current.0 as u32;
        let col = current.1.text.chars().count() as u32;
        Some((
            text_area_x + col.saturating_mul(cell_w),
            text_area_y + row.saturating_mul(line_h),
        ))
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
    fn new(manifest_root: &Path, font_file: &str, font_size: f32) -> Result<Self> {
        let font_bytes = read_verified_font_bytes(manifest_root, font_file)?;
        let font = Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|error| anyhow!("failed to parse Geist Pixel font {font_file}: {error}"))?;
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
    ) -> Result<()> {
        ensure_supported_codepoints(&self.font, text, "chat")?;
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
        Ok(())
    }
}

pub fn render_chat_video(args: &ChatRenderArgs) -> Result<ChatRenderSummary> {
    if args.fps == 0 {
        bail!("--fps must be > 0");
    }
    if !args.speed.is_finite() || args.speed <= 0.0 {
        bail!("--speed must be > 0");
    }

    let raw = fs::read_to_string(&args.input)
        .with_context(|| format!("failed to read chat script {}", args.input.display()))?;
    let blocks = parse_chat_script(&raw)?;
    if blocks.is_empty() {
        bail!("vcr chat render: empty input script");
    }

    let theme = resolve_theme(&args.theme)?;
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut painter = TextPainter::new(manifest_root, theme.font_file, DEFAULT_FONT_SIZE)?;

    let actions = build_actions(&blocks);
    let (events, total_ms) = build_timeline(&actions, args.seed, args.speed);

    let frame_count_u64 = ((total_ms.saturating_mul(args.fps as u64) + 999) / 1000)
        .saturating_add(1)
        .max(1);
    let frame_count = u32::try_from(frame_count_u64).context("chat render frame count overflow")?;

    let mut writer =
        ChatFfmpegWriter::spawn(DEFAULT_WIDTH, DEFAULT_HEIGHT, args.fps, &args.output)?;

    let terminal_rect = Rect {
        x: 48,
        y: 36,
        w: DEFAULT_WIDTH.saturating_sub(96),
        h: DEFAULT_HEIGHT.saturating_sub(72),
    };
    let text_area = Rect {
        x: terminal_rect.x + 24,
        y: terminal_rect.y + 62,
        w: terminal_rect.w.saturating_sub(48),
        h: terminal_rect.h.saturating_sub(84),
    };

    let cell_width = painter.cell_width().max(8);
    let line_height = painter.line_height().max(16);
    let max_cols = ((text_area.w / cell_width).max(8)) as usize;
    let visible_rows = ((text_area.h / line_height).max(4)) as usize;

    let mut state = TerminalState::new(max_cols);
    let mut event_cursor = 0usize;
    let mut base_frame = vec![0_u8; (DEFAULT_WIDTH * DEFAULT_HEIGHT * 4) as usize];
    draw_static_frame(
        &mut base_frame,
        DEFAULT_WIDTH,
        DEFAULT_HEIGHT,
        terminal_rect,
        &theme,
    );

    for frame_index in 0..frame_count {
        let mut frame = base_frame.clone();
        let t_ms = (u64::from(frame_index) * 1000) / u64::from(args.fps);

        while event_cursor < events.len() && events[event_cursor].at_ms <= t_ms {
            state.apply(&events[event_cursor].op);
            event_cursor += 1;
        }

        let lines = state.snapshot_lines();
        let start = lines.len().saturating_sub(visible_rows);
        let visible = &lines[start..];

        for (row_idx, line) in visible.iter().enumerate() {
            let y = text_area.y + (row_idx as u32 * line_height);
            if line.kind == LineKind::ToolBody {
                fill_rect(
                    &mut frame,
                    DEFAULT_WIDTH,
                    DEFAULT_HEIGHT,
                    text_area.x.saturating_sub(8),
                    y.saturating_sub(2),
                    text_area.w,
                    line_height,
                    theme.tool_body_bg,
                );
                fill_rect(
                    &mut frame,
                    DEFAULT_WIDTH,
                    DEFAULT_HEIGHT,
                    text_area.x.saturating_sub(8),
                    y.saturating_sub(2),
                    3,
                    line_height,
                    theme.tool_header_text,
                );
            }

            let color = match line.kind {
                LineKind::User => theme.user_text,
                LineKind::Assistant => theme.assistant_text,
                LineKind::System => theme.system_text,
                LineKind::ToolHeader => theme.tool_header_text,
                LineKind::ToolBody => theme.tool_body_text,
                LineKind::Spacer => theme.assistant_text,
            };

            painter.draw_line(
                &mut frame,
                DEFAULT_WIDTH,
                DEFAULT_HEIGHT,
                text_area.x,
                y,
                &line.text,
                color,
            )?;
        }

        if let Some((cursor_x, cursor_y)) = state.cursor_position(
            visible_rows,
            text_area.x,
            text_area.y,
            cell_width,
            line_height,
        ) {
            fill_rect(
                &mut frame,
                DEFAULT_WIDTH,
                DEFAULT_HEIGHT,
                cursor_x,
                cursor_y.saturating_add(2),
                cell_width.saturating_sub(2).max(2),
                line_height.saturating_sub(4).max(3),
                theme.cursor,
            );
        }

        writer.write_frame(&frame)?;
    }

    writer.finish()?;

    Ok(ChatRenderSummary {
        frame_count,
        duration_ms: total_ms,
        width: DEFAULT_WIDTH,
        height: DEFAULT_HEIGHT,
    })
}

pub fn parse_chat_script(raw: &str) -> Result<Vec<Block>> {
    let normalized = raw.replace("\r\n", "\n");
    if normalized.trim().is_empty() {
        bail!("vcr chat render: empty input script");
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
                    "invalid .vcrchat format at line {}: content must be inside a tagged block. Hint: start with @user, @assistant, @tool <name>, or @system",
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
            "invalid .vcrchat format at line {}: missing tag",
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
                    "invalid .vcrchat format at line {}: @tool requires a tool name. Hint: use '@tool ffmpeg'",
                    line_number
                );
            }
            OpenKind::Tool { name }
        }
        _ => {
            bail!(
                "invalid .vcrchat format at line {}: unknown tag '{}'. Hint: use @user, @assistant, @tool <name>, or @system",
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
            "invalid .vcrchat format at line {}: empty directive",
            line_number
        )
    })?;

    match directive {
        "::pause" | "::wait" => {
            let raw = parts.next().ok_or_else(|| {
                anyhow!(
                    "invalid .vcrchat format at line {}: {} requires a duration. Hint: ::pause 400ms",
                    line_number,
                    directive
                )
            })?;
            if parts.next().is_some() {
                bail!(
                    "invalid .vcrchat format at line {}: {} accepts only one duration argument",
                    line_number,
                    directive
                );
            }
            let duration_ms = parse_duration_ms(raw).with_context(|| {
                format!(
                    "invalid .vcrchat format at line {}: unable to parse duration '{}'. Hint: use 400ms or 1.2s",
                    line_number, raw
                )
            })?;
            Ok(Block::Pause(duration_ms))
        }
        "::type" => {
            let raw = parts.next().ok_or_else(|| {
                anyhow!(
                    "invalid .vcrchat format at line {}: ::type requires fast, normal, or slow",
                    line_number
                )
            })?;
            if parts.next().is_some() {
                bail!(
                    "invalid .vcrchat format at line {}: ::type accepts only one mode",
                    line_number
                );
            }
            let mode = match raw {
                "fast" => TypeSpeedMode::Fast,
                "normal" => TypeSpeedMode::Normal,
                "slow" => TypeSpeedMode::Slow,
                _ => {
                    bail!(
                        "invalid .vcrchat format at line {}: unknown ::type mode '{}'. Hint: fast|normal|slow",
                        line_number,
                        raw
                    )
                }
            };
            Ok(Block::TypeSpeed(mode))
        }
        _ => {
            bail!(
                "invalid .vcrchat format at line {}: unknown directive '{}'. Hint: use ::pause, ::wait, or ::type",
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
                    events.push(TimedEvent {
                        at_ms: current_ms,
                        op: EventOp::SetCursor(true),
                    });

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
                        );
                        current_ms = current_ms.saturating_add(delay);
                    }

                    events.push(TimedEvent {
                        at_ms: current_ms,
                        op: EventOp::SetCursor(false),
                    });
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

    current_ms = current_ms.saturating_add(FINAL_HOLD_MS);
    (events, current_ms)
}

fn typing_delay_ms(
    mode: TypeSpeedMode,
    speed: f32,
    seed: u64,
    action_index: usize,
    char_index: usize,
    content_len: usize,
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

    delay as u64
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

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn draw_static_frame(frame: &mut [u8], width: u32, height: u32, terminal: Rect, theme: &Theme) {
    for y in 0..height {
        let t = y as f32 / (height.max(1) as f32);
        let color = [
            lerp_u8(theme.bg_top[0], theme.bg_bottom[0], t),
            lerp_u8(theme.bg_top[1], theme.bg_bottom[1], t),
            lerp_u8(theme.bg_top[2], theme.bg_bottom[2], t),
            255,
        ];
        fill_rect(frame, width, height, 0, y, width, 1, color);
    }

    fill_rect(
        frame,
        width,
        height,
        terminal.x.saturating_add(4),
        terminal.y.saturating_add(6),
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

    fill_rect(
        frame,
        width,
        height,
        terminal.x,
        terminal.y,
        terminal.w,
        34,
        theme.terminal_header,
    );

    let dot_y = terminal.y + 12;
    fill_rect(
        frame,
        width,
        height,
        terminal.x + 14,
        dot_y,
        10,
        10,
        [255, 95, 86, 255],
    );
    fill_rect(
        frame,
        width,
        height,
        terminal.x + 30,
        dot_y,
        10,
        10,
        [255, 189, 46, 255],
    );
    fill_rect(
        frame,
        width,
        height,
        terminal.x + 46,
        dot_y,
        10,
        10,
        [39, 201, 63, 255],
    );
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let a = a as f32;
    let b = b as f32;
    (a + (b - a) * t.clamp(0.0, 1.0)).round() as u8
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

fn resolve_theme(raw: &str) -> Result<Theme> {
    let normalized = raw.trim().to_ascii_lowercase();
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let font_file: &'static str = match normalized.as_str() {
        "" | "default" | "geist" | "geist-pixel" | "line" => "GeistPixel-Line.ttf",
        "square" | "geist-square" => "GeistPixel-Square.ttf",
        "grid" | "geist-grid" => "GeistPixel-Grid.ttf",
        "circle" | "geist-circle" => "GeistPixel-Circle.ttf",
        "triangle" | "geist-triangle" => "GeistPixel-Triangle.ttf",
        _ => {
            bail!(
                "unknown --theme '{}'. Supported: geist-pixel, line, square, grid, circle, triangle",
                raw
            )
        }
    };

    verify_geist_pixel_bundle(manifest_root)?;
    let _ = font_path(manifest_root, font_file)?;

    Ok(Theme {
        font_file,
        bg_top: [12, 15, 24, 255],
        bg_bottom: [22, 30, 44, 255],
        terminal_bg: [8, 12, 18, 245],
        terminal_header: [20, 28, 38, 255],
        terminal_border: [64, 84, 108, 255],
        user_text: [143, 205, 255, 255],
        assistant_text: [233, 239, 245, 255],
        system_text: [245, 196, 120, 255],
        tool_header_text: [123, 243, 171, 255],
        tool_body_text: [205, 239, 216, 255],
        tool_body_bg: [22, 46, 34, 220],
        cursor: [226, 236, 246, 255],
    })
}

struct ChatFfmpegWriter {
    child: Child,
    stdin: ChildStdin,
    frame_size: usize,
}

impl ChatFfmpegWriter {
    fn spawn(width: u32, height: u32, fps: u32, output_path: &Path) -> Result<Self> {
        let frame_size = usize::try_from(width)
            .ok()
            .and_then(|w| {
                usize::try_from(height)
                    .ok()
                    .map(|h| w.saturating_mul(h).saturating_mul(4))
            })
            .context("chat render frame size overflow")?;

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
                    .arg("yuva444p10le");
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
                    "ffmpeg was not found on PATH. Install ffmpeg and verify `ffmpeg -version` works before running `vcr chat render`."
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
                "chat render frame size mismatch: expected {} bytes, got {}",
                self.frame_size,
                frame.len()
            );
        }
        self.stdin
            .write_all(frame)
            .context("failed to write chat frame to ffmpeg stdin")
    }

    fn finish(mut self) -> Result<()> {
        self.stdin
            .flush()
            .context("failed to flush ffmpeg stdin for chat render")?;
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
    use super::{parse_chat_script, Block, TypeSpeedMode};

    #[test]
    fn parse_tagged_block_example_with_directives() {
        let input = r#"@user
Write a haiku about FFmpeg.

@assistant
Sure. Here you go:
The frames fall in line
Code hums under midnight screens
Video wakes, precise

@tool ffmpeg
command: ffmpeg -i input.mov -vf scale=1920:1080 output.mp4

@assistant
Rendered. Want it in vertical too?

::pause 600ms

::type slow

@user
Yes, 9:16.
"#;

        let blocks = parse_chat_script(input).expect("example should parse");
        assert_eq!(blocks.len(), 7);
        assert_eq!(
            blocks,
            vec![
                Block::User("Write a haiku about FFmpeg.".to_owned()),
                Block::Assistant(
                    "Sure. Here you go:\nThe frames fall in line\nCode hums under midnight screens\nVideo wakes, precise"
                        .to_owned()
                ),
                Block::Tool {
                    name: "ffmpeg".to_owned(),
                    body: "command: ffmpeg -i input.mov -vf scale=1920:1080 output.mp4".to_owned()
                },
                Block::Assistant("Rendered. Want it in vertical too?".to_owned()),
                Block::Pause(600),
                Block::TypeSpeed(TypeSpeedMode::Slow),
                Block::User("Yes, 9:16.".to_owned()),
            ]
        );
    }
}
