use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use fontdue::Font;
use sha2::{Digest, Sha256};

pub const FONT_ASSET_HASH_MISMATCH: &str = "FONT_ASSET_HASH_MISMATCH";
pub const GEIST_PIXEL_DIR_REL: &str = "assets/fonts/geist_pixel";

pub const GEIST_PIXEL_FILES: [&str; 5] = [
    "GeistPixel-Square.ttf",
    "GeistPixel-Grid.ttf",
    "GeistPixel-Circle.ttf",
    "GeistPixel-Triangle.ttf",
    "GeistPixel-Line.ttf",
];

pub const FONT_ASSET_HASHES: [(&str, &str); 5] = [
    (
        "GeistPixel-Square.ttf",
        "ae5cc2ad5b210071b5371229a276fddf289c601836a2d90f6bb0d94846754e90",
    ),
    (
        "GeistPixel-Grid.ttf",
        "cc559e53d4e4016145a71df9bffbd41faa9335c169889c746be733126ae39a92",
    ),
    (
        "GeistPixel-Circle.ttf",
        "3ff695214159a4986ac37fc30b85c3f4710f4cbe48087c4066ffc54851386d30",
    ),
    (
        "GeistPixel-Triangle.ttf",
        "01e0423267700ab59fa0b4bcccc8cf91cfc5da8bcd60bb6541b3794563aade63",
    ),
    (
        "GeistPixel-Line.ttf",
        "d585be1e1fd947c9be3bff90bfcc527bf9b0f66f42f6f50cd6363063071d14f8",
    ),
];

pub fn verify_geist_pixel_bundle(manifest_root: &Path) -> Result<()> {
    let font_dir = manifest_root.join(GEIST_PIXEL_DIR_REL);
    let mut actual = Vec::new();

    let entries = fs::read_dir(&font_dir).with_context(|| {
        format!(
            "missing Geist Pixel font directory '{}'",
            font_dir.display()
        )
    })?;

    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed reading entries from Geist Pixel directory '{}'",
                font_dir.display()
            )
        })?;
        let file_type = entry.file_type().with_context(|| {
            format!(
                "failed reading file metadata in Geist Pixel directory '{}'",
                font_dir.display()
            )
        })?;
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".ttf") {
            actual.push(name);
        }
    }

    let missing = GEIST_PIXEL_FILES
        .iter()
        .copied()
        .filter(|name| !actual.iter().any(|actual_name| actual_name == name))
        .collect::<Vec<_>>();
    let extra = actual
        .iter()
        .filter(|name| !GEIST_PIXEL_FILES.contains(&name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut extra = extra;
    extra.sort();

    if !missing.is_empty() || !extra.is_empty() {
        bail!(
            "invalid Geist Pixel bundle in '{}': missing [{}], extra [{}]. Expected exactly: {}",
            font_dir.display(),
            missing.join(", "),
            extra.join(", "),
            GEIST_PIXEL_FILES.join(", ")
        );
    }

    for (file_name, expected_hash) in FONT_ASSET_HASHES {
        let path = font_dir.join(file_name);
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read font file '{}'", path.display()))?;
        let actual_hash = sha256_hex(&bytes);
        if actual_hash != expected_hash {
            bail!(
                "{}: {} expected sha256={} actual sha256={}",
                FONT_ASSET_HASH_MISMATCH,
                file_name,
                expected_hash,
                actual_hash
            );
        }
    }

    Ok(())
}

pub fn font_path(manifest_root: &Path, file_name: &str) -> Result<PathBuf> {
    if !GEIST_PIXEL_FILES.contains(&file_name) {
        bail!("unsupported Geist Pixel font file '{}'", file_name);
    }
    Ok(manifest_root.join(GEIST_PIXEL_DIR_REL).join(file_name))
}

pub fn read_verified_font_bytes(manifest_root: &Path, file_name: &str) -> Result<Vec<u8>> {
    verify_geist_pixel_bundle(manifest_root)?;
    let path = font_path(manifest_root, file_name)?;
    fs::read(&path).with_context(|| format!("failed to read font file '{}'", path.display()))
}

pub fn ensure_supported_codepoints(font: &Font, text: &str, font_name: &str) -> Result<()> {
    for ch in text.chars() {
        if matches!(ch, '\n' | '\r' | '\t') {
            continue;
        }
        if font.lookup_glyph_index(ch) == 0 {
            return Err(anyhow!(
                "unsupported Geist Pixel codepoint U+{:04X} ({}) in {}",
                ch as u32,
                ch.escape_default(),
                font_name
            ));
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
