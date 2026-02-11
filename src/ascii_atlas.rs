use crate::ascii_atlas_data::{
    GlyphRows, ASCII_END, ASCII_START, GEIST_PIXEL_BOLD, GEIST_PIXEL_LIGHT, GEIST_PIXEL_MEDIUM,
    GEIST_PIXEL_MONO, GEIST_PIXEL_REGULAR, GLYPH_COUNT, GLYPH_HEIGHT, GLYPH_WIDTH,
};
use crate::schema::AsciiFontVariant;

#[derive(Debug, Clone)]
pub struct GeistPixelAtlas {
    glyphs: &'static [GlyphRows; GLYPH_COUNT],
}

impl GeistPixelAtlas {
    pub fn new(variant: AsciiFontVariant) -> Self {
        let glyphs = match variant {
            AsciiFontVariant::GeistPixelRegular => &GEIST_PIXEL_REGULAR,
            AsciiFontVariant::GeistPixelMedium => &GEIST_PIXEL_MEDIUM,
            AsciiFontVariant::GeistPixelBold => &GEIST_PIXEL_BOLD,
            AsciiFontVariant::GeistPixelLight => &GEIST_PIXEL_LIGHT,
            AsciiFontVariant::GeistPixelMono => &GEIST_PIXEL_MONO,
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

    pub fn density_ramp(&self) -> Vec<u8> {
        // Deterministic sorting rule from docs/ascii_perception_and_density.md:
        // 1. on_pixels ascending
        // 2. codepoint ascending
        
        // We use the full printable ASCII range to allow for maximum tonal depth,
        // or a curated set if we want more "dope" results. 
        // Let's use the printable ASCII range but filter for common tonal chars for a clean vibe,
        // OR just follow the spec literally and use all printable ASCII.
        // Actually, the experiment used " .:-=+*#%@". 
        // I'll use the printable ASCII range but ensure it's sorted as per spec.
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

#[cfg(test)]
mod tests {
    use super::GeistPixelAtlas;
    use crate::schema::AsciiFontVariant;

    #[test]
    fn regular_and_bold_differ_for_printable_character() {
        let regular = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelRegular);
        let bold = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelBold);

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
    fn regular_atlas_has_visible_pixels_for_letter_a() {
        let regular = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelRegular);
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
