use std::path::Path;

use anyhow::{Context, Result};
use tiny_skia::{Color, Pixmap};

use crate::ascii_atlas::GeistPixelAtlas;
use crate::decoding::FfmpegInput;
use crate::encoding::FfmpegPipe;
use crate::schema::{AsciiFontVariant, Duration as ManifestDuration, Environment, Resolution};

const SAMPLE_GRID_FACTOR: u32 = 4;
const SAMPLE_GRID_AREA: u32 = SAMPLE_GRID_FACTOR * SAMPLE_GRID_FACTOR;
const BT709_R_WEIGHT: u32 = 2126;
const BT709_G_WEIGHT: u32 = 7152;
const BT709_B_WEIGHT: u32 = 722;
const BT709_WEIGHT_SUM: u32 = 10_000;
const LUMA_BOOST_NUM: u32 = 100;
const LUMA_BOOST_DEN: u32 = 100;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;
const FLOYD_STEINBERG_DEN: i32 = 16;
pub const DEFAULT_HYSTERESIS_BAND: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiTemporalMode {
    None,
    Hysteresis { band: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiDitherMode {
    None,
    FloydSteinbergCell,
}

pub struct AsciiRenderArgs<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub width: u32,
    pub height: u32,
    pub font_variant: AsciiFontVariant,
    pub bg_alpha: f32,
    pub sidecar: bool,
    pub expected_hash: Option<u64>,
    pub temporal_mode: AsciiTemporalMode,
    pub dither_mode: AsciiDitherMode,
    pub debug_stage_hashes: bool,
}

pub struct AsciiLabRenderArgs<'a> {
    pub luma_frames: &'a [Vec<u8>],
    pub cols: u32,
    pub rows: u32,
    pub font_variant: AsciiFontVariant,
    pub temporal_mode: AsciiTemporalMode,
    pub dither_mode: AsciiDitherMode,
    pub debug_stage_hashes: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AsciiLabFrameStageHashes {
    pub luma_grid_hash: u64,
    pub mapped_grid_hash: u64,
    pub frame_chars_hash: u64,
}

#[derive(Debug, Clone)]
pub struct AsciiLabFrameResult {
    pub frame_chars: Vec<u8>,
    pub frame_hash: u64,
    pub stage_hashes: Option<AsciiLabFrameStageHashes>,
}

#[derive(Debug, Clone)]
pub struct AsciiLabSequenceResult {
    pub cols: u32,
    pub rows: u32,
    pub frames: Vec<AsciiLabFrameResult>,
    pub sequence_hash: u64,
}

#[derive(serde::Serialize)]
struct AsciiFrameStageHashes {
    pub frame_index: u32,
    pub luma_grid_hash: String,
    pub mapped_grid_hash: String,
    pub frame_chars_hash: String,
}

#[derive(serde::Serialize)]
struct AsciiSequenceSidecar {
    pub cols: u32,
    pub rows: u32,
    pub font: String,
    pub temporal_mode: String,
    pub dither_mode: String,
    pub frame_hashes: Vec<String>,
    pub sequence_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_hashes: Option<Vec<AsciiFrameStageHashes>>,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn fnv1a64_u16(values: &[u16]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for value in values {
        for byte in value.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

fn bt709_luma_u8(r: u8, g: u8, b: u8) -> u8 {
    let weighted = BT709_R_WEIGHT * u32::from(r)
        + BT709_G_WEIGHT * u32::from(g)
        + BT709_B_WEIGHT * u32::from(b);
    ((weighted + (BT709_WEIGHT_SUM / 2)) / BT709_WEIGHT_SUM) as u8
}

fn quantize_luma_to_index(y8: u8, ramp_len: usize) -> usize {
    if ramp_len <= 1 {
        return 0;
    }
    let n = ramp_len as u32;
    ((u32::from(y8) * (n - 1) + 127) / 255) as usize
}

fn quantized_luma_from_index(index: usize, ramp_len: usize) -> u8 {
    if ramp_len <= 1 {
        return 0;
    }
    let denom = (ramp_len - 1) as u32;
    ((index as u32 * 255 + (denom / 2)) / denom) as u8
}

fn div_round_nearest_ties_away_from_zero(numer: i32, denom: i32) -> i32 {
    debug_assert!(denom > 0);
    let abs_numer = i64::from(numer).abs();
    let abs_quot = (abs_numer + (i64::from(denom) / 2)) / i64::from(denom);
    if numer < 0 {
        -(abs_quot as i32)
    } else {
        abs_quot as i32
    }
}

fn apply_hysteresis(
    previous_index: usize,
    nearest_index: usize,
    effective_luma: u8,
    ramp_levels: &[u8],
    band: u8,
) -> usize {
    if ramp_levels.is_empty() {
        return 0;
    }
    let previous_index = previous_index.min(ramp_levels.len() - 1);
    if previous_index == nearest_index {
        return nearest_index;
    }

    let center = ramp_levels[previous_index];
    let low = center.saturating_sub(band);
    let high = center.saturating_add(band);

    if (low..=high).contains(&effective_luma) {
        previous_index
    } else {
        nearest_index
    }
}

fn diffuse_floyd_steinberg(
    error16: &mut [i32],
    width: usize,
    height: usize,
    row: usize,
    col: usize,
    quant_error: i32,
) {
    if col + 1 < width {
        error16[row * width + (col + 1)] += quant_error * 7;
    }
    if row + 1 < height {
        let next_row = row + 1;
        if col > 0 {
            error16[next_row * width + (col - 1)] += quant_error * 3;
        }
        error16[next_row * width + col] += quant_error * 5;
        if col + 1 < width {
            error16[next_row * width + (col + 1)] += quant_error;
        }
    }
}

fn map_luma_grid_to_ascii(
    luma_grid: &[u8],
    cols: u32,
    rows: u32,
    ramp: &[u8],
    dither_mode: AsciiDitherMode,
    temporal_mode: AsciiTemporalMode,
    previous_indices: Option<&[u16]>,
) -> (Vec<u16>, Vec<u8>) {
    let width = cols as usize;
    let height = rows as usize;
    let cell_count = width * height;

    debug_assert_eq!(luma_grid.len(), cell_count);

    let previous_indices = previous_indices.filter(|values| values.len() == cell_count);
    let ramp_levels = (0..ramp.len())
        .map(|index| quantized_luma_from_index(index, ramp.len()))
        .collect::<Vec<_>>();

    let mut mapped_indices = vec![0_u16; cell_count];
    let mut frame_chars = vec![b' '; cell_count];
    let mut error16 = if dither_mode == AsciiDitherMode::FloydSteinbergCell {
        vec![0_i32; cell_count]
    } else {
        Vec::new()
    };

    for row in 0..height {
        for col in 0..width {
            let idx = row * width + col;
            let base_luma = i32::from(luma_grid[idx]);

            let adjusted_luma = match dither_mode {
                AsciiDitherMode::None => base_luma,
                AsciiDitherMode::FloydSteinbergCell => {
                    base_luma
                        + div_round_nearest_ties_away_from_zero(error16[idx], FLOYD_STEINBERG_DEN)
                }
            };
            let effective_luma = adjusted_luma.clamp(0, 255) as u8;
            let nearest_index =
                quantize_luma_to_index(effective_luma, ramp.len()).min(ramp.len() - 1);

            let mapped_index = match temporal_mode {
                AsciiTemporalMode::None => nearest_index,
                AsciiTemporalMode::Hysteresis { band } => {
                    if let Some(previous_indices) = previous_indices {
                        apply_hysteresis(
                            usize::from(previous_indices[idx]),
                            nearest_index,
                            effective_luma,
                            &ramp_levels,
                            band,
                        )
                    } else {
                        nearest_index
                    }
                }
            };

            mapped_indices[idx] = mapped_index as u16;
            frame_chars[idx] = ramp[mapped_index];

            if dither_mode == AsciiDitherMode::FloydSteinbergCell {
                let quantized_luma = i32::from(ramp_levels[mapped_index]);
                let quant_error = i32::from(effective_luma) - quantized_luma;
                diffuse_floyd_steinberg(&mut error16, width, height, row, col, quant_error);
            }
        }
    }

    (mapped_indices, frame_chars)
}

fn temporal_mode_label(mode: AsciiTemporalMode) -> String {
    match mode {
        AsciiTemporalMode::None => "none".to_owned(),
        AsciiTemporalMode::Hysteresis { band } => format!("hysteresis(band={band})"),
    }
}

fn dither_mode_label(mode: AsciiDitherMode) -> &'static str {
    match mode {
        AsciiDitherMode::None => "none",
        AsciiDitherMode::FloydSteinbergCell => "floyd_steinberg_cell",
    }
}

pub fn render_ascii_luma_sequence(args: AsciiLabRenderArgs) -> Result<AsciiLabSequenceResult> {
    let atlas = GeistPixelAtlas::new(args.font_variant);
    let ramp = atlas.density_ramp();
    if ramp.is_empty() {
        anyhow::bail!("density ramp is empty");
    }

    if args.cols == 0 || args.rows == 0 {
        anyhow::bail!("ascii lab dimensions must be greater than zero");
    }

    let cell_count = (args.cols as usize)
        .checked_mul(args.rows as usize)
        .ok_or_else(|| anyhow::anyhow!("ascii lab cell count overflow"))?;

    let mut frames = Vec::with_capacity(args.luma_frames.len());
    let mut frame_hashes = Vec::with_capacity(args.luma_frames.len());
    let mut previous_indices: Option<Vec<u16>> = None;

    for luma_grid in args.luma_frames {
        if luma_grid.len() != cell_count {
            anyhow::bail!(
                "ascii lab frame length mismatch: expected {}, got {}",
                cell_count,
                luma_grid.len()
            );
        }

        let (mapped_indices, frame_chars) = map_luma_grid_to_ascii(
            luma_grid,
            args.cols,
            args.rows,
            &ramp,
            args.dither_mode,
            args.temporal_mode,
            previous_indices.as_deref(),
        );

        if matches!(args.temporal_mode, AsciiTemporalMode::Hysteresis { .. }) {
            previous_indices = Some(mapped_indices.clone());
        } else {
            previous_indices = None;
        }

        let frame_hash = fnv1a64(&frame_chars);
        frame_hashes.push(frame_hash);

        let stage_hashes = if args.debug_stage_hashes {
            Some(AsciiLabFrameStageHashes {
                luma_grid_hash: fnv1a64(luma_grid),
                mapped_grid_hash: fnv1a64_u16(&mapped_indices),
                frame_chars_hash: frame_hash,
            })
        } else {
            None
        };

        frames.push(AsciiLabFrameResult {
            frame_chars,
            frame_hash,
            stage_hashes,
        });
    }

    let mut sequence_bytes = Vec::with_capacity(frame_hashes.len() * 8);
    for hash in &frame_hashes {
        sequence_bytes.extend_from_slice(&hash.to_le_bytes());
    }
    let sequence_hash = fnv1a64(&sequence_bytes);

    Ok(AsciiLabSequenceResult {
        cols: args.cols,
        rows: args.rows,
        frames,
        sequence_hash,
    })
}

pub fn run_ascii_render(args: AsciiRenderArgs) -> Result<()> {
    let atlas = GeistPixelAtlas::new(args.font_variant);
    let ramp = atlas.density_ramp();
    if ramp.is_empty() {
        anyhow::bail!("density ramp is empty");
    }

    let glyph_w = atlas.glyph_width();
    let glyph_h = atlas.glyph_height();

    // Calculate grid size based on target resolution.
    let cols = args.width / glyph_w;
    let rows = args.height / glyph_h;
    if cols == 0 || rows == 0 {
        anyhow::bail!("output size too small for selected glyph dimensions");
    }

    // Oversample for smoother per-cell luma before quantization.
    let sample_width = cols * SAMPLE_GRID_FACTOR;
    let sample_height = rows * SAMPLE_GRID_FACTOR;
    let decoder = FfmpegInput::spawn(args.input, sample_width, sample_height)?;

    // Encoder should use the full target resolution.
    let environment = Environment {
        resolution: Resolution {
            width: args.width,
            height: args.height,
        },
        fps: 30, // Default to 30 or probe from video? FfmpegInput doesn't probe yet.
        duration: ManifestDuration::Seconds(0.0), // Placeholder
        color_space: Default::default(),
    };
    let encoder = FfmpegPipe::spawn(&environment, args.output)?;

    println!(
        "[VCR] Rendering video ASCII: {} -> {}",
        args.input.display(),
        args.output.display()
    );
    println!("[VCR] Grid size: {}x{}", cols, rows);

    let mut frame_hashes = Vec::new();
    let mut stage_hashes = if args.debug_stage_hashes {
        Some(Vec::new())
    } else {
        None
    };
    let mut previous_indices: Option<Vec<u16>> = None;

    while let Some(frame) = decoder.read_frame() {
        let mut pixmap = Pixmap::new(args.width, args.height).context("failed to create pixmap")?;

        // Fill background.
        if args.bg_alpha > 0.0 {
            pixmap.fill(Color::from_rgba8(0, 0, 0, (args.bg_alpha * 255.0) as u8));
        }

        let cell_count = (cols * rows) as usize;
        let mut cell_luma = vec![0_u8; cell_count];
        let mut cell_alpha = vec![0_u8; cell_count];
        let mut cell_r = vec![0_u8; cell_count];
        let mut cell_g = vec![0_u8; cell_count];
        let mut cell_b = vec![0_u8; cell_count];

        for r in 0..rows {
            for c in 0..cols {
                // Average SAMPLE_GRID_FACTOR x SAMPLE_GRID_FACTOR samples for this cell.
                let mut total_luma: u32 = 0;
                let mut total_alpha: u32 = 0;
                let mut total_r: u32 = 0;
                let mut total_g: u32 = 0;
                let mut total_b: u32 = 0;

                for sy in 0..SAMPLE_GRID_FACTOR {
                    for sx in 0..SAMPLE_GRID_FACTOR {
                        let sc = c * SAMPLE_GRID_FACTOR + sx;
                        let sr = r * SAMPLE_GRID_FACTOR + sy;
                        let s_idx = ((sr * sample_width + sc) * 4) as usize;
                        let rgba = &frame[s_idx..s_idx + 4];

                        total_luma += u32::from(bt709_luma_u8(rgba[0], rgba[1], rgba[2]));
                        total_alpha += u32::from(rgba[3]);
                        total_r += u32::from(rgba[0]);
                        total_g += u32::from(rgba[1]);
                        total_b += u32::from(rgba[2]);
                    }
                }

                let idx = (r * cols + c) as usize;
                let avg_luma = (total_luma + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA;
                let avg_alpha = (total_alpha + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA;

                // Alpha-gated effective luma; optional boost remains deterministic.
                let mut y8 = ((avg_luma * avg_alpha + 127) / 255).min(255);
                y8 = ((y8 * LUMA_BOOST_NUM + (LUMA_BOOST_DEN / 2)) / LUMA_BOOST_DEN).min(255);

                cell_luma[idx] = y8 as u8;
                cell_alpha[idx] = avg_alpha as u8;
                cell_r[idx] = ((total_r + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA) as u8;
                cell_g[idx] = ((total_g + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA) as u8;
                cell_b[idx] = ((total_b + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA) as u8;
            }
        }

        let (mapped_indices, frame_chars) = map_luma_grid_to_ascii(
            &cell_luma,
            cols,
            rows,
            &ramp,
            args.dither_mode,
            args.temporal_mode,
            previous_indices.as_deref(),
        );

        if matches!(args.temporal_mode, AsciiTemporalMode::Hysteresis { .. }) {
            previous_indices = Some(mapped_indices.clone());
        } else {
            previous_indices = None;
        }

        for idx in 0..frame_chars.len() {
            let ch = frame_chars[idx];
            if ch == b' ' {
                continue;
            }
            let row = idx / cols as usize;
            let col = idx % cols as usize;
            let x = col as u32 * glyph_w;
            let y = row as u32 * glyph_h;

            for gy in 0..glyph_h {
                for gx in 0..glyph_w {
                    if atlas.sample(ch, gx, gy) {
                        let pixel_x = x + gx;
                        let pixel_y = y + gy;
                        if pixel_x < args.width && pixel_y < args.height {
                            let p_idx = (pixel_y * args.width + pixel_x) as usize;
                            pixmap.pixels_mut()[p_idx] =
                                tiny_skia::PremultipliedColorU8::from_rgba(
                                    cell_r[idx],
                                    cell_g[idx],
                                    cell_b[idx],
                                    cell_alpha[idx],
                                )
                                .unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
                        }
                    }
                }
            }
        }

        encoder.write_frame(pixmap.data().to_vec())?;

        // Calculate frame hash.
        let frame_hash = fnv1a64(&frame_chars);
        frame_hashes.push(frame_hash);

        if let Some(stage_hashes) = &mut stage_hashes {
            let luma_hash = fnv1a64(&cell_luma);
            let mapped_hash = fnv1a64_u16(&mapped_indices);
            stage_hashes.push(AsciiFrameStageHashes {
                frame_index: stage_hashes.len() as u32,
                luma_grid_hash: format!("0x{luma_hash:016x}"),
                mapped_grid_hash: format!("0x{mapped_hash:016x}"),
                frame_chars_hash: format!("0x{frame_hash:016x}"),
            });
        }
    }

    decoder.finish()?;
    encoder.finish()?;

    // Sequence hash from frame hashes.
    let mut seq_bytes = Vec::with_capacity(frame_hashes.len() * 8);
    for hash in &frame_hashes {
        seq_bytes.extend_from_slice(&hash.to_le_bytes());
    }
    let sequence_hash = fnv1a64(&seq_bytes);

    if let Some(expected) = args.expected_hash {
        if expected != sequence_hash {
            anyhow::bail!(
                "Sequence hash mismatch! Expected 0x{expected:016x}, got 0x{sequence_hash:016x}"
            );
        }
        println!("[VCR] Regression check passed: 0x{sequence_hash:016x}");
    } else {
        println!("[VCR] Sequence hash: 0x{sequence_hash:016x}");
    }

    if args.debug_stage_hashes {
        if let Some(stage_hashes) = &stage_hashes {
            for hash in stage_hashes {
                println!(
                    "[VCR] frame {} stage hashes: luma={}, mapped={}, chars={}",
                    hash.frame_index,
                    hash.luma_grid_hash,
                    hash.mapped_grid_hash,
                    hash.frame_chars_hash
                );
            }
        }
    }

    if args.sidecar {
        let sidecar = AsciiSequenceSidecar {
            cols,
            rows,
            font: format!("{:?}", args.font_variant),
            temporal_mode: temporal_mode_label(args.temporal_mode),
            dither_mode: dither_mode_label(args.dither_mode).to_owned(),
            frame_hashes: frame_hashes
                .iter()
                .map(|hash| format!("0x{hash:016x}"))
                .collect(),
            sequence_hash: format!("0x{sequence_hash:016x}"),
            stage_hashes,
        };
        let sidecar_path = args.output.with_extension("json");
        let json = serde_json::to_string_pretty(&sidecar)?;
        std::fs::write(&sidecar_path, json)?;
        println!("[VCR] Wrote sidecar to {}", sidecar_path.display());
    }

    println!("[VCR] Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        div_round_nearest_ties_away_from_zero, fnv1a64, fnv1a64_u16, map_luma_grid_to_ascii,
        AsciiDitherMode, AsciiTemporalMode,
    };

    #[test]
    fn division_rounding_table_matches_spec() {
        let cases = [
            (0, 16, 0),
            (7, 16, 0),
            (8, 16, 1),
            (9, 16, 1),
            (15, 16, 1),
            (23, 16, 1),
            (24, 16, 2),
            (-7, 16, 0),
            (-8, 16, -1),
            (-9, 16, -1),
            (-15, 16, -1),
            (-23, 16, -1),
            (-24, 16, -2),
            (2, 3, 1),
            (-2, 3, -1),
            (5, 3, 2),
            (-5, 3, -2),
        ];
        for (numer, denom, expected) in cases {
            assert_eq!(
                div_round_nearest_ties_away_from_zero(numer, denom),
                expected,
                "numer={numer}, denom={denom}"
            );
        }
    }

    #[test]
    fn mapping_is_deterministic_for_same_inputs() {
        let cols = 6;
        let rows = 4;
        let ramp = b" .:-=+*#%@";
        let luma = vec![
            12, 27, 43, 61, 79, 96, 112, 129, 144, 151, 160, 171, 182, 193, 205, 214, 223, 231,
            240, 228, 214, 198, 180, 162,
        ];

        let (prev_indices, _) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::None,
            AsciiTemporalMode::None,
            None,
        );

        let (_, first_chars) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::FloydSteinbergCell,
            AsciiTemporalMode::Hysteresis { band: 8 },
            Some(&prev_indices),
        );
        let (_, second_chars) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::FloydSteinbergCell,
            AsciiTemporalMode::Hysteresis { band: 8 },
            Some(&prev_indices),
        );

        assert_eq!(first_chars, second_chars);
        assert_eq!(fnv1a64(&first_chars), fnv1a64(&second_chars));
    }

    #[test]
    fn dither_mode_change_changes_hash() {
        let cols = 4;
        let rows = 2;
        let ramp = b" .#";
        let luma = vec![190; (cols * rows) as usize];

        let (_, no_dither_chars) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::None,
            AsciiTemporalMode::None,
            None,
        );
        let (_, fs_chars) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::FloydSteinbergCell,
            AsciiTemporalMode::None,
            None,
        );

        assert_ne!(fnv1a64(&no_dither_chars), fnv1a64(&fs_chars));
    }

    #[test]
    fn hysteresis_band_change_changes_hash() {
        let cols = 5;
        let rows = 2;
        let ramp = b" .:-=+*#%@";
        let previous_luma = vec![113; (cols * rows) as usize];
        let current_luma = vec![129; (cols * rows) as usize];

        let (prev_indices, _) = map_luma_grid_to_ascii(
            &previous_luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::None,
            AsciiTemporalMode::None,
            None,
        );

        let (_, narrow_band_chars) = map_luma_grid_to_ascii(
            &current_luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::None,
            AsciiTemporalMode::Hysteresis { band: 0 },
            Some(&prev_indices),
        );
        let (_, wide_band_chars) = map_luma_grid_to_ascii(
            &current_luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::None,
            AsciiTemporalMode::Hysteresis { band: 24 },
            Some(&prev_indices),
        );

        assert_ne!(fnv1a64(&narrow_band_chars), fnv1a64(&wide_band_chars));
    }

    #[test]
    fn stage_hashes_are_deterministic_for_same_inputs() {
        let cols = 6;
        let rows = 3;
        let ramp = b" .:-=+*#%@";
        let luma = vec![
            8, 24, 37, 53, 71, 88, 105, 122, 139, 157, 174, 191, 208, 224, 239, 220, 199, 178,
        ];

        let (mapped_a, chars_a) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::FloydSteinbergCell,
            AsciiTemporalMode::None,
            None,
        );
        let (mapped_b, chars_b) = map_luma_grid_to_ascii(
            &luma,
            cols,
            rows,
            ramp,
            AsciiDitherMode::FloydSteinbergCell,
            AsciiTemporalMode::None,
            None,
        );

        let luma_hash_a = fnv1a64(&luma);
        let luma_hash_b = fnv1a64(&luma.clone());
        assert_eq!(luma_hash_a, luma_hash_b);
        assert_eq!(fnv1a64_u16(&mapped_a), fnv1a64_u16(&mapped_b));
        assert_eq!(fnv1a64(&chars_a), fnv1a64(&chars_b));
    }
}
