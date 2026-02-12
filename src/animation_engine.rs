use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::ascii_atlas::GeistPixelAtlas;
use crate::schema::AsciiFontVariant;

pub const DEFAULT_ANIMATIONS_ROOT: &str = "assets/animations";
const DEFAULT_SOURCE_FPS: u32 = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnimationMetadata {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artist_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub credit: Option<String>,
}

impl AnimationMetadata {
    pub fn credit_line(&self, fallback_title: &str) -> String {
        if let Some(credit) = &self.credit {
            if !credit.trim().is_empty() {
                return credit.trim().to_owned();
            }
        }

        let title = if self.title.trim().is_empty() {
            fallback_title
        } else {
            self.title.trim()
        };
        let artist = if self.artist.trim().is_empty() {
            "unknown artist"
        } else {
            self.artist.trim()
        };

        match (&self.source_url, &self.license) {
            (Some(source), Some(license)) => {
                format!("{title} by {artist} ({license}) — {source}")
            }
            (Some(source), None) => format!("{title} by {artist} — {source}"),
            (None, Some(license)) => format!("{title} by {artist} ({license})"),
            (None, None) => format!("{title} by {artist}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsciiAnimationFrame {
    lines: Vec<String>,
    columns: u32,
    rows: u32,
}

impl AsciiAnimationFrame {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn columns(&self) -> u32 {
        self.columns
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    pub fn to_text(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }
}

#[derive(Debug, Clone)]
pub struct AsciiAnimationClip {
    name: String,
    source_fps: u32,
    frames: Vec<AsciiAnimationFrame>,
    metadata: AnimationMetadata,
}

impl AsciiAnimationClip {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source_fps(&self) -> u32 {
        self.source_fps
    }

    pub fn frames(&self) -> &[AsciiAnimationFrame] {
        &self.frames
    }

    pub fn metadata(&self) -> &AnimationMetadata {
        &self.metadata
    }
}

#[derive(Debug, Clone)]
pub struct AnimationImportOptions {
    pub source_fps: u32,
    pub strip_ansi_escape_codes: bool,
}

impl Default for AnimationImportOptions {
    fn default() -> Self {
        Self {
            source_fps: DEFAULT_SOURCE_FPS,
            strip_ansi_escape_codes: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AsciiCellMetrics {
    pub width: u32,
    pub height: u32,
    pub pixel_aspect_ratio: f32,
}

impl Default for AsciiCellMetrics {
    fn default() -> Self {
        Self {
            width: 8,
            height: 8,
            pixel_aspect_ratio: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationColors {
    pub foreground: [u8; 4],
    pub background: [u8; 4],
}

impl Default for AnimationColors {
    fn default() -> Self {
        Self {
            foreground: [240, 240, 240, 255],
            background: [0, 0, 0, 0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationLoopMode {
    Loop,
    Once,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackOptions {
    pub speed: f32,
    pub start_offset_frames: u32,
    pub loop_mode: AnimationLoopMode,
}

impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            speed: 1.0,
            start_offset_frames: 0,
            loop_mode: AnimationLoopMode::Loop,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationBlendMode {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitOptions {
    pub padding_px: u32,
    pub anchor_x: f32,
    pub anchor_y: f32,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            padding_px: 0,
            anchor_x: 0.5,
            anchor_y: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoutiqueFilter {
    pub seed: u64,
    pub drop_frame_probability: f32,
    pub brightness_jitter: f32,
    pub horizontal_shift_px: i32,
}

impl Default for BoutiqueFilter {
    fn default() -> Self {
        Self {
            seed: 0,
            drop_frame_probability: 0.0,
            brightness_jitter: 0.0,
            horizontal_shift_px: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnimationLayer {
    pub clip_name: String,
    pub playback: PlaybackOptions,
    pub cell: AsciiCellMetrics,
    pub colors: AnimationColors,
    pub font_variant: AsciiFontVariant,
    pub fit: FitOptions,
    pub blend_mode: AnimationBlendMode,
    pub opacity: f32,
    pub filter: BoutiqueFilter,
}

impl AnimationLayer {
    pub fn new(clip_name: impl Into<String>) -> Self {
        Self {
            clip_name: clip_name.into(),
            playback: PlaybackOptions::default(),
            cell: AsciiCellMetrics::default(),
            colors: AnimationColors::default(),
            font_variant: AsciiFontVariant::GeistPixelRegular,
            fit: FitOptions::default(),
            blend_mode: AnimationBlendMode::Foreground,
            opacity: 1.0,
            filter: BoutiqueFilter::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FitPlacement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationCompositeReport {
    pub source_frame_index: usize,
    pub placement: FitPlacement,
    pub dropped: bool,
    pub brightness: f32,
    pub horizontal_shift: i32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AnimationCreditEntry {
    pub clip_name: String,
    pub title: String,
    pub artist: String,
    pub artist_url: Option<String>,
    pub source_url: Option<String>,
    pub license: Option<String>,
    pub tags: Vec<String>,
    pub credit_line: String,
}

#[derive(Debug, Default)]
pub struct AnimationManager {
    clips: BTreeMap<String, AsciiAnimationClip>,
}

impl AnimationManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_from_repo_assets(
        &mut self,
        animation_name: &str,
        options: AnimationImportOptions,
    ) -> Result<()> {
        self.load_from_assets_root(DEFAULT_ANIMATIONS_ROOT, animation_name, options)
    }

    pub fn load_from_assets_root<P: AsRef<Path>>(
        &mut self,
        assets_root: P,
        animation_name: &str,
        options: AnimationImportOptions,
    ) -> Result<()> {
        let directory = assets_root.as_ref().join(animation_name);
        self.load_from_directory(animation_name, directory, options)
    }

    pub fn load_from_directory<P: AsRef<Path>>(
        &mut self,
        animation_name: &str,
        directory: P,
        options: AnimationImportOptions,
    ) -> Result<()> {
        if options.source_fps == 0 {
            bail!("animation '{}' source_fps must be > 0", animation_name);
        }

        let directory = directory.as_ref();
        if !directory.exists() {
            bail!(
                "animation '{}' directory not found: {}",
                animation_name,
                directory.display()
            );
        }
        if !directory.is_dir() {
            bail!(
                "animation '{}' path is not a directory: {}",
                animation_name,
                directory.display()
            );
        }

        let metadata = read_animation_metadata(directory, animation_name)?;
        let frame_paths = collect_frame_paths(directory)
            .with_context(|| format!("animation '{animation_name}': failed to collect frames"))?;
        if frame_paths.is_empty() {
            bail!(
                "animation '{}' has no frame files (.txt or .ans) in {}",
                animation_name,
                directory.display()
            );
        }

        let mut raw_frames = Vec::with_capacity(frame_paths.len());
        let mut max_columns = 0_usize;
        let mut max_rows = 0_usize;
        for frame_path in &frame_paths {
            let frame = read_frame_lines(frame_path, options.strip_ansi_escape_codes)?;
            max_rows = max_rows.max(frame.len());
            max_columns = max_columns.max(frame.iter().map(String::len).max().unwrap_or(0));
            raw_frames.push(frame);
        }

        let columns = u32::try_from(max_columns.max(1))
            .context("animation frame width exceeds supported u32 range")?;
        let rows = u32::try_from(max_rows.max(1))
            .context("animation frame height exceeds supported u32 range")?;

        let frames = raw_frames
            .into_iter()
            .map(|frame| AsciiAnimationFrame {
                lines: normalize_frame_lines(frame, columns as usize, rows as usize),
                columns,
                rows,
            })
            .collect::<Vec<_>>();

        self.clips.insert(
            animation_name.to_owned(),
            AsciiAnimationClip {
                name: animation_name.to_owned(),
                source_fps: options.source_fps,
                frames,
                metadata,
            },
        );

        Ok(())
    }

    pub fn clip(&self, animation_name: &str) -> Option<&AsciiAnimationClip> {
        self.clips.get(animation_name)
    }

    pub fn sample_frame_text(
        &self,
        animation_name: &str,
        output_frame_index: u32,
        output_fps: u32,
        playback: PlaybackOptions,
    ) -> Result<String> {
        let clip = self
            .clips
            .get(animation_name)
            .ok_or_else(|| anyhow!("unknown animation '{}'", animation_name))?;
        if output_fps == 0 {
            bail!("output_fps must be > 0");
        }
        let source_index =
            sample_source_frame_index(clip, output_frame_index, output_fps, playback);
        Ok(clip.frames[source_index].to_text())
    }

    pub fn compose_layer_into_rgba(
        &self,
        target_rgba: &mut [u8],
        target_width: u32,
        target_height: u32,
        output_frame_index: u32,
        output_fps: u32,
        layer: &AnimationLayer,
    ) -> Result<AnimationCompositeReport> {
        validate_target_buffer(target_rgba, target_width, target_height)?;
        if output_fps == 0 {
            bail!("output_fps must be > 0");
        }
        if layer.opacity <= 0.0 {
            return Ok(AnimationCompositeReport {
                source_frame_index: 0,
                placement: FitPlacement {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                },
                dropped: true,
                brightness: 1.0,
                horizontal_shift: 0,
            });
        }

        let clip = self
            .clips
            .get(&layer.clip_name)
            .ok_or_else(|| anyhow!("unknown animation '{}'", layer.clip_name))?;
        if clip.frames.is_empty() {
            bail!("animation '{}' has no frames", layer.clip_name);
        }

        let source_frame_index =
            sample_source_frame_index(clip, output_frame_index, output_fps, layer.playback);
        let source_frame = &clip.frames[source_frame_index];

        let source_width = source_frame
            .columns
            .checked_mul(layer.cell.width)
            .ok_or_else(|| anyhow!("animation source width overflow"))?;
        let source_height = source_frame
            .rows
            .checked_mul(layer.cell.height)
            .ok_or_else(|| anyhow!("animation source height overflow"))?;
        let placement = compute_fit_placement(
            source_width,
            source_height,
            target_width,
            target_height,
            layer.fit,
        );

        let filter_state = layer.filter.sample(output_frame_index);
        if filter_state.drop {
            return Ok(AnimationCompositeReport {
                source_frame_index,
                placement,
                dropped: true,
                brightness: filter_state.brightness,
                horizontal_shift: filter_state.horizontal_shift,
            });
        }

        let atlas = GeistPixelAtlas::new(layer.font_variant);
        let raster = rasterize_ascii_frame(source_frame, layer, &atlas)?;
        composite_scaled(
            target_rgba,
            target_width,
            target_height,
            &raster,
            source_width,
            source_height,
            placement,
            filter_state,
            layer.opacity,
            layer.blend_mode,
        )?;

        Ok(AnimationCompositeReport {
            source_frame_index,
            placement,
            dropped: false,
            brightness: filter_state.brightness,
            horizontal_shift: filter_state.horizontal_shift,
        })
    }

    pub fn credits_manifest(&self) -> Vec<AnimationCreditEntry> {
        self.clips
            .iter()
            .map(|(clip_name, clip)| {
                let title = if clip.metadata.title.trim().is_empty() {
                    clip_name.clone()
                } else {
                    clip.metadata.title.clone()
                };
                let artist = if clip.metadata.artist.trim().is_empty() {
                    "unknown artist".to_owned()
                } else {
                    clip.metadata.artist.clone()
                };
                AnimationCreditEntry {
                    clip_name: clip_name.clone(),
                    title,
                    artist,
                    artist_url: clip.metadata.artist_url.clone(),
                    source_url: clip.metadata.source_url.clone(),
                    license: clip.metadata.license.clone(),
                    tags: clip.metadata.tags.clone(),
                    credit_line: clip.metadata.credit_line(clip_name),
                }
            })
            .collect()
    }

    pub fn discover_library<P: AsRef<Path>>(
        &self,
        root_path: P,
    ) -> Result<Vec<AnimationLibraryEntry>> {
        let root = root_path.as_ref();
        if !root.exists() || !root.is_dir() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        self.scan_directory_for_library(root, root, &mut entries)?;
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(entries)
    }

    fn scan_directory_for_library(
        &self,
        root: &Path,
        current: &Path,
        entries: &mut Vec<AnimationLibraryEntry>,
    ) -> Result<()> {
        if current.join("metadata.json").exists() {
            let id = current
                .strip_prefix(root)
                .unwrap_or(current)
                .to_string_lossy()
                .replace('\\', "/");
            let name = current
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let metadata = read_animation_metadata(current, name)?;
            let frame_count = collect_frame_paths(current)?.len();

            entries.push(AnimationLibraryEntry {
                id,
                title: metadata.title.clone(),
                category: metadata.category.clone(),
                artist: metadata.artist.clone(),
                license: metadata.license.clone(),
                frame_count,
            });
            return Ok(());
        }

        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.scan_directory_for_library(root, &path, entries)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AnimationLibraryEntry {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub artist: String,
    pub license: Option<String>,
    pub frame_count: usize,
}

fn validate_target_buffer(buffer: &[u8], width: u32, height: u32) -> Result<()> {
    let expected = usize::try_from(width)
        .context("target width does not fit usize")?
        .checked_mul(usize::try_from(height).context("target height does not fit usize")?)
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| anyhow!("target frame size overflow"))?;

    if buffer.len() != expected {
        bail!(
            "target frame size mismatch: expected {} bytes, got {}",
            expected,
            buffer.len()
        );
    }
    Ok(())
}

fn read_animation_metadata(directory: &Path, animation_name: &str) -> Result<AnimationMetadata> {
    let metadata_path = directory.join("metadata.json");
    if !metadata_path.exists() {
        return Ok(AnimationMetadata {
            title: animation_name.to_owned(),
            artist: "unknown artist".to_owned(),
            ..AnimationMetadata::default()
        });
    }

    let raw = fs::read(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let mut metadata: AnimationMetadata = serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;

    if metadata.title.trim().is_empty() {
        metadata.title = animation_name.to_owned();
    }
    if metadata.artist.trim().is_empty() {
        metadata.artist = "unknown artist".to_owned();
    }

    Ok(metadata)
}

fn collect_frame_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_supported_frame_file(path))
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| compare_frame_paths(left, right));
    Ok(entries)
}

fn is_supported_frame_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let normalized = ext.to_ascii_lowercase();
            normalized == "txt" || normalized == "ans"
        })
        .unwrap_or(false)
}

fn compare_frame_paths(left: &Path, right: &Path) -> std::cmp::Ordering {
    let left_name = left
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let right_name = right
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let left_number = extract_numeric_order_key(left);
    let right_number = extract_numeric_order_key(right);

    match (left_number, right_number) {
        (Some(a), Some(b)) => a.cmp(&b).then_with(|| left_name.cmp(right_name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left_name.cmp(right_name),
    }
}

fn extract_numeric_order_key(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    if stem.is_empty() {
        return None;
    }
    let prefix_digits = stem
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if prefix_digits.is_empty() {
        return None;
    }
    prefix_digits.parse::<u64>().ok()
}

fn read_frame_lines(path: &Path, strip_ansi: bool) -> Result<Vec<String>> {
    let raw = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut text = String::from_utf8_lossy(&raw).to_string();
    if strip_ansi {
        text = strip_ansi_escape_sequences(&text);
    }

    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized
        .lines()
        .map(sanitize_ascii_line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(String::new());
    }
    Ok(lines)
}

fn sanitize_ascii_line(raw: &str) -> String {
    let expanded = raw.replace('\t', "    ");
    expanded
        .chars()
        .map(|ch| {
            if ch.is_ascii() && (ch == ' ' || ch.is_ascii_graphic()) {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
}

fn normalize_frame_lines(lines: Vec<String>, columns: usize, rows: usize) -> Vec<String> {
    let mut normalized = lines
        .into_iter()
        .take(rows)
        .map(|line| {
            let mut line = line.chars().take(columns).collect::<String>();
            if line.len() < columns {
                line.push_str(&" ".repeat(columns - line.len()));
            }
            line
        })
        .collect::<Vec<_>>();

    if normalized.len() < rows {
        normalized.extend(std::iter::repeat(" ".repeat(columns)).take(rows - normalized.len()));
    }
    normalized
}

fn strip_ansi_escape_sequences(input: &str) -> String {
    enum State {
        Text,
        Escape,
        Csi,
        Osc,
    }

    let mut state = State::Text;
    let mut output = String::with_capacity(input.len());

    for ch in input.chars() {
        match state {
            State::Text => {
                if ch == '\u{1b}' {
                    state = State::Escape;
                } else {
                    output.push(ch);
                }
            }
            State::Escape => match ch {
                '[' => state = State::Csi,
                ']' => state = State::Osc,
                _ => state = State::Text,
            },
            State::Csi => {
                if ('@'..='~').contains(&ch) {
                    state = State::Text;
                }
            }
            State::Osc => {
                if ch == '\u{7}' {
                    state = State::Text;
                }
            }
        }
    }

    output
}

fn sample_source_frame_index(
    clip: &AsciiAnimationClip,
    output_frame_index: u32,
    output_fps: u32,
    playback: PlaybackOptions,
) -> usize {
    if clip.frames.is_empty() || output_fps == 0 {
        return 0;
    }

    let speed = if playback.speed.is_finite() && playback.speed > 0.0 {
        playback.speed as f64
    } else {
        1.0
    };

    let source_progress = (f64::from(output_frame_index) * f64::from(clip.source_fps) * speed)
        / f64::from(output_fps);
    let sampled_index = source_progress.floor() as i64 + i64::from(playback.start_offset_frames);

    match playback.loop_mode {
        AnimationLoopMode::Loop => {
            let count = clip.frames.len() as i64;
            sampled_index.rem_euclid(count) as usize
        }
        AnimationLoopMode::Once => sampled_index.clamp(0, clip.frames.len() as i64 - 1) as usize,
    }
}

fn compute_fit_placement(
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
    fit: FitOptions,
) -> FitPlacement {
    if source_width == 0 || source_height == 0 || target_width == 0 || target_height == 0 {
        return FitPlacement {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    let padding = fit.padding_px.saturating_mul(2);
    let available_width = target_width.saturating_sub(padding).max(1);
    let available_height = target_height.saturating_sub(padding).max(1);

    let scale_x = available_width as f32 / source_width as f32;
    let scale_y = available_height as f32 / source_height as f32;
    let scale = scale_x.min(scale_y);

    let width = ((source_width as f32 * scale).round() as u32)
        .max(1)
        .min(available_width);
    let height = ((source_height as f32 * scale).round() as u32)
        .max(1)
        .min(available_height);

    let anchor_x = fit.anchor_x.clamp(0.0, 1.0);
    let anchor_y = fit.anchor_y.clamp(0.0, 1.0);
    let free_x = available_width.saturating_sub(width);
    let free_y = available_height.saturating_sub(height);
    let x = fit.padding_px as i32 + (free_x as f32 * anchor_x).round() as i32;
    let y = fit.padding_px as i32 + (free_y as f32 * anchor_y).round() as i32;

    FitPlacement {
        x,
        y,
        width,
        height,
    }
}

fn rasterize_ascii_frame(
    frame: &AsciiAnimationFrame,
    layer: &AnimationLayer,
    atlas: &GeistPixelAtlas,
) -> Result<Vec<u8>> {
    if layer.cell.width == 0 || layer.cell.height == 0 {
        bail!("animation layer cell metrics must be > 0");
    }
    if !layer.cell.pixel_aspect_ratio.is_finite() || layer.cell.pixel_aspect_ratio <= 0.0 {
        bail!("animation layer cell pixel_aspect_ratio must be finite and > 0");
    }

    let width = frame
        .columns
        .checked_mul(layer.cell.width)
        .ok_or_else(|| anyhow!("raster width overflow"))?;
    let height = frame
        .rows
        .checked_mul(layer.cell.height)
        .ok_or_else(|| anyhow!("raster height overflow"))?;
    let len = usize::try_from(width)
        .context("raster width exceeds usize")?
        .checked_mul(usize::try_from(height).context("raster height exceeds usize")?)
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| anyhow!("raster buffer overflow"))?;
    let mut pixels = vec![0_u8; len];

    for row in 0..frame.rows {
        for col in 0..frame.columns {
            let line = &frame.lines[row as usize];
            let ch = line.as_bytes().get(col as usize).copied().unwrap_or(b' ');
            let origin_x = col * layer.cell.width;
            let origin_y = row * layer.cell.height;

            paint_background(
                &mut pixels,
                width,
                height,
                origin_x,
                origin_y,
                layer.cell.width,
                layer.cell.height,
                layer.colors.background,
            );
            paint_glyph(
                &mut pixels,
                width,
                height,
                origin_x,
                origin_y,
                ch,
                layer.cell,
                layer.colors.foreground,
                atlas,
            );
        }
    }

    Ok(pixels)
}

fn paint_background(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    origin_x: u32,
    origin_y: u32,
    cell_width: u32,
    cell_height: u32,
    color: [u8; 4],
) {
    if color[3] == 0 {
        return;
    }

    for y in 0..cell_height {
        let py = origin_y + y;
        if py >= height {
            continue;
        }
        for x in 0..cell_width {
            let px = origin_x + x;
            if px >= width {
                continue;
            }
            if let Some(idx) = pixel_offset(width, px, py) {
                blend_src_over(&mut pixels[idx..idx + 4], color);
            }
        }
    }
}

fn paint_glyph(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    origin_x: u32,
    origin_y: u32,
    character: u8,
    cell: AsciiCellMetrics,
    color: [u8; 4],
    atlas: &GeistPixelAtlas,
) {
    if character == b' ' || color[3] == 0 {
        return;
    }

    let glyph_width = atlas.glyph_width();
    let glyph_height = atlas.glyph_height();

    for y in 0..cell.height {
        let py = origin_y + y;
        if py >= height {
            continue;
        }
        let glyph_y = ((y * glyph_height) / cell.height).min(glyph_height - 1);
        for x in 0..cell.width {
            let px = origin_x + x;
            if px >= width {
                continue;
            }

            let normalized_x = (x as f32 + 0.5) / cell.width as f32;
            let centered = normalized_x - 0.5;
            let warped = centered / cell.pixel_aspect_ratio + 0.5;
            if !(0.0..1.0).contains(&warped) {
                continue;
            }
            let glyph_x = ((warped * glyph_width as f32).floor() as u32).min(glyph_width - 1);
            if !atlas.sample(character, glyph_x, glyph_y) {
                continue;
            }

            if let Some(idx) = pixel_offset(width, px, py) {
                blend_src_over(&mut pixels[idx..idx + 4], color);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FilterState {
    drop: bool,
    brightness: f32,
    horizontal_shift: i32,
}

impl BoutiqueFilter {
    fn sample(self, output_frame_index: u32) -> FilterState {
        let drop_probability = self.drop_frame_probability.clamp(0.0, 1.0);
        let brightness_jitter = self.brightness_jitter.clamp(0.0, 1.0);
        let max_shift = i32::try_from(self.horizontal_shift_px.unsigned_abs()).unwrap_or(i32::MAX);

        let drop_seed =
            hash_u64(self.seed ^ ((output_frame_index as u64) << 32) ^ 0x9E37_79B9_7F4A_7C15_u64);
        let drop = unit_from_hash(drop_seed) < drop_probability;

        let brightness_seed =
            hash_u64(self.seed ^ ((output_frame_index as u64) << 16) ^ 0xC2B2_AE3D_27D4_EB4F_u64);
        let brightness =
            (1.0 + ((unit_from_hash(brightness_seed) * 2.0 - 1.0) * brightness_jitter)).max(0.0);

        let horizontal_shift = if max_shift == 0 {
            0
        } else {
            let shift_seed = hash_u64(
                self.seed ^ ((output_frame_index as u64) << 8) ^ 0x1656_67B1_9E37_79F9_u64,
            );
            let span = (max_shift * 2 + 1) as u64;
            (shift_seed % span) as i32 - max_shift
        };

        FilterState {
            drop,
            brightness,
            horizontal_shift,
        }
    }
}

fn unit_from_hash(hash: u64) -> f32 {
    (hash as f64 / u64::MAX as f64) as f32
}

fn hash_u64(mut value: u64) -> u64 {
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^= value >> 33;
    value
}

#[allow(clippy::too_many_arguments)]
fn composite_scaled(
    target_rgba: &mut [u8],
    target_width: u32,
    target_height: u32,
    source_rgba: &[u8],
    source_width: u32,
    source_height: u32,
    placement: FitPlacement,
    filter: FilterState,
    opacity: f32,
    blend_mode: AnimationBlendMode,
) -> Result<()> {
    validate_target_buffer(target_rgba, target_width, target_height)?;
    validate_target_buffer(source_rgba, source_width, source_height)?;
    if placement.width == 0 || placement.height == 0 || opacity <= 0.0 {
        return Ok(());
    }
    let opacity = opacity.clamp(0.0, 1.0);

    for dy in 0..placement.height {
        let ty = placement.y + dy as i32;
        if ty < 0 || ty >= target_height as i32 {
            continue;
        }
        let sy = ((u64::from(dy) * u64::from(source_height)) / u64::from(placement.height)) as u32;

        for dx in 0..placement.width {
            let tx = placement.x + filter.horizontal_shift + dx as i32;
            if tx < 0 || tx >= target_width as i32 {
                continue;
            }
            let sx =
                ((u64::from(dx) * u64::from(source_width)) / u64::from(placement.width)) as u32;
            let src_idx = pixel_offset(source_width, sx, sy)
                .ok_or_else(|| anyhow!("source pixel index overflow"))?;
            let dst_idx = pixel_offset(target_width, tx as u32, ty as u32)
                .ok_or_else(|| anyhow!("target pixel index overflow"))?;

            let mut source = [
                source_rgba[src_idx],
                source_rgba[src_idx + 1],
                source_rgba[src_idx + 2],
                source_rgba[src_idx + 3],
            ];
            if source[3] == 0 {
                continue;
            }

            source[0] = ((source[0] as f32 * filter.brightness)
                .clamp(0.0, 255.0)
                .round()) as u8;
            source[1] = ((source[1] as f32 * filter.brightness)
                .clamp(0.0, 255.0)
                .round()) as u8;
            source[2] = ((source[2] as f32 * filter.brightness)
                .clamp(0.0, 255.0)
                .round()) as u8;
            source[3] = ((source[3] as f32 * opacity).clamp(0.0, 255.0).round()) as u8;
            if source[3] == 0 {
                continue;
            }

            let destination = &mut target_rgba[dst_idx..dst_idx + 4];
            match blend_mode {
                AnimationBlendMode::Foreground => blend_src_over(destination, source),
                AnimationBlendMode::Background => blend_src_under(destination, source),
            }
        }
    }

    Ok(())
}

fn pixel_offset(width: u32, x: u32, y: u32) -> Option<usize> {
    usize::try_from(y)
        .ok()?
        .checked_mul(usize::try_from(width).ok()?)?
        .checked_add(usize::try_from(x).ok()?)?
        .checked_mul(4)
}

fn blend_src_over(dst: &mut [u8], src: [u8; 4]) {
    if dst.len() < 4 {
        return;
    }
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }

    let da = dst[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        dst[0] = 0;
        dst[1] = 0;
        dst[2] = 0;
        dst[3] = 0;
        return;
    }

    let src_r = src[0] as f32 / 255.0;
    let src_g = src[1] as f32 / 255.0;
    let src_b = src[2] as f32 / 255.0;
    let dst_r = dst[0] as f32 / 255.0;
    let dst_g = dst[1] as f32 / 255.0;
    let dst_b = dst[2] as f32 / 255.0;

    let out_r = (src_r * sa + dst_r * da * (1.0 - sa)) / out_a;
    let out_g = (src_g * sa + dst_g * da * (1.0 - sa)) / out_a;
    let out_b = (src_b * sa + dst_b * da * (1.0 - sa)) / out_a;

    dst[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn blend_src_under(dst: &mut [u8], src: [u8; 4]) {
    if dst.len() < 4 {
        return;
    }
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = da + sa * (1.0 - da);
    if out_a <= 0.0 {
        dst[0] = 0;
        dst[1] = 0;
        dst[2] = 0;
        dst[3] = 0;
        return;
    }

    let src_r = src[0] as f32 / 255.0;
    let src_g = src[1] as f32 / 255.0;
    let src_b = src[2] as f32 / 255.0;
    let dst_r = dst[0] as f32 / 255.0;
    let dst_g = dst[1] as f32 / 255.0;
    let dst_b = dst[2] as f32 / 255.0;

    let out_r = (dst_r * da + src_r * sa * (1.0 - da)) / out_a;
    let out_g = (dst_g * da + src_g * sa * (1.0 - da)) / out_a;
    let out_b = (dst_b * da + src_b * sa * (1.0 - da)) / out_a;

    dst[0] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
    dst[3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        compute_fit_placement, AnimationImportOptions, AnimationLayer, AnimationLoopMode,
        AnimationManager, BoutiqueFilter, FitOptions, PlaybackOptions,
    };

    #[test]
    fn load_orders_frames_by_numeric_prefix() {
        let dir = tempfile::tempdir().expect("tempdir");
        let animation_dir = dir.path().join("clip");
        fs::create_dir_all(&animation_dir).expect("create animation dir");
        fs::write(animation_dir.join("010.txt"), "TEN").expect("frame");
        fs::write(animation_dir.join("002.txt"), "TWO").expect("frame");
        fs::write(
            animation_dir.join("metadata.json"),
            r#"{"artist":"tester"}"#,
        )
        .expect("meta");

        let mut manager = AnimationManager::new();
        manager
            .load_from_directory("clip", &animation_dir, AnimationImportOptions::default())
            .expect("load animation");

        let clip = manager.clip("clip").expect("clip");
        assert_eq!(clip.frames().len(), 2);
        assert!(clip.frames()[0].to_text().starts_with("TWO"));
        assert!(clip.frames()[1].to_text().starts_with("TEN"));
    }

    #[test]
    fn playback_sampling_supports_source_fps_vs_output_fps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let animation_dir = dir.path().join("fps");
        fs::create_dir_all(&animation_dir).expect("create animation dir");
        fs::write(animation_dir.join("0001.txt"), "A").expect("frame");
        fs::write(animation_dir.join("0002.txt"), "B").expect("frame");

        let mut manager = AnimationManager::new();
        manager
            .load_from_directory(
                "fps",
                &animation_dir,
                AnimationImportOptions {
                    source_fps: 12,
                    strip_ansi_escape_codes: true,
                },
            )
            .expect("load animation");

        let playback = PlaybackOptions {
            speed: 1.0,
            start_offset_frames: 0,
            loop_mode: AnimationLoopMode::Loop,
        };
        let f0 = manager
            .sample_frame_text("fps", 0, 24, playback)
            .expect("sample");
        let f1 = manager
            .sample_frame_text("fps", 1, 24, playback)
            .expect("sample");
        let f2 = manager
            .sample_frame_text("fps", 2, 24, playback)
            .expect("sample");
        assert!(f0.starts_with('A'));
        assert!(f1.starts_with('A'));
        assert!(f2.starts_with('B'));
    }

    #[test]
    fn fit_preserves_aspect_ratio() {
        let placement = compute_fit_placement(
            200,
            50,
            100,
            100,
            FitOptions {
                padding_px: 0,
                anchor_x: 0.0,
                anchor_y: 0.0,
            },
        );
        assert_eq!(placement.width, 100);
        assert_eq!(placement.height, 25);
    }

    #[test]
    fn compose_overlay_modifies_target_rgba() {
        let dir = tempfile::tempdir().expect("tempdir");
        let animation_dir = dir.path().join("overlay");
        fs::create_dir_all(&animation_dir).expect("create animation dir");
        fs::write(animation_dir.join("0001.txt"), "##\n##").expect("frame");

        let mut manager = AnimationManager::new();
        manager
            .load_from_directory("overlay", &animation_dir, AnimationImportOptions::default())
            .expect("load animation");

        let mut frame = vec![0_u8; 64 * 64 * 4];
        let mut layer = AnimationLayer::new("overlay");
        layer.filter = BoutiqueFilter {
            seed: 1,
            drop_frame_probability: 0.0,
            brightness_jitter: 0.0,
            horizontal_shift_px: 0,
        };
        let report = manager
            .compose_layer_into_rgba(&mut frame, 64, 64, 0, 24, &layer)
            .expect("compose");
        assert!(!report.dropped);
        assert!(frame.iter().any(|byte| *byte != 0));
    }
}
