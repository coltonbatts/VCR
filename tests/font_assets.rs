use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::tempdir;
use vcr::font_assets::{
    verify_geist_pixel_bundle, FONT_ASSET_HASHES, FONT_ASSET_HASH_MISMATCH, GEIST_PIXEL_FILES,
};

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn font_dir(root: &Path) -> PathBuf {
    root.join("assets/fonts/geist_pixel")
}

fn copy_font_bundle(dst_manifest_root: &Path) {
    let src_dir = font_dir(&manifest_root());
    let dst_dir = font_dir(dst_manifest_root);
    fs::create_dir_all(&dst_dir).unwrap();

    for file_name in GEIST_PIXEL_FILES {
        fs::copy(src_dir.join(file_name), dst_dir.join(file_name)).unwrap();
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[test]
fn geist_pixel_bundle_is_exact_and_hash_pinned() {
    let root = manifest_root();
    verify_geist_pixel_bundle(&root).expect("expected bundled Geist Pixel fonts to verify");

    let mut actual_ttf = fs::read_dir(font_dir(&root))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".ttf") {
                Some(name)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    actual_ttf.sort();

    let mut expected_ttf = GEIST_PIXEL_FILES
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    expected_ttf.sort();
    assert_eq!(
        actual_ttf, expected_ttf,
        "unexpected Geist Pixel font file set"
    );

    for (file_name, expected_hash) in FONT_ASSET_HASHES {
        let bytes = fs::read(font_dir(&root).join(file_name)).unwrap();
        assert_eq!(
            sha256_hex(&bytes),
            expected_hash,
            "hash mismatch for {}",
            file_name
        );
    }
}

#[test]
fn geist_pixel_bundle_rejects_extra_font_files() {
    let temp = tempdir().unwrap();
    copy_font_bundle(temp.path());
    let extra = font_dir(temp.path()).join("GeistPixel-Extra.ttf");
    fs::write(extra, b"not-a-real-font").unwrap();

    let err = verify_geist_pixel_bundle(temp.path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid Geist Pixel bundle"));
    assert!(err.contains("GeistPixel-Extra.ttf"));
}

#[test]
fn geist_pixel_bundle_reports_hash_mismatch() {
    let temp = tempdir().unwrap();
    copy_font_bundle(temp.path());
    let target = font_dir(temp.path()).join("GeistPixel-Line.ttf");
    let mut bytes = fs::read(&target).unwrap();
    bytes.push(0);
    fs::write(target, bytes).unwrap();

    let err = verify_geist_pixel_bundle(temp.path())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(FONT_ASSET_HASH_MISMATCH),
        "expected deterministic hash mismatch code, got: {err}"
    );
}
