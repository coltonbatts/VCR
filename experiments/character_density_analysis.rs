use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, ValueEnum};
use vcr::ascii_atlas::GeistPixelAtlas;
use vcr::ascii_atlas_data::{ASCII_END, ASCII_START};
use vcr::schema::AsciiFontVariant;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum FontVariantArg {
    Regular,
    Medium,
    Bold,
    Light,
    Mono,
}

impl FontVariantArg {
    fn into_schema_variant(self) -> AsciiFontVariant {
        match self {
            Self::Regular => AsciiFontVariant::GeistPixelRegular,
            Self::Medium => AsciiFontVariant::GeistPixelMedium,
            Self::Bold => AsciiFontVariant::GeistPixelBold,
            Self::Light => AsciiFontVariant::GeistPixelLight,
            Self::Mono => AsciiFontVariant::GeistPixelMono,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "character_density_analysis",
    about = "Deterministically ranks printable ASCII by glyph darkness"
)]
struct Cli {
    #[arg(long, value_enum, default_value_t = FontVariantArg::Regular)]
    variant: FontVariantArg,

    #[arg(long)]
    write_ramp: Option<PathBuf>,

    #[arg(long)]
    write_table: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GlyphDensity {
    codepoint: u8,
    on_pixels: u32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let atlas = GeistPixelAtlas::new(cli.variant.into_schema_variant());

    let glyphs = rank_glyph_density(&atlas);
    let ramp = ramp_string(&glyphs)?;
    let table = density_table(&glyphs, atlas.glyph_width() * atlas.glyph_height());
    let ramp_hash = fnv1a64(ramp.as_bytes());

    if let Some(path) = &cli.write_ramp {
        fs::write(path, ramp.as_bytes())
            .with_context(|| format!("failed writing ramp file {}", path.display()))?;
    }

    if let Some(path) = &cli.write_table {
        fs::write(path, table.as_bytes())
            .with_context(|| format!("failed writing table file {}", path.display()))?;
    }

    println!("variant={:?}", cli.variant);
    println!("glyphs={} (printable ASCII)", glyphs.len());
    println!("ramp={}", ramp);
    println!("ramp_hash_fnv1a64=0x{ramp_hash:016x}");
    println!();
    print!("{table}");

    Ok(())
}

fn rank_glyph_density(atlas: &GeistPixelAtlas) -> Vec<GlyphDensity> {
    let mut glyphs = Vec::with_capacity((ASCII_END - ASCII_START + 1) as usize);

    for codepoint in ASCII_START..=ASCII_END {
        let mut on_pixels = 0_u32;
        for y in 0..atlas.glyph_height() {
            for x in 0..atlas.glyph_width() {
                if atlas.sample(codepoint, x, y) {
                    on_pixels += 1;
                }
            }
        }

        glyphs.push(GlyphDensity {
            codepoint,
            on_pixels,
        });
    }

    // Stable deterministic ordering: darkness first, codepoint tie-breaker.
    glyphs.sort_by_key(|glyph| (glyph.on_pixels, glyph.codepoint));
    glyphs
}

fn ramp_string(glyphs: &[GlyphDensity]) -> Result<String> {
    let mut ramp = String::with_capacity(glyphs.len());
    for glyph in glyphs {
        let ch = char::from(glyph.codepoint);
        if !ch.is_ascii_graphic() && ch != ' ' {
            return Err(anyhow!(
                "non-printable glyph escaped ranking set: 0x{:02X}",
                glyph.codepoint
            ));
        }
        ramp.push(ch);
    }
    Ok(ramp)
}

fn density_table(glyphs: &[GlyphDensity], pixels_per_cell: u32) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "codepoint\tglyph\ton_pixels\tdensity");

    for glyph in glyphs {
        let density = glyph.on_pixels as f64 / f64::from(pixels_per_cell);
        let display = display_ascii(glyph.codepoint);
        let _ = writeln!(
            &mut out,
            "0x{:02X}\t{}\t{}\t{:.6}",
            glyph.codepoint, display, glyph.on_pixels, density
        );
    }

    out
}

fn display_ascii(codepoint: u8) -> String {
    match codepoint {
        b' ' => "<space>".to_string(),
        b'\\' => "\\\\".to_string(),
        _ => char::from(codepoint).to_string(),
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_should_be_stable_for_same_atlas() {
        let atlas = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelRegular);
        let first = rank_glyph_density(&atlas);
        let second = rank_glyph_density(&atlas);
        assert_eq!(first, second);
    }

    #[test]
    fn ranking_should_be_monotonic() {
        let atlas = GeistPixelAtlas::new(AsciiFontVariant::GeistPixelRegular);
        let ranked = rank_glyph_density(&atlas);

        for pair in ranked.windows(2) {
            assert!(pair[0].on_pixels <= pair[1].on_pixels);
        }
    }
}
