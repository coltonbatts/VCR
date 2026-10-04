//! Execution provenance and atomic publication helpers.
//!
//! Two sidecars exist per rendered artifact, with different jobs:
//! - `<out>.metadata.json` — the *scene record*: deterministic and machine-independent.
//!   Byte-identical for identical effective inputs, regardless of output filename.
//! - `<out>.provenance.json` — the *execution record*: which engine/toolchain/backend produced
//!   the bytes, the hashes at each level, the inputs, and verification results. Written last, so
//!   its presence (with a matching output hash) is what marks an export complete.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::agent_contract::{sha256_file, sha256_hex, EngineIdentity};
use crate::schema::{Layer, Manifest};

pub const PROVENANCE_SCHEMA: &str = "vcr.provenance/1";

pub fn provenance_path_for(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "render".to_owned());
    output
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(format!("{name}.provenance.json"))
}

/// A hidden sibling that keeps the output's extension so ffmpeg selects the right container.
pub fn partial_path_for(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "render".to_owned());
    let ext = output
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    output
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(format!(".{stem}.partial-{}{ext}", std::process::id()))
}

/// Removes the partial file unless disarmed (i.e. the render was published).
pub struct PartialGuard {
    path: PathBuf,
    armed: bool,
}

impl PartialGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Write `bytes` to a sibling temp file and rename over `path` (atomic on one filesystem).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id()
    ));
    fs::write(&tmp, bytes).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Delete sidecars that describe a previous artifact at `output`, before replacing it. A render
/// that then fails leaves an old file with *no* provenance rather than a stale "complete" record.
pub fn invalidate_sidecars(output: &Path) {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = output.parent().map(Path::to_path_buf).unwrap_or_default();
    let _ = fs::remove_file(provenance_path_for(output));
    let _ = fs::remove_file(dir.join(format!("{name}.metadata.json")));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputRef {
    pub layer: String,
    pub field: String,
    /// Path as authored, relative to the manifest directory.
    pub path: String,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub exists: bool,
}

fn hash_input(sandbox_root: &Path, layer: &str, field: &str, rel: &Path) -> InputRef {
    let full = sandbox_root.join(rel);
    InputRef {
        layer: layer.to_owned(),
        field: field.to_owned(),
        path: rel.display().to_string(),
        sha256: sha256_file(&full).ok(),
        bytes: fs::metadata(&full).ok().map(|m| m.len()),
        exists: full.is_file(),
    }
}

/// Every external file the manifest's layers reference, with content hashes.
pub fn collect_inputs(manifest: &Manifest) -> Vec<InputRef> {
    let Some(sandbox) = manifest.sandbox.as_ref() else {
        return Vec::new();
    };
    let root = sandbox.root();
    let mut inputs = Vec::new();
    for layer in &manifest.layers {
        let id = layer.id();
        match layer {
            Layer::Asset(l) => inputs.push(hash_input(root, id, "source_path", &l.source_path)),
            Layer::Image(l) => inputs.push(hash_input(root, id, "image.path", &l.image.path)),
            Layer::Video(l) => inputs.push(hash_input(root, id, "video.path", &l.video.path)),
            Layer::Lottie(l) => inputs.push(hash_input(root, id, "lottie.path", &l.lottie.path)),
            Layer::Shader(l) => {
                if let Some(path) = &l.shader.path {
                    inputs.push(hash_input(root, id, "shader.path", path));
                }
            }
            Layer::WgpuShader(l) => inputs.push(hash_input(
                root,
                id,
                "wgpu_shader.shader_path",
                &l.wgpu_shader.shader_path,
            )),
            Layer::Sequence(l) => {
                // Directory-level digest: hash of the sorted per-file hashes.
                let dir = root.join(&l.sequence.path);
                let mut files: Vec<_> = fs::read_dir(&dir)
                    .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
                    .unwrap_or_default();
                files.sort();
                let digest = files
                    .iter()
                    .filter_map(|f| sha256_file(f).ok())
                    .collect::<Vec<_>>()
                    .join("\n");
                inputs.push(InputRef {
                    layer: id.to_owned(),
                    field: "sequence.path".to_owned(),
                    path: l.sequence.path.display().to_string(),
                    sha256: (!files.is_empty()).then(|| sha256_hex(digest.as_bytes())),
                    bytes: None,
                    exists: dir.is_dir(),
                });
            }
            _ => {}
        }
    }
    inputs
}

pub fn ffmpeg_version_line() -> Option<String> {
    let out = Command::new("ffmpeg").arg("-version").output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_owned())
}

/// SHA-256 of the decoded RGBA video stream (what a player/compositor would see).
pub fn decoded_frames_sha256(path: &Path) -> Result<String> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map", "0:v:0", "-pix_fmt", "rgba", "-f", "hash", "-hash", "sha256", "-",
        ])
        .output()
        .map_err(|e| anyhow!("failed to run ffmpeg for decoded hash: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find_map(|l| l.trim().strip_prefix("SHA256=").map(str::to_owned))
        .ok_or_else(|| anyhow!("ffmpeg did not report a decoded hash"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub schema: &'static str,
    /// `complete` is written only after publication and verification succeeded.
    pub status: &'static str,
    pub engine: EngineIdentity,
    pub toolchain: BTreeMap<&'static str, Option<String>>,
    pub backend: serde_json::Value,
    pub manifest: serde_json::Value,
    pub inputs: Vec<InputRef>,
    pub window: serde_json::Value,
    pub output: serde_json::Value,
    pub hashes: serde_json::Value,
    pub determinism: serde_json::Value,
    pub verification: serde_json::Value,
    pub sidecars: serde_json::Value,
}

/// What reproducibility we claim, and the conditions under which the claim holds.
pub fn determinism_statement(backend: &str, seed: u64) -> serde_json::Value {
    let software = backend.eq_ignore_ascii_case("cpu") || backend.eq_ignore_ascii_case("software");
    serde_json::json!({
        "seed": seed,
        "backend": backend,
        "levels": {
            "scene_and_settings": "reproducible: same manifest, params and seed resolve to the same settings (manifest/render hashes)",
            "raster_frames": if software {
                "expected identical (raster_frames_sha256) for the same engine build on the software backend; matches across machines in the repo's golden test"
            } else {
                "not guaranteed: GPU output can differ across hardware, drivers and OS"
            },
            "decoded_frames": "expected identical only when encoded bytes are identical (ProRes is lossy; decode is deterministic)",
            "encoded_file_bytes": "identical only for the same ffmpeg build and arguments (see toolchain.ffmpeg); different ffmpeg builds can produce different bytes from identical frames",
        },
        "conditions": "compare engine.version/git_hash, engine.os/arch, toolchain.ffmpeg and backend before treating a hash difference as a defect",
    })
}
