use std::path::Path;
use anyhow::{Result, Context};
use tiny_skia::{Pixmap, Color};
use crate::decoding::FfmpegInput;
use crate::encoding::FfmpegPipe;
use crate::ascii_atlas::GeistPixelAtlas;
use crate::schema::{AsciiFontVariant, Environment, Resolution, Duration as ManifestDuration};

const SAMPLE_GRID_FACTOR: u32 = 4;
const SAMPLE_GRID_AREA: u32 = SAMPLE_GRID_FACTOR * SAMPLE_GRID_FACTOR;
const BT709_R_WEIGHT: u32 = 2126;
const BT709_G_WEIGHT: u32 = 7152;
const BT709_B_WEIGHT: u32 = 722;
const BT709_WEIGHT_SUM: u32 = 10_000;
const LUMA_BOOST_NUM: u32 = 100;
const LUMA_BOOST_DEN: u32 = 100;

pub struct AsciiRenderArgs<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub width: u32,
    pub height: u32,
    pub font_variant: AsciiFontVariant,
    pub bg_alpha: f32,
    pub sidecar: bool,
    pub expected_hash: Option<u64>,
}

#[derive(serde::Serialize)]
struct AsciiSequenceSidecar {
    pub cols: u32,
    pub rows: u32,
    pub font: String,
    pub frame_hashes: Vec<String>,
    pub sequence_hash: String,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

fn bt709_luma_u8(r: u8, g: u8, b: u8) -> u8 {
    let weighted = BT709_R_WEIGHT * u32::from(r)
        + BT709_G_WEIGHT * u32::from(g)
        + BT709_B_WEIGHT * u32::from(b);
    ((weighted + (BT709_WEIGHT_SUM / 2)) / BT709_WEIGHT_SUM) as u8
}

pub fn run_ascii_render(args: AsciiRenderArgs) -> Result<()> {
    let atlas = GeistPixelAtlas::new(args.font_variant);
    let ramp = atlas.density_ramp();
    
    let glyph_w = atlas.glyph_width();
    let glyph_h = atlas.glyph_height();
    
    // Calculate grid size based on target resolution
    let cols = args.width / glyph_w;
    let rows = args.height / glyph_h;
    
    // Oversample for smoother per-cell luma before quantization.
    let sample_width = cols * SAMPLE_GRID_FACTOR;
    let sample_height = rows * SAMPLE_GRID_FACTOR;
    let decoder = FfmpegInput::spawn(args.input, sample_width, sample_height)?;
    
    // Encoder should use the full target resolution
    let environment = Environment {
        resolution: Resolution { width: args.width, height: args.height },
        fps: 30, // Default to 30 or probe from video? FfmpegInput doesn't probe yet.
        duration: ManifestDuration::Seconds(0.0), // Placeholder
        color_space: Default::default(),
    };
    let encoder = FfmpegPipe::spawn(&environment, args.output)?;

    println!("[VCR] Rendering video ASCII: {} -> {}", args.input.display(), args.output.display());
    println!("[VCR] Grid size: {}x{}", cols, rows);

    let mut frame_hashes = Vec::new();


    while let Some(frame) = decoder.read_frame() {
        let mut pixmap = Pixmap::new(args.width, args.height).context("failed to create pixmap")?;
        let mut frame_chars = Vec::with_capacity((cols * rows) as usize);
        
        // Fill background
        if args.bg_alpha > 0.0 {
            pixmap.fill(Color::from_rgba8(0, 0, 0, (args.bg_alpha * 255.0) as u8));
        }

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
                
                let avg_luma = (total_luma + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA;
                let avg_alpha = (total_alpha + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA;
                let final_r = ((total_r + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA) as u8;
                let final_g = ((total_g + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA) as u8;
                let final_b = ((total_b + (SAMPLE_GRID_AREA / 2)) / SAMPLE_GRID_AREA) as u8;
                
                // Alpha-gated effective luma; optional boost remains deterministic.
                let mut y8 = ((avg_luma * avg_alpha + 127) / 255).min(255);
                y8 = ((y8 * LUMA_BOOST_NUM + (LUMA_BOOST_DEN / 2)) / LUMA_BOOST_DEN).min(255);
                
                // Spec-compliant luma quantization: index = floor((Y8 * (N - 1) + 127) / 255)
                let n = ramp.len() as u32;
                let char_idx = ((y8 * (n - 1) + 127) / 255) as usize;
                let ch = ramp[char_idx.min(ramp.len() - 1)];
                
                // Track characters for hashing this frame
                frame_chars.push(ch);
                
                if ch != b' ' {
                    let x = c * glyph_w;
                    let y = r * glyph_h;
                    
                    // Paint character
                    for gy in 0..glyph_h {
                        for gx in 0..glyph_w {
                            if atlas.sample(ch, gx, gy) {
                                let pixel_x = x + gx;
                                let pixel_y = y + gy;
                                if pixel_x < args.width && pixel_y < args.height {
                                    let p_idx = (pixel_y * args.width + pixel_x) as usize;
                                    pixmap.pixels_mut()[p_idx] = tiny_skia::PremultipliedColorU8::from_rgba(
                                        final_r, final_g, final_b, avg_alpha as u8
                                    ).unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        encoder.write_frame(pixmap.data().to_vec())?;

        // Calculate frame hash
        let hash = fnv1a64(&frame_chars);
        frame_hashes.push(hash);
    }

    decoder.finish()?;
    encoder.finish()?;

    // Sequence hash from frame hashes
    let mut seq_bytes = Vec::with_capacity(frame_hashes.len() * 8);
    for h in &frame_hashes {
        seq_bytes.extend_from_slice(&h.to_le_bytes());
    }
    let sequence_hash = fnv1a64(&seq_bytes);

    if let Some(expected) = args.expected_hash {
        if expected != sequence_hash {
            anyhow::bail!("Sequence hash mismatch! Expected 0x{:016x}, got 0x{:016x}", expected, sequence_hash);
        }
        println!("[VCR] Regression check passed: 0x{:016x}", sequence_hash);
    } else {
        println!("[VCR] Sequence hash: 0x{:016x}", sequence_hash);
    }

    if args.sidecar {
        let sidecar = AsciiSequenceSidecar {
            cols,
            rows,
            font: format!("{:?}", args.font_variant),
            frame_hashes: frame_hashes.iter().map(|h| format!("0x{:016x}", h)).collect(),
            sequence_hash: format!("0x{:016x}", sequence_hash),
        };
        let sidecar_path = args.output.with_extension("json");
        let json = serde_json::to_string_pretty(&sidecar)?;
        std::fs::write(&sidecar_path, json)?;
        println!("[VCR] Wrote sidecar to {}", sidecar_path.display());
    }
    
    println!("[VCR] Done.");
    Ok(())
}
