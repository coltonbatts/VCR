use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;

use crate::font_assets::{ensure_supported_codepoints, read_verified_font_bytes};

#[derive(Debug, Clone)]
pub struct GlyphBitmap {
    pub width: usize,
    pub height: usize,
    pub bitmap: Vec<u8>,
}

pub struct AsciiPainter {
    font: Font,
    font_size: f32,
    glyph_cache: HashMap<fontdue::layout::GlyphRasterConfig, GlyphBitmap>,
}

impl AsciiPainter {
    pub fn new(manifest_root: &Path, font_file: &str, font_size: f32) -> Result<Self> {
        let font_bytes = read_verified_font_bytes(manifest_root, font_file)?;
        let font = Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|error| anyhow!("failed to parse Geist Pixel font {font_file}: {error}"))?;
        Ok(Self {
            font,
            font_size,
            glyph_cache: HashMap::new(),
        })
    }

    pub fn from_path(font_path: &Path, font_size: f32) -> Result<Self> {
        let font_bytes = std::fs::read(font_path)
            .with_context(|| format!("failed to read font file {}", font_path.display()))?;
        let font = Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|error| anyhow!("failed to parse font {}: {error}", font_path.display()))?;
        Ok(Self {
            font,
            font_size,
            glyph_cache: HashMap::new(),
        })
    }
    
    pub fn set_font_size(&mut self, font_size: f32) {
        if (self.font_size - font_size).abs() > f32::EPSILON {
            self.font_size = font_size;
            self.glyph_cache.clear();
        }
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn cell_width(&self) -> u32 {
        let metrics = self.font.metrics('M', self.font_size);
        metrics.advance_width.ceil().max(1.0) as u32
    }

    pub fn line_height(&self) -> u32 {
        (self.font_size * 1.45).round().max(1.0) as u32
    }

    pub fn draw_line(
        &mut self,
        frame: &mut [u8],
        frame_width: u32,
        frame_height: u32,
        x: u32,
        y: u32,
        text: &str,
        color: [u8; 4],
        max_width: Option<f32>,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        
        // This validation was in some files but not all; it's useful to ensure no crashes.
        ensure_supported_codepoints(&self.font, text, "ascii_painter")?;
        
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: x as f32,
            y: y as f32,
            max_width,
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
                frame_width,
                frame_height,
                glyph.x.round() as i32,
                glyph.y.round() as i32,
                glyph_bitmap,
                color,
            );
        }
        
        Ok(())
    }
}

pub fn blend_glyph(
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

pub fn blend_pixel(frame: &mut [u8], idx: usize, src: [u8; 4]) {
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
