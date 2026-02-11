use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "luminance_to_char_mapping_test",
    about = "Deterministic luminance quantization and character mapping regression test"
)]
struct Cli {
    #[arg(long, default_value_t = 128)]
    width: u32,

    #[arg(long, default_value_t = 72)]
    height: u32,

    #[arg(long, default_value = " .:-=+*#%@")]
    ramp: String,

    #[arg(long)]
    expected_hash: Option<String>,

    #[arg(long)]
    write_ascii: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_dims(cli.width, cli.height)?;
    let ramp = validate_ramp(&cli.ramp)?;
    verify_monotonic_quantizer(ramp.len())?;

    let ascii_frame = map_synthetic_field_to_ascii(cli.width, cli.height, ramp);
    let hash = fnv1a64(&ascii_frame);

    if let Some(path) = &cli.write_ascii {
        fs::write(path, &ascii_frame)
            .with_context(|| format!("failed writing ascii frame {}", path.display()))?;
    }

    if let Some(expected_raw) = &cli.expected_hash {
        let expected = parse_u64_hex_or_decimal(expected_raw)?;
        if hash != expected {
            bail!("hash mismatch: expected 0x{expected:016x}, actual 0x{hash:016x}");
        }
        println!("regression_check=pass");
    }

    println!("width={}", cli.width);
    println!("height={}", cli.height);
    println!("ramp_len={}", ramp.len());
    println!("hash_fnv1a64=0x{hash:016x}");

    Ok(())
}

fn validate_dims(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        bail!("width and height must be > 0");
    }
    Ok(())
}

fn validate_ramp(raw: &str) -> Result<&[u8]> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        bail!("ramp must not be empty");
    }
    if !bytes.iter().all(u8::is_ascii) {
        bail!("ramp must contain ASCII bytes only");
    }
    Ok(bytes)
}

fn verify_monotonic_quantizer(ramp_len: usize) -> Result<()> {
    let mut last = 0_usize;
    for y8 in 0_u16..=255_u16 {
        let idx = quantize_luma(y8 as u8, ramp_len);
        if idx < last {
            bail!(
                "quantizer is non-monotonic at Y={} ({} -> {})",
                y8,
                last,
                idx
            );
        }
        last = idx;
    }
    Ok(())
}

fn map_synthetic_field_to_ascii(width: u32, height: u32, ramp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((width as usize + 1) * height as usize);

    for y in 0..height {
        for x in 0..width {
            let y8 = synthetic_luma(x, y);
            let idx = quantize_luma(y8, ramp.len());
            out.push(ramp[idx]);
        }
        if y + 1 < height {
            out.push(b'\n');
        }
    }

    out
}

fn synthetic_luma(x: u32, y: u32) -> u8 {
    // Fully deterministic integer field with mixed frequencies.
    let term_a = (x.wrapping_mul(13)) & 0xFF;
    let term_b = (y.wrapping_mul(17)) & 0xFF;
    let term_c = ((x.wrapping_mul(y)) % 31).wrapping_mul(7) & 0xFF;
    ((term_a + term_b + term_c) & 0xFF) as u8
}

fn quantize_luma(y8: u8, ramp_len: usize) -> usize {
    if ramp_len <= 1 {
        return 0;
    }
    let max_idx = ramp_len as u32 - 1;
    ((u32::from(y8) * max_idx + 127) / 255) as usize
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
    fn quantizer_should_be_bounded() {
        for y in 0_u16..=255_u16 {
            let idx = quantize_luma(y as u8, 10);
            assert!(idx <= 9);
        }
    }

    #[test]
    fn synthetic_field_hash_should_be_stable() {
        let ramp = b" .:-=+*#%@";
        let first = fnv1a64(&map_synthetic_field_to_ascii(64, 32, ramp));
        let second = fnv1a64(&map_synthetic_field_to_ascii(64, 32, ramp));
        assert_eq!(first, second);
    }
}
