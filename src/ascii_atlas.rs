use crate::ascii_atlas_data::{
    GlyphRows, ASCII_END, ASCII_START, GEIST_PIXEL_CIRCLE, GEIST_PIXEL_GRID, GEIST_PIXEL_LINE,
    GEIST_PIXEL_SQUARE, GEIST_PIXEL_TRIANGLE, GLYPH_COUNT, GLYPH_HEIGHT, GLYPH_WIDTH,
};
use crate::schema::AsciiFontVariant;

#[derive(Debug, Clone)]
pub struct GeistPixelAtlas {
    glyphs: &'static [GlyphRows; GLYPH_COUNT],
}

impl GeistPixelAtlas {
    pub fn new(variant: AsciiFontVariant) -> Self {
        let glyphs = match variant {
            AsciiFontVariant::GeistPixelLine => &GEIST_PIXEL_LINE,
            AsciiFontVariant::GeistPixelSquare => &GEIST_PIXEL_SQUARE,
            AsciiFontVariant::GeistPixelGrid => &GEIST_PIXEL_GRID,
            AsciiFontVariant::GeistPixelCircle => &GEIST_PIXEL_CIRCLE,
            AsciiFontVariant::GeistPixelTriangle => &GEIST_PIXEL_TRIANGLE,
        };

        Self { glyphs }
    }

    pub fn glyph_width(&self) -> u32 {
        GLYPH_WIDTH
    }

    pub fn glyph_height(&self) -> u32 {
        GLYPH_HEIGHT
    }

    pub fn sample(&self, character: u8, x: u32, y: u32) -> bool {
        if character < ASCII_START || character > ASCII_END {
            return false;
        }
        if x >= GLYPH_WIDTH || y >= GLYPH_HEIGHT {
            return false;
        }

        let glyph_index = (character - ASCII_START) as usize;
        let row_mask = self.glyphs[glyph_index][y as usize];
        ((row_mask >> x) & 1) == 1
    }

    pub fn glyph_density(&self, character: u8) -> f32 {
        let mut count = 0;
        for y in 0..GLYPH_HEIGHT {
            for x in 0..GLYPH_WIDTH {
                if self.sample(character, x, y) {
                    count += 1;
                }
            }
        }
        count as f32 / (GLYPH_WIDTH * GLYPH_HEIGHT) as f32
    }

    pub fn closest_character_by_density(&self, target_density: f32) -> u8 {
        let ramp = self.density_ramp();
        if ramp.is_empty() {
            return b' ';
        }

        let mut best_ch = ramp[0];
        let mut min_diff = f32::MAX;

        for &ch in &ramp {
            let density = self.glyph_density(ch);
            let diff = (density - target_density).abs();
            if diff < min_diff {
                min_diff = diff;
                best_ch = ch;
            } else if diff > min_diff {
                // Since ramp is sorted by density, we can stop early if diff starts increasing
                // (though there might be tie-breakers, so we have to be careful).
                // But for safety and simplicity, we can just iterate.
                break;
            }
        }
        best_ch
    }

    pub fn density_ramp(&self) -> Vec<u8> {
        // Deterministic sorting rule from docs/ascii_perception_and_density.md:
        // 1. on_pixels ascending
        // 2. codepoint ascending

        let mut ramp: Vec<(u8, u32)> = (ASCII_START..=ASCII_END)
            .map(|ch| {
                let mut count = 0;
                for y in 0..GLYPH_HEIGHT {
                    for x in 0..GLYPH_WIDTH {
                        if self.sample(ch, x, y) {
                            count += 1;
                        }
                    }
                }
                (ch, count)
            })
            .collect();

        // Stable deterministic ordering: darkness first, codepoint tie-breaker.
        ramp.sort_by_key(|&(ch, count)| (count, ch));
        ramp.into_iter().map(|(ch, _)| ch).collect()
    }
}

/// Returns a heuristic density (0.0 to 1.0) for standard ASCII characters
/// to facilitate mapping from arbitrary ASCII art into a target font.
pub fn standard_ascii_density(ch: u8) -> f32 {
    match ch {
        b' ' => 0.0,
        b'.' | b',' | b'`' | b'\'' => 0.05,
        b':' | b';' | b'-' | b'_' => 0.1,
        b'~' | b'=' | b'+' | b'"' => 0.15,
        b'!' | b'r' | b'/' | b'\\' | b'|' | b'(' | b')' | b'[' | b']' | b'<' | b'>' => 0.2,
        b'i' | b'v' | b't' | b'z' | b'7' | b'L' => 0.25,
        b'c' | b's' | b'u' | b'n' | b'x' | b'o' | b'e' | b'f' | b'k' => 0.3,
        b'a' | b'w' | b'h' | b'm' | b'y' | b'g' | b'p' | b'q' | b'd' | b'b' => 0.4,
        b'S' | b'O' | b'C' | b'U' | b'N' | b'X' | b'Z' | b'K' | b'T' | b'E' | b'F' => 0.5,
        b'A' | b'V' | b'W' | b'H' | b'M' | b'Y' | b'G' | b'P' | b'D' | b'B' | b'R' => 0.6,
        b'0'..=b'9' => 0.55,
        b'&' | b'$' | b'%' | b'#' | b'@' => 0.7,
        _ => if ch.is_ascii_graphic() { 0.5 } else { 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::GeistPixelAtlas;
    use crate::schema::AsciiFontVariant;

    #[test]
    fn line_and_grid_differ_for_printable_character() {
        let regular = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelLine);
        let bold = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelGrid);

        let mut any_difference = false;
        for y in 0..regular.glyph_height() {
            for x in 0..regular.glyph_width() {
                if regular.sample(b'A', x, y) != bold.sample(b'A', x, y) {
                    any_difference = true;
                    break;
                }
            }
        }

        assert!(any_difference);
    }

    #[test]
    fn line_atlas_has_visible_pixels_for_letter_a() {
        let regular = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelLine);
        let mut seen = false;
        for y in 0..regular.glyph_height() {
            for x in 0..regular.glyph_width() {
                if regular.sample(b'A', x, y) {
                    seen = true;
                    break;
                }
            }
        }
        assert!(seen);
    }
}
