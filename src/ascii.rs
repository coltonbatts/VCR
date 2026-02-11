use anyhow::{anyhow, Result};
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::ascii_atlas::GeistPixelAtlas;
use crate::schema::{AsciiReveal, AsciiRevealDirection, AsciiSource, ColorRgba};

const ASCII_GLYPH_WIDTH: u32 = 8;
const ASCII_GLYPH_HEIGHT: u32 = 8;

#[derive(Debug, Clone)]
pub struct PreparedAsciiLayer {
    source: AsciiSource,
    base_cells: Vec<u8>,
    cell_overrides: Vec<Vec<RuntimeCellOverride>>,
    pixel_width: u32,
    pixel_height: u32,
    atlas: GeistPixelAtlas,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeCellOverride {
    character: Option<u8>,
    foreground: Option<ColorRgba>,
    background: Option<ColorRgba>,
    visible_from_frame: Option<u32>,
    visible_until_frame: Option<u32>,
}

impl PreparedAsciiLayer {
    pub fn new(source: &AsciiSource, layer_id: &str) -> Result<Self> {
        let base_cells = source.compile_base_cells(layer_id)?;
        let (pixel_width, pixel_height) = source.pixel_dimensions()?;

        let total_cells = (source.grid.rows as usize)
            .checked_mul(source.grid.columns as usize)
            .ok_or_else(|| anyhow!("layer '{layer_id}': ascii cell count overflow"))?;
        if base_cells.len() != total_cells {
            return Err(anyhow!(
                "layer '{layer_id}': compiled ascii grid size mismatch (expected {}, got {})",
                total_cells,
                base_cells.len()
            ));
        }

        let mut cell_overrides = vec![Vec::new(); total_cells];
        for cell in &source.cells {
            let index = (cell.row as usize)
                .checked_mul(source.grid.columns as usize)
                .and_then(|offset| offset.checked_add(cell.column as usize))
                .ok_or_else(|| {
                    anyhow!(
                        "layer '{layer_id}': ascii override index overflow at row {}, column {}",
                        cell.row,
                        cell.column
                    )
                })?;

            let parsed_character = match &cell.character {
                Some(character) => {
                    let bytes = character.as_bytes();
                    if bytes.len() != 1 {
                        return Err(anyhow!(
                            "layer '{layer_id}': ascii override character must be one byte"
                        ));
                    }
                    Some(bytes[0])
                }
                None => None,
            };

            cell_overrides[index].push(RuntimeCellOverride {
                character: parsed_character,
                foreground: cell.foreground,
                background: cell.background,
                visible_from_frame: cell.visible_from_frame,
                visible_until_frame: cell.visible_until_frame,
            });
        }

        Ok(Self {
            source: source.clone(),
            base_cells,
            cell_overrides,
            pixel_width,
            pixel_height,
            atlas: GeistPixelAtlas::new(source.font_variant),
        })
    }

    pub fn pixel_width(&self) -> u32 {
        self.pixel_width
    }

    pub fn pixel_height(&self) -> u32 {
        self.pixel_height
    }

    pub fn is_static(&self) -> bool {
        !self.source.is_dynamic()
    }

    pub fn render_frame_pixmap(&self, frame_index: u32) -> Result<Pixmap> {
        let mut pixmap = Pixmap::new(self.pixel_width, self.pixel_height).ok_or_else(|| {
            anyhow!(
                "failed to allocate ascii pixmap {}x{}",
                self.pixel_width,
                self.pixel_height
            )
        })?;

        for row in 0..self.source.grid.rows {
            for column in 0..self.source.grid.columns {
                let cell_index = (row * self.source.grid.columns + column) as usize;
                let mut character = self.base_cells[cell_index];
                let mut foreground = self.source.foreground;
                let mut background = self.source.background;
                let mut visible = self.reveal_visible(row, column, frame_index);

                for override_cell in &self.cell_overrides[cell_index] {
                    if let Some(character_override) = override_cell.character {
                        character = character_override;
                    }
                    if let Some(foreground_override) = override_cell.foreground {
                        foreground = foreground_override;
                    }
                    if let Some(background_override) = override_cell.background {
                        background = background_override;
                    }

                    if let Some(start_frame) = override_cell.visible_from_frame {
                        if frame_index < start_frame {
                            visible = false;
                        }
                    }
                    if let Some(end_frame) = override_cell.visible_until_frame {
                        if frame_index >= end_frame {
                            visible = false;
                        }
                    }
                }

                if !visible {
                    continue;
                }

                let origin_x = column * self.source.cell.width;
                let origin_y = row * self.source.cell.height;
                self.paint_cell_background(&mut pixmap, origin_x, origin_y, background);
                self.paint_cell_glyph(&mut pixmap, origin_x, origin_y, character, foreground);
            }
        }

        Ok(pixmap)
    }

