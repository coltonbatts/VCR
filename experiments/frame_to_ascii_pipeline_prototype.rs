use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "frame_to_ascii_pipeline_prototype",
    about = "Deterministic frame-sequence to ASCII pipeline with per-frame and sequence hashes"
)]
struct Cli {
    #[arg(long)]
    input_image: Option<PathBuf>,

    #[arg(long)]
    input_dir: Option<PathBuf>,

    #[arg(long, default_value_t = 16)]
    synthetic_frames: u32,

    #[arg(long, default_value_t = 120)]
    cols: u32,

    #[arg(long, default_value_t = 67)]
    rows: u32,

    #[arg(long, default_value = " .:-=+*#%@")]
    ramp: String,

    #[arg(long)]
    out_dir: Option<PathBuf>,

    #[arg(long)]
    expected_sequence_hash: Option<String>,
}

#[derive(Debug, Clone)]
struct InputFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct SequenceSidecar {
    cols: u32,
    rows: u32,
    ramp: String,
    frame_hashes_fnv1a64_hex: Vec<String>,
    sequence_hash_fnv1a64_hex: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_dimensions(cli.cols, cli.rows)?;

    let ramp = validate_ramp(&cli.ramp)?;
    let frames = load_frames(&cli)?;
    if frames.is_empty() {
        bail!("no frames available for pipeline");
    }

    let mut ascii_frames = Vec::with_capacity(frames.len());
    let mut frame_hashes = Vec::with_capacity(frames.len());

    for frame in &frames {
        let ascii = frame_to_ascii(frame, cli.cols, cli.rows, ramp);
        let hash = fnv1a64(&ascii);
        ascii_frames.push(ascii);
        frame_hashes.push(hash);
    }

    let sequence_hash = sequence_hash_fnv1a64(&frame_hashes);

    if let Some(expected_raw) = &cli.expected_sequence_hash {
        let expected = parse_u64_hex_or_decimal(expected_raw)?;
        if expected != sequence_hash {
            bail!(
                "sequence hash mismatch: expected 0x{expected:016x}, actual 0x{sequence_hash:016x}"
            );
        }
        println!("regression_check=pass");
    }

    if let Some(out_dir) = &cli.out_dir {
        fs::create_dir_all(out_dir)
            .with_context(|| format!("failed creating output dir {}", out_dir.display()))?;

        for (index, ascii) in ascii_frames.iter().enumerate() {
            let frame_path = out_dir.join(format!("frame_{index:05}.txt"));
            fs::write(&frame_path, ascii)
                .with_context(|| format!("failed writing {}", frame_path.display()))?;
        }

        let sidecar = SequenceSidecar {
            cols: cli.cols,
            rows: cli.rows,
            ramp: cli.ramp.clone(),
            frame_hashes_fnv1a64_hex: frame_hashes
                .iter()
                .map(|hash| format!("0x{hash:016x}"))
                .collect(),
            sequence_hash_fnv1a64_hex: format!("0x{sequence_hash:016x}"),
        };

        let sidecar_path = out_dir.join("ascii_sequence_hashes.json");
        let payload = serde_json::to_vec_pretty(&sidecar)?;
        fs::write(&sidecar_path, payload)
            .with_context(|| format!("failed writing {}", sidecar_path.display()))?;
    }

    println!("frames={}", frames.len());
    println!("cols={}", cli.cols);
    println!("rows={}", cli.rows);
    for (index, hash) in frame_hashes.iter().enumerate() {
        println!("frame[{index}]_hash_fnv1a64=0x{hash:016x}");
    }
    println!("sequence_hash_fnv1a64=0x{sequence_hash:016x}");

    Ok(())
}

fn validate_dimensions(cols: u32, rows: u32) -> Result<()> {
    if cols == 0 || rows == 0 {
        bail!("cols and rows must be > 0");
    }
    Ok(())
}

fn validate_ramp(raw: &str) -> Result<&[u8]> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        bail!("ramp must not be empty");
    }
    if !bytes.iter().all(u8::is_ascii) {
        bail!("ramp must be ASCII-only");
    }
    Ok(bytes)
}

fn load_frames(cli: &Cli) -> Result<Vec<InputFrame>> {
    match (&cli.input_image, &cli.input_dir) {
        (Some(_), Some(_)) => bail!("use only one of --input-image or --input-dir"),
        (Some(path), None) => Ok(vec![load_image(path)?]),
        (None, Some(path)) => load_images_from_dir(path),
        (None, None) => synthesize_frames(cli.synthetic_frames),
    }
}

