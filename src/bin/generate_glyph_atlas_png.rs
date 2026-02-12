//! Build-time tool: generates assets/glyph_atlas/geist_pixel_line.png and
//! geist_pixel_line.meta.json for the default ASCII ramp.
//!
//! Run from repo root: cargo run --bin generate_glyph_atlas_png

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use fontdue::Font;
use image::{ImageBuffer, Rgba, RgbaImage};

const DEFAULT_RAMP: &str = " .:-=+*#%@";
const CELL_WIDTH: u32 = 16;
const CELL_HEIGHT: u32 = 16;
const ATLAS_COLUMNS: u32 = 16;
const FONT_SIZE: f32 = 14.0;
const ALPHA_THRESHOLD: u8 = 96;

fn main() -> Result<()> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let font_path = repo_root.join("assets/fonts/geist_pixel/GeistPixel-Line.ttf");
    let out_dir = repo_root.join("assets/glyph_atlas");
    let png_path = out_dir.join("geist_pixel_line.png");
    let meta_path = out_dir.join("geist_pixel_line.meta.json");

    fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;

    let font_bytes =
        fs::read(&font_path).with_context(|| format!("failed to read {}", font_path.display()))?;
    let font = Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|e| anyhow!("failed to parse font: {e}"))?;

    let glyph_count = DEFAULT_RAMP.chars().count() as u32;
    let atlas_rows = (glyph_count + ATLAS_COLUMNS - 1) / ATLAS_COLUMNS;
    let atlas_width = ATLAS_COLUMNS * CELL_WIDTH;
    let atlas_height = atlas_rows * CELL_HEIGHT;

    let mut img: RgbaImage = ImageBuffer::new(atlas_width, atlas_height);
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    for (idx, ch) in DEFAULT_RAMP.chars().enumerate() {
        let (metrics, bitmap) = font.rasterize(ch, FONT_SIZE);
        if metrics.width > CELL_WIDTH as usize || metrics.height > CELL_HEIGHT as usize {
            bail!(
                "glyph '{}' is {}x{} which exceeds {}x{} cell",
                ch,
                metrics.width,
                metrics.height,
                CELL_WIDTH,
                CELL_HEIGHT
            );
        }

        let col = (idx as u32) % ATLAS_COLUMNS;
        let row = (idx as u32) / ATLAS_COLUMNS;
        let offset_x = ((CELL_WIDTH as usize).saturating_sub(metrics.width)) / 2;
        let offset_y = ((CELL_HEIGHT as usize).saturating_sub(metrics.height)) / 2;

        let base_x = col * CELL_WIDTH;
        let base_y = row * CELL_HEIGHT;

        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let alpha = bitmap[y * metrics.width + x];
                if alpha >= ALPHA_THRESHOLD {
                    let px = base_x as i64 + x as i64 + offset_x as i64;
                    let py = base_y as i64 + y as i64 + offset_y as i64;
                    if px >= 0 && px < atlas_width as i64 && py >= 0 && py < atlas_height as i64 {
                        img[(px as u32, py as u32)] = Rgba([255, 255, 255, alpha]);
                    }
                }
            }
        }
    }

    img.save(&png_path)
        .with_context(|| format!("failed to write {}", png_path.display()))?;

    let meta = serde_json::json!({
        "cell_width": CELL_WIDTH,
        "cell_height": CELL_HEIGHT,
        "atlas_columns": ATLAS_COLUMNS,
        "glyph_count": glyph_count
    });
    fs::write(
        &meta_path,
        serde_json::to_string_pretty(&meta).expect("json serialization"),
    )
    .with_context(|| format!("failed to write {}", meta_path.display()))?;

    println!("wrote {} and {}", png_path.display(), meta_path.display());
    Ok(())
}