    fn reveal_visible(&self, row: u32, column: u32, frame_index: u32) -> bool {
        let Some(reveal) = &self.source.reveal else {
            return true;
        };

        let rows = self.source.grid.rows as u64;
        let columns = self.source.grid.columns as u64;
        let base_index = match reveal {
            AsciiReveal::RowMajor { .. } => row as u64 * columns + column as u64,
            AsciiReveal::ColumnMajor { .. } => column as u64 * rows + row as u64,
        };
        let total_cells = rows * columns;

        let (start_frame, frames_per_cell, direction) = match reveal {
            AsciiReveal::RowMajor {
                start_frame,
                frames_per_cell,
                direction,
            }
            | AsciiReveal::ColumnMajor {
                start_frame,
                frames_per_cell,
                direction,
            } => (*start_frame as u64, *frames_per_cell as u64, *direction),
        };

        let ordered_index = match direction {
            AsciiRevealDirection::Forward => base_index,
            AsciiRevealDirection::Reverse => {
                total_cells.saturating_sub(1).saturating_sub(base_index)
            }
        };
        let threshold = start_frame.saturating_add(ordered_index.saturating_mul(frames_per_cell));

        (frame_index as u64) >= threshold
    }

    fn paint_cell_background(
        &self,
        pixmap: &mut Pixmap,
        origin_x: u32,
        origin_y: u32,
        color: ColorRgba,
    ) {
        for y in 0..self.source.cell.height {
            for x in 0..self.source.cell.width {
                blend_pixel(pixmap, origin_x + x, origin_y + y, color);
            }
        }
    }

    fn paint_cell_glyph(
        &self,
        pixmap: &mut Pixmap,
        origin_x: u32,
        origin_y: u32,
        character: u8,
        color: ColorRgba,
    ) {
        if character == b' ' {
            return;
        }

        let cell_width = self.source.cell.width;
        let cell_height = self.source.cell.height;
        let aspect = self.source.cell.pixel_aspect_ratio;

        for y in 0..cell_height {
            let glyph_y = ((y * ASCII_GLYPH_HEIGHT) / cell_height).min(ASCII_GLYPH_HEIGHT - 1);
            for x in 0..cell_width {
                let normalized_x = (x as f32 + 0.5) / cell_width as f32;
                let centered = normalized_x - 0.5;
                let warped = centered / aspect + 0.5;
                if !(0.0..1.0).contains(&warped) {
                    continue;
                }

                let glyph_x =
                    ((warped * ASCII_GLYPH_WIDTH as f32).floor() as u32).min(ASCII_GLYPH_WIDTH - 1);
                if self.atlas.sample(character, glyph_x, glyph_y) {
                    blend_pixel(pixmap, origin_x + x, origin_y + y, color);
                }
            }
        }
    }
}

fn blend_pixel(pixmap: &mut Pixmap, x: u32, y: u32, color: ColorRgba) {
    if x >= pixmap.width() || y >= pixmap.height() {
        return;
    }

    let alpha = color.a.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }

    let src_r = color.r.clamp(0.0, 1.0) * alpha * 255.0;
    let src_g = color.g.clamp(0.0, 1.0) * alpha * 255.0;
    let src_b = color.b.clamp(0.0, 1.0) * alpha * 255.0;
    let src_a = alpha * 255.0;
    let src_a_norm = alpha;

    let index = (y * pixmap.width() + x) as usize;
    if let Some(pixel) = pixmap.pixels_mut().get_mut(index) {
        let dst_r = pixel.red() as f32;
        let dst_g = pixel.green() as f32;
        let dst_b = pixel.blue() as f32;
        let dst_a = pixel.alpha() as f32;

        let out_r = (src_r + dst_r * (1.0 - src_a_norm))
            .clamp(0.0, 255.0)
            .round() as u8;
        let out_g = (src_g + dst_g * (1.0 - src_a_norm))
            .clamp(0.0, 255.0)
            .round() as u8;
        let out_b = (src_b + dst_b * (1.0 - src_a_norm))
            .clamp(0.0, 255.0)
            .round() as u8;
        let out_a = (src_a + dst_a * (1.0 - src_a_norm))
            .clamp(0.0, 255.0)
            .round() as u8;

        if let Some(out) = PremultipliedColorU8::from_rgba(out_r, out_g, out_b, out_a) {
            *pixel = out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PreparedAsciiLayer;
    use crate::schema::AsciiSource;

    fn sample_source() -> AsciiSource {
        serde_yaml::from_str(
            r#"
grid: { rows: 1, columns: 2 }
cell: { width: 8, height: 8 }
font_variant: geist_pixel_regular
foreground: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
background: { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
inline:
  - "AB"
"#,
        )
        .expect("ascii source should parse")
    }

    #[test]
    fn prepared_ascii_matches_expected_size() {
        let source = sample_source();
        let prepared = PreparedAsciiLayer::new(&source, "test").expect("ascii should prepare");
        assert_eq!(prepared.pixel_width(), 16);
        assert_eq!(prepared.pixel_height(), 8);
    }

    #[test]
    fn reveal_hides_first_frame() {
        let source: AsciiSource = serde_yaml::from_str(
            r#"
grid: { rows: 1, columns: 1 }
cell: { width: 8, height: 8 }
font_variant: geist_pixel_regular
foreground: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
background: { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
inline:
  - "A"
reveal:
  kind: row_major
  start_frame: 2
  frames_per_cell: 1
"#,
        )
        .expect("ascii source should parse");

        let prepared = PreparedAsciiLayer::new(&source, "test").expect("ascii should prepare");
        let hidden = prepared
            .render_frame_pixmap(0)
            .expect("frame should render");
        let shown = prepared
            .render_frame_pixmap(2)
            .expect("frame should render");

        assert!(hidden.data().iter().all(|byte| *byte == 0));
        assert!(shown.data().iter().any(|byte| *byte != 0));
    }
}
