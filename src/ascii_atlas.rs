use crate::schema::AsciiFontVariant;

const ASCII_GLYPH_WIDTH: u32 = 8;
const ASCII_GLYPH_HEIGHT: u32 = 8;

#[derive(Debug, Clone)]
pub struct GeistPixelAtlas {
    variant: AsciiFontVariant,
}

impl GeistPixelAtlas {
    pub fn new(variant: AsciiFontVariant) -> Self {
        Self { variant }
    }

    pub fn sample(&self, character: u8, x: u32, y: u32) -> bool {
        let base = sample_base_glyph(character, x, y);
        match self.variant {
            AsciiFontVariant::GeistPixelRegular | AsciiFontVariant::GeistPixelMono => base,
            AsciiFontVariant::GeistPixelMedium => {
                base || sample_base_glyph(character, x.saturating_sub(1), y)
            }
            AsciiFontVariant::GeistPixelBold => {
                base || sample_base_glyph(character, x.saturating_sub(1), y)
                    || sample_base_glyph(character, x, y.saturating_sub(1))
            }
            AsciiFontVariant::GeistPixelLight => {
                if !base {
                    return false;
                }

                let left = sample_base_glyph(character, x.saturating_sub(1), y);
                let right = sample_base_glyph(character, (x + 1).min(ASCII_GLYPH_WIDTH - 1), y);
                let up = sample_base_glyph(character, x, y.saturating_sub(1));
                let down = sample_base_glyph(character, x, (y + 1).min(ASCII_GLYPH_HEIGHT - 1));
                !(left && right && up && down)
            }
        }
    }
}

fn sample_base_glyph(character: u8, x: u32, y: u32) -> bool {
    if x >= ASCII_GLYPH_WIDTH || y >= ASCII_GLYPH_HEIGHT {
        return false;
    }
    if character == b' ' {
        return false;
    }

    // Atlas boundary: this deterministic synthetic glyph encoding is temporary.
    // Replace with baked Geist Pixel atlas bitmaps for each fixed variant.
    if !(1..=5).contains(&y) {
        return false;
    }

    let local_y = y - 1;
    let high_nibble = (character >> 4) & 0x0F;
    let low_nibble = character & 0x0F;

    if x <= 2 {
        return sample_hex_digit(high_nibble, x, local_y);
    }
    if (4..=6).contains(&x) {
        return sample_hex_digit(low_nibble, x - 4, local_y);
    }

    false
}

fn sample_hex_digit(nibble: u8, x: u32, y: u32) -> bool {
    let [a, b, c, d, e, f, g] = hex_segments(nibble);

    (a && y == 0 && x <= 2)
        || (b && x == 2 && y <= 2)
        || (c && x == 2 && y >= 2)
        || (d && y == 4 && x <= 2)
        || (e && x == 0 && y >= 2)
        || (f && x == 0 && y <= 2)
        || (g && y == 2 && x <= 2)
}

fn hex_segments(nibble: u8) -> [bool; 7] {
    match nibble {
        0x0 => [true, true, true, true, true, true, false],
        0x1 => [false, true, true, false, false, false, false],
        0x2 => [true, true, false, true, true, false, true],
        0x3 => [true, true, true, true, false, false, true],
        0x4 => [false, true, true, false, false, true, true],
        0x5 => [true, false, true, true, false, true, true],
        0x6 => [true, false, true, true, true, true, true],
        0x7 => [true, true, true, false, false, false, false],
        0x8 => [true, true, true, true, true, true, true],
        0x9 => [true, true, true, true, false, true, true],
        0xA => [true, true, true, false, true, true, true],
        0xB => [false, false, true, true, true, true, true],
        0xC => [true, false, false, true, true, true, false],
        0xD => [false, true, true, true, true, false, true],
        0xE => [true, false, false, true, true, true, true],
        0xF => [true, false, false, false, true, true, true],
        _ => [false; 7],
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
        for y in 0..8 {
            for x in 0..8 {
                if regular.sample(b'A', x, y) != bold.sample(b'A', x, y) {
                    any_difference = true;
                    break;
                }
            }
        }

        assert!(any_difference);
    }
}