fn load_images_from_dir(path: &Path) -> Result<Vec<InputFrame>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path).with_context(|| format!("failed reading {}", path.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }
        let p = entry.path();
        let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower = ext.to_ascii_lowercase();
        if matches!(lower.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            files.push(p);
        }
    }

    if files.is_empty() {
        bail!("no supported image files found in {}", path.display());
    }

    files.sort();
    files
        .iter()
        .map(|path| load_image(path))
        .collect::<Result<Vec<_>>>()
}

fn load_image(path: &Path) -> Result<InputFrame> {
    let img = image::open(path).with_context(|| format!("failed decoding {}", path.display()))?;
    let rgba = img.to_rgba8();
    Ok(InputFrame {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn synthesize_frames(count: u32) -> Result<Vec<InputFrame>> {
    if count == 0 {
        bail!("--synthetic-frames must be > 0 when no input is provided");
    }

    let width = 320_u32;
    let height = 180_u32;
    let mut frames = Vec::with_capacity(count as usize);

    for frame_index in 0..count {
        let mut rgba = vec![0_u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;

                let base = ((x.wrapping_mul(11)
                    + y.wrapping_mul(7)
                    + frame_index.wrapping_mul(13))
                    & 0xFF) as u8;
                let r = base;
                let g = base.wrapping_add(((frame_index * 3) & 0xFF) as u8);
                let b = base.wrapping_add((((x ^ y) + frame_index) & 0xFF) as u8);

                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            }
        }

        frames.push(InputFrame {
            width,
            height,
            rgba,
        });
    }

    Ok(frames)
}

fn frame_to_ascii(frame: &InputFrame, cols: u32, rows: u32, ramp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((cols as usize + 1) * rows as usize);

    for row in 0..rows {
        let y0 = row * frame.height / rows;
        let mut y1 = (row + 1) * frame.height / rows;
        if y1 <= y0 {
            y1 = (y0 + 1).min(frame.height);
        }

        for col in 0..cols {
            let x0 = col * frame.width / cols;
            let mut x1 = (col + 1) * frame.width / cols;
            if x1 <= x0 {
                x1 = (x0 + 1).min(frame.width);
            }

            let mut sum_luma: u64 = 0;
            let mut sample_count: u64 = 0;

            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = ((y * frame.width + x) * 4) as usize;
                    let r = u32::from(frame.rgba[idx]);
                    let g = u32::from(frame.rgba[idx + 1]);
                    let b = u32::from(frame.rgba[idx + 2]);
                    let y8 = (2126 * r + 7152 * g + 722 * b + 5000) / 10000;
                    sum_luma += u64::from(y8);
                    sample_count += 1;
                }
            }

            let avg = ((sum_luma + sample_count / 2) / sample_count) as u8;
            let idx = quantize_luma(avg, ramp.len());
            out.push(ramp[idx]);
        }

        if row + 1 < rows {
            out.push(b'\n');
        }
    }

    out
}

fn quantize_luma(y8: u8, ramp_len: usize) -> usize {
    if ramp_len <= 1 {
        return 0;
    }

    let max_idx = ramp_len as u32 - 1;
    ((u32::from(y8) * max_idx + 127) / 255) as usize
}

fn sequence_hash_fnv1a64(frame_hashes: &[u64]) -> u64 {
    let mut bytes = Vec::with_capacity(frame_hashes.len() * 8);
    for hash in frame_hashes {
        bytes.extend_from_slice(&hash.to_le_bytes());
    }
    fnv1a64(&bytes)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

fn parse_u64_hex_or_decimal(raw: &str) -> Result<u64> {
    if let Some(hex) = raw.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .with_context(|| format!("invalid expected hash hex value: {raw}"));
    }

    raw.parse::<u64>()
        .with_context(|| format!("invalid expected hash decimal value: {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizer_bounds_should_hold() {
        for y in 0_u16..=255_u16 {
            let idx = quantize_luma(y as u8, 10);
            assert!(idx <= 9);
        }
    }

    #[test]
    fn sequence_hash_should_be_repeatable() {
        let hashes = [1_u64, 2_u64, 3_u64, 4_u64];
        let first = sequence_hash_fnv1a64(&hashes);
        let second = sequence_hash_fnv1a64(&hashes);
        assert_eq!(first, second);
    }

    #[test]
    fn synthetic_pipeline_should_be_repeatable() {
        let frames = synthesize_frames(3).expect("synthetic frames should generate");
        let ramp = b" .:-=+*#%@";

        let first: Vec<u64> = frames
            .iter()
            .map(|frame| fnv1a64(&frame_to_ascii(frame, 40, 20, ramp)))
            .collect();
        let second: Vec<u64> = frames
            .iter()
            .map(|frame| fnv1a64(&frame_to_ascii(frame, 40, 20, ramp)))
            .collect();

        assert_eq!(first, second);
    }
}
