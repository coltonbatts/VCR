use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use image::{GenericImageView, ImageReader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const LIBRARY_REGISTRY_REL_PATH: &str = "library/library.json";
pub const LIBRARY_ITEMS_REL_PATH: &str = "library/items";
const TRAILER_NORMALIZED_DURATION_SECONDS: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryItemType {
    Video,
    Image,
    Ascii,
    Frames,
    Lottie,
}

impl LibraryItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Image => "image",
            Self::Ascii => "ascii",
            Self::Frames => "frames",
            Self::Lottie => "lottie",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestSourceUsage {
    Image,
    Ascii,
    Sequence,
    Lottie,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryNormalizeProfile {
    Trailer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LibraryRegistry {
    #[serde(default = "default_registry_version")]
    pub version: u32,
    #[serde(default)]
    pub items: Vec<LibraryItem>,
}

impl Default for LibraryRegistry {
    fn default() -> Self {
        Self {
            version: default_registry_version(),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LibraryItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: LibraryItemType,
    pub path: String,
    pub sha256: String,
    pub spec: LibrarySpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<LibraryProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LibrarySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f32>,
    pub has_alpha: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LibraryProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LibraryAddRequest {
    pub source_path: PathBuf,
    pub id: String,
    pub item_type: Option<LibraryItemType>,
    pub normalize: Option<LibraryNormalizeProfile>,
}

#[derive(Debug, Clone)]
pub struct LibraryAddSummary {
    pub item: LibraryItem,
    pub stored_path: PathBuf,
    pub registry_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LibraryPreparedAsset {
    pub item: LibraryItem,
    pub stored_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LibraryListFilter {
    pub item_type: Option<LibraryItemType>,
    pub tag: Option<String>,
}

fn default_registry_version() -> u32 {
    1
}

pub fn default_registry_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(LIBRARY_REGISTRY_REL_PATH)
}

pub fn default_items_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(LIBRARY_ITEMS_REL_PATH)
}

pub fn parse_library_reference(raw: &str) -> Option<&str> {
    raw.trim().strip_prefix("library:").map(str::trim)
}

pub fn validate_library_id(id: &str) -> Result<()> {
    let raw = id.trim();
    if raw.is_empty() {
        bail!("library id cannot be empty");
    }
    if raw != id {
        bail!("library id must not have surrounding whitespace");
    }

    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        bail!("library id cannot be empty");
    };
    if !first.is_ascii_lowercase() {
        bail!(
            "invalid library id '{}': must start with a lowercase letter and use kebab-case",
            id
        );
    }

    if raw.ends_with('-') || raw.contains("--") {
        bail!(
            "invalid library id '{}': use kebab-case words separated by single hyphens",
            id
        );
    }

    for ch in raw.chars() {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
            bail!(
                "invalid library id '{}': use lowercase letters, digits, and '-' only",
                id
            );
        }
    }

    Ok(())
}

pub fn load_registry(workspace_root: &Path) -> Result<LibraryRegistry> {
    let registry_path = default_registry_path(workspace_root);
    if !registry_path.exists() {
        return Ok(LibraryRegistry::default());
    }
    let content = fs::read_to_string(&registry_path).with_context(|| {
        format!(
            "failed to read library registry {}",
            registry_path.display()
        )
    })?;
    let mut registry: LibraryRegistry = serde_json::from_str(&content).with_context(|| {
        format!(
            "failed to parse library registry JSON {}",
            registry_path.display()
        )
    })?;
    normalize_registry_in_place(&mut registry)?;
    Ok(registry)
}

pub fn save_registry(workspace_root: &Path, registry: &LibraryRegistry) -> Result<PathBuf> {
    let mut normalized = registry.clone();
    normalize_registry_in_place(&mut normalized)?;

    let path = default_registry_path(workspace_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create library directory {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(&normalized)
        .context("failed to serialize library registry JSON")?;
    fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("failed to write library registry {}", path.display()))?;
    Ok(path)
}

fn normalize_registry_in_place(registry: &mut LibraryRegistry) -> Result<()> {
    if registry.version == 0 {
        bail!("library registry version must be >= 1");
    }

    registry.items.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen = BTreeSet::new();
    for item in &mut registry.items {
        validate_library_id(&item.id)?;
        if !seen.insert(item.id.clone()) {
            bail!("duplicate library id '{}' in registry", item.id);
        }
        if item.path.trim().is_empty() {
            bail!("library item '{}' path cannot be empty", item.id);
        }
        if Path::new(&item.path).is_absolute() {
            bail!(
                "library item '{}' path must be workspace-relative, got absolute path '{}'",
                item.id,
                item.path
            );
        }
        if Path::new(&item.path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!(
                "library item '{}' path '{}' cannot contain '..'",
                item.id,
                item.path
            );
        }
        if item.sha256.len() != 64 || !item.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!(
                "library item '{}' has invalid sha256 '{}': expected 64 lowercase hex chars",
                item.id,
                item.sha256
            );
        }

        item.tags.sort();
        item.tags.dedup();
    }

    Ok(())
}

pub fn add_asset(workspace_root: &Path, request: &LibraryAddRequest) -> Result<LibraryAddSummary> {
    let item_dir = default_items_root(workspace_root).join(&request.id);
    let prepared = prepare_asset_for_storage(workspace_root, request, &item_dir)?;

    let mut registry = load_registry(workspace_root)?;
    if let Some(existing) = registry
        .items
        .iter_mut()
        .find(|entry| entry.id == prepared.item.id)
    {
        *existing = prepared.item.clone();
    } else {
        registry.items.push(prepared.item.clone());
    }
    let registry_path = save_registry(workspace_root, &registry)?;

    Ok(LibraryAddSummary {
        item: prepared.item,
        stored_path: prepared.stored_path,
        registry_path,
    })
}

pub fn prepare_asset_for_storage(
    workspace_root: &Path,
    request: &LibraryAddRequest,
    item_dir: &Path,
) -> Result<LibraryPreparedAsset> {
    validate_library_id(&request.id)?;

    if !request.source_path.exists() {
        bail!(
            "missing asset file: {}",
            request.source_path.to_string_lossy()
        );
    }

    let inferred_type = detect_item_type(&request.source_path)?;
    let item_type = request.item_type.unwrap_or(inferred_type);

    if matches!(request.normalize, Some(LibraryNormalizeProfile::Trailer))
        && item_type != LibraryItemType::Video
    {
        bail!(
            "--normalize trailer currently supports only type=video (got {})",
            item_type.as_str()
        );
    }

    if item_dir.exists() {
        fs::remove_dir_all(item_dir)
            .with_context(|| format!("failed to reset existing item dir {}", item_dir.display()))?;
    }
    fs::create_dir_all(item_dir)
        .with_context(|| format!("failed to create item dir {}", item_dir.display()))?;

    let stored_path = if item_type == LibraryItemType::Frames {
        if !request.source_path.is_dir() {
            bail!(
                "library add expected a directory for type=frames, got '{}'",
                request.source_path.display()
            );
        }
        let target_dir = item_dir.join("source");
        copy_dir_recursive(&request.source_path, &target_dir)?;
        target_dir
    } else {
        if !request.source_path.is_file() {
            bail!(
                "library add expected a file for type={}, got directory '{}'",
                item_type.as_str(),
                request.source_path.display()
            );
        }

        let extension = if matches!(request.normalize, Some(LibraryNormalizeProfile::Trailer)) {
            "mov".to_owned()
        } else {
            request
                .source_path
                .extension()
                .and_then(OsStr::to_str)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_else(|| default_extension_for_type(item_type).to_owned())
        };

        let target = item_dir.join(format!("source.{extension}"));
        if matches!(request.normalize, Some(LibraryNormalizeProfile::Trailer)) {
            normalize_video_for_trailer(&request.source_path, &target)?;
        } else {
            fs::copy(&request.source_path, &target).with_context(|| {
                format!(
                    "failed to copy '{}' to '{}'",
                    request.source_path.display(),
                    target.display()
                )
            })?;
        }
        target
    };

    let spec = probe_spec(item_type, &stored_path)?;
    let sha256 = compute_sha256_for_path(&stored_path)?;
    let relative_path = stored_path
        .strip_prefix(workspace_root)
        .map_err(|_| {
            anyhow!(
                "failed to store library item '{}' under workspace root {}",
                request.id,
                workspace_root.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");

    let item = LibraryItem {
        id: request.id.clone(),
        item_type,
        path: relative_path,
        sha256,
        spec,
        tags: Vec::new(),
        provenance: None,
    };

    Ok(LibraryPreparedAsset { item, stored_path })
}

pub fn list_items<'a>(
    registry: &'a LibraryRegistry,
    filter: &LibraryListFilter,
) -> Vec<&'a LibraryItem> {
    let mut items = registry.items.iter().collect::<Vec<_>>();
    if let Some(kind) = filter.item_type {
        items.retain(|item| item.item_type == kind);
    }
    if let Some(tag) = filter.tag.as_deref() {
        items.retain(|item| item.tags.iter().any(|item_tag| item_tag == tag));
    }
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items
}

pub fn find_registry_root(start_dir: &Path) -> Option<PathBuf> {
    for ancestor in start_dir.ancestors() {
        if default_registry_path(ancestor).is_file() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub fn verify_registry(workspace_root: &Path, registry: &LibraryRegistry) -> Result<()> {
    let mut failures = Vec::new();
    for item in &registry.items {
        if let Err(error) = verify_item(workspace_root, item) {
            failures.push(error.to_string());
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        let mut message = String::new();
        message.push_str("library verification failed:\n");
        for failure in failures {
            message.push_str("- ");
            message.push_str(&failure);
            message.push('\n');
        }
        bail!(message.trim_end().to_owned())
    }
}

pub fn resolve_manifest_library_reference(
    manifest_path: &Path,
    id: &str,
    usage: ManifestSourceUsage,
) -> Result<PathBuf> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let workspace_root = find_registry_root(manifest_dir)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| find_registry_root(&cwd))
        })
        .ok_or_else(|| {
            anyhow!(
                "could not locate {} from '{}'",
                LIBRARY_REGISTRY_REL_PATH,
                manifest_path.display()
            )
        })?;

    let registry = load_registry(&workspace_root)?;
    let item = registry
        .items
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| anyhow!("unknown library id '{}': run `vcr library list`", id))?;

    match usage {
        ManifestSourceUsage::Image => {
            if !matches!(item.item_type, LibraryItemType::Image) {
                bail!(
                    "library id '{}' has type '{}' but this layer expects an image asset",
                    item.id,
                    item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Ascii => {
            if !matches!(item.item_type, LibraryItemType::Ascii) {
                bail!(
                    "library id '{}' has type '{}' but this layer expects an ascii asset",
                    item.id,
                    item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Sequence => {
            if !matches!(item.item_type, LibraryItemType::Frames) {
                bail!(
                    "library id '{}' has type '{}' but this layer expects frames",
                    item.id,
                    item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Lottie => {
            if !matches!(item.item_type, LibraryItemType::Lottie) {
                bail!(
                    "library id '{}' has type '{}' but this layer expects a lottie asset",
                    item.id,
                    item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Video => {
            if !matches!(item.item_type, LibraryItemType::Video) {
                bail!(
                    "library id '{}' has type '{}' but this layer expects a video asset",
                    item.id,
                    item.item_type.as_str()
                );
            }
        }
    }

    verify_item(&workspace_root, item)?;
    Ok(workspace_root.join(&item.path))
}

pub fn verify_item(workspace_root: &Path, item: &LibraryItem) -> Result<()> {
    let asset_path = workspace_root.join(&item.path);
    if !asset_path.exists() {
        bail!(
            "missing asset file for id '{}': {}",
            item.id,
            asset_path.display()
        );
    }

    let actual_sha = compute_sha256_for_path(&asset_path)?;
    if actual_sha != item.sha256 {
        bail!(
            "hash mismatch for id '{}': expected {}, got {}",
            item.id,
            item.sha256,
            actual_sha
        );
    }

    let actual_spec = probe_spec(item.item_type, &asset_path)?;
    compare_specs(&item.id, &item.spec, &actual_spec)
}

fn compare_specs(id: &str, expected: &LibrarySpec, actual: &LibrarySpec) -> Result<()> {
    compare_optional_u32(id, "width", expected.width, actual.width)?;
    compare_optional_u32(id, "height", expected.height, actual.height)?;
    compare_optional_u32(id, "frames", expected.frames, actual.frames)?;
    compare_optional_string(
        id,
        "pixel_format",
        expected.pixel_format.as_deref(),
        actual.pixel_format.as_deref(),
    )?;

    if let Some(expected_fps) = expected.fps {
        let Some(actual_fps) = actual.fps else {
            bail!(
                "spec mismatch for id '{}': missing fps in probed media metadata",
                id
            );
        };
        if (expected_fps - actual_fps).abs() > 0.001 {
            bail!(
                "spec mismatch for id '{}': fps expected {:.6}, got {:.6}",
                id,
                expected_fps,
                actual_fps
            );
        }
    }

    if let Some(expected_duration) = expected.duration_seconds {
        let Some(actual_duration) = actual.duration_seconds else {
            bail!(
                "spec mismatch for id '{}': missing duration_seconds in probed media metadata",
                id
            );
        };
        if (expected_duration - actual_duration).abs() > 0.05 {
            bail!(
                "spec mismatch for id '{}': duration_seconds expected {:.3}, got {:.3}",
                id,
                expected_duration,
                actual_duration
            );
        }
    }

    if expected.has_alpha != actual.has_alpha {
        bail!(
            "spec mismatch for id '{}': has_alpha expected {}, got {}",
            id,
            expected.has_alpha,
            actual.has_alpha
        );
    }

    Ok(())
}

fn compare_optional_u32(
    id: &str,
    label: &str,
    expected: Option<u32>,
    actual: Option<u32>,
) -> Result<()> {
    if let Some(expected) = expected {
        let Some(actual) = actual else {
            bail!(
                "spec mismatch for id '{}': missing {} in probed media metadata",
                id,
                label
            );
        };
        if expected != actual {
            bail!(
                "spec mismatch for id '{}': {} expected {}, got {}",
                id,
                label,
                expected,
                actual
            );
        }
    }
    Ok(())
}

fn compare_optional_string(
    id: &str,
    label: &str,
    expected: Option<&str>,
    actual: Option<&str>,
) -> Result<()> {
    if let Some(expected) = expected {
        let Some(actual) = actual else {
            bail!(
                "spec mismatch for id '{}': missing {} in probed media metadata",
                id,
                label
            );
        };
        if expected != actual {
            bail!(
                "spec mismatch for id '{}': {} expected '{}', got '{}'",
                id,
                label,
                expected,
                actual
            );
        }
    }
    Ok(())
}

pub fn compute_sha256_for_path(path: &Path) -> Result<String> {
    if path.is_file() {
        return compute_sha256_for_file(path);
    }
    if path.is_dir() {
        let mut files = collect_files_recursively(path)?;
        files.sort();
        let mut hasher = Sha256::new();
        for file in files {
            let relative = file
                .strip_prefix(path)
                .map_err(|_| anyhow!("failed to compute relative path hash component"))?;
            hasher.update(relative.to_string_lossy().as_bytes());
            hasher.update([0]);
            let mut reader = fs::File::open(&file)
                .with_context(|| format!("failed to open frame file {}", file.display()))?;
            let mut buffer = [0_u8; 16 * 1024];
            loop {
                let read = reader
                    .read(&mut buffer)
                    .with_context(|| format!("failed to read frame file {}", file.display()))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
        }
        return Ok(format!("{:x}", hasher.finalize()));
    }

    bail!(
        "asset path is neither file nor directory: {}",
        path.display()
    )
}

fn compute_sha256_for_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open file for hashing {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read file for hashing {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files_recursively(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
        {
            let entry = entry
                .with_context(|| format!("failed reading directory entry in {}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn detect_item_type(path: &Path) -> Result<LibraryItemType> {
    if path.is_dir() {
        return Ok(LibraryItemType::Frames);
    }

    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| {
            anyhow!(
                "could not infer asset type for '{}': missing file extension; use --type",
                path.display()
            )
        })?;

    let item_type = match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => LibraryItemType::Image,
        "txt" | "asc" | "ascii" => LibraryItemType::Ascii,
        "mov" | "mp4" | "mkv" | "webm" | "gif" => LibraryItemType::Video,
        "json" => LibraryItemType::Lottie,
        _ => {
            bail!(
                "could not infer asset type for '{}': unsupported extension '.{}'; use --type",
                path.display(),
                extension
            )
        }
    };
    Ok(item_type)
}

fn default_extension_for_type(item_type: LibraryItemType) -> &'static str {
    match item_type {
        LibraryItemType::Video => "mov",
        LibraryItemType::Image => "png",
        LibraryItemType::Ascii => "txt",
        LibraryItemType::Frames => "",
        LibraryItemType::Lottie => "json",
    }
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)
        .with_context(|| format!("failed to create directory {}", to.display()))?;
    for entry in fs::read_dir(from)
        .with_context(|| format!("failed to read directory {}", from.display()))?
    {
        let entry = entry
            .with_context(|| format!("failed reading directory entry in {}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else if source.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
            fs::copy(&source, &target).with_context(|| {
                format!(
                    "failed to copy '{}' to '{}'",
                    source.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn normalize_video_for_trailer(source_path: &Path, output_path: &Path) -> Result<()> {
    let ffmpeg_status = Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match ffmpeg_status {
        Ok(status) if status.success() => {}
        Ok(_) | Err(_) => {
            bail!(
                "failed to run trailer normalization: ffmpeg is required on PATH for --normalize trailer"
            )
        }
    }

    let status = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-stream_loop")
        .arg("-1")
        .arg("-i")
        .arg(source_path)
        .arg("-t")
        .arg(format!("{TRAILER_NORMALIZED_DURATION_SECONDS}"))
        .arg("-vf")
        .arg("fps=24,scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2:color=black@0")
        .arg("-an")
        .arg("-c:v")
        .arg("prores_ks")
        .arg("-profile:v")
        .arg("4")
        .arg("-pix_fmt")
        .arg("yuva444p10le")
        .arg(output_path)
        .status()
        .with_context(|| {
            format!(
                "failed to spawn ffmpeg for trailer normalization of {}",
                source_path.display()
            )
        })?;

    if !status.success() {
        bail!(
            "ffmpeg trailer normalization failed for {} (exit status: {})",
            source_path.display(),
            status
        );
    }

    Ok(())
}

fn probe_spec(item_type: LibraryItemType, path: &Path) -> Result<LibrarySpec> {
    match item_type {
        LibraryItemType::Image => probe_image_spec(path),
        LibraryItemType::Ascii => probe_ascii_spec(path),
        LibraryItemType::Video => probe_video_spec(path),
        LibraryItemType::Frames => probe_frames_spec(path),
        LibraryItemType::Lottie => probe_lottie_spec(path),
    }
}

fn probe_lottie_spec(path: &Path) -> Result<LibrarySpec> {
    let content = std::fs::read_to_string(path)?;
    let composition = velato::Composition::from_str(&content)
        .map_err(|e| anyhow!("failed to parse lottie for spec: {:?}", e))?;

    Ok(LibrarySpec {
        width: Some(composition.width as u32),
        height: Some(composition.height as u32),
        fps: Some(composition.frame_rate as f32),
        frames: Some((composition.frames.end - composition.frames.start) as u32),
        duration_seconds: Some(
            (composition.frames.end - composition.frames.start) as f32
                / composition.frame_rate as f32,
        ),
        has_alpha: true,
        pixel_format: None,
    })
}

fn probe_image_spec(path: &Path) -> Result<LibrarySpec> {
    let image = ImageReader::open(path)
        .with_context(|| format!("failed to open image {}", path.display()))?
        .decode()
        .with_context(|| format!("failed to decode image {}", path.display()))?;

    let (width, height) = image.dimensions();
    let color = image.color();

    Ok(LibrarySpec {
        width: Some(width),
        height: Some(height),
        fps: None,
        frames: None,
        duration_seconds: None,
        has_alpha: color.has_alpha(),
        pixel_format: Some(format!("{:?}", color).to_ascii_lowercase()),
    })
}

fn probe_ascii_spec(path: &Path) -> Result<LibrarySpec> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read ascii asset {}", path.display()))?;
    let mut width = 0_u32;
    let mut height = 0_u32;
    for line in raw.lines() {
        let trimmed = line.strip_suffix('\r').unwrap_or(line);
        height = height.saturating_add(1);
        width = width.max(trimmed.chars().count() as u32);
    }

    Ok(LibrarySpec {
        width: Some(width),
        height: Some(height),
        fps: None,
        frames: None,
        duration_seconds: None,
        has_alpha: false,
        pixel_format: Some("ascii".to_owned()),
    })
}

fn probe_video_spec(path: &Path) -> Result<LibrarySpec> {
    #[derive(Debug, Deserialize)]
    struct FfprobeOutput {
        #[serde(default)]
        streams: Vec<FfprobeStream>,
        #[serde(default)]
        format: Option<FfprobeFormat>,
    }

    #[derive(Debug, Deserialize)]
    struct FfprobeStream {
        #[serde(default)]
        codec_type: Option<String>,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
        #[serde(default)]
        pix_fmt: Option<String>,
        #[serde(default)]
        r_frame_rate: Option<String>,
        #[serde(default)]
        avg_frame_rate: Option<String>,
        #[serde(default)]
        nb_frames: Option<String>,
        #[serde(default)]
        duration: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct FfprobeFormat {
        #[serde(default)]
        duration: Option<String>,
    }

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_streams")
        .arg("-show_format")
        .arg("-print_format")
        .arg("json")
        .arg(path)
        .output()
        .with_context(|| format!("failed to spawn ffprobe for {}", path.display()))?;

    if !output.status.success() {
        bail!(
            "ffprobe failed for {} (exit status: {})",
            path.display(),
            output.status
        );
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("failed to parse ffprobe JSON for {}", path.display()))?;

    let stream = parsed
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| {
            anyhow!(
                "ffprobe did not report a video stream for {}",
                path.display()
            )
        })?;

    let fps = stream
        .r_frame_rate
        .as_deref()
        .and_then(parse_ffprobe_rate)
        .or_else(|| {
            stream
                .avg_frame_rate
                .as_deref()
                .and_then(parse_ffprobe_rate)
        })
        .ok_or_else(|| anyhow!("ffprobe did not provide a valid fps for {}", path.display()))?;

    let duration = stream
        .duration
        .as_deref()
        .and_then(parse_ffprobe_float)
        .or_else(|| {
            parsed
                .format
                .as_ref()
                .and_then(|format| format.duration.as_deref())
                .and_then(parse_ffprobe_float)
        });

    let frames = stream
        .nb_frames
        .as_deref()
        .and_then(parse_ffprobe_u32)
        .or_else(|| duration.map(|value| (value * fps).round().max(1.0) as u32));

    let pixel_format = stream.pix_fmt.clone();
    let has_alpha = pixel_format
        .as_deref()
        .map(pixel_format_has_alpha)
        .unwrap_or(false);

    Ok(LibrarySpec {
        width: stream.width,
        height: stream.height,
        fps: Some(fps),
        frames,
        duration_seconds: duration,
        has_alpha,
        pixel_format,
    })
}

fn probe_frames_spec(path: &Path) -> Result<LibrarySpec> {
    if !path.is_dir() {
        bail!("frames asset path is not a directory: {}", path.display());
    }

    let mut files = collect_files_recursively(path)?;
    files.sort();
    let first = files
        .first()
        .ok_or_else(|| anyhow!("frames asset has no files: {}", path.display()))?;

    let image = ImageReader::open(first)
        .with_context(|| format!("failed to open first frame image {}", first.display()))?
        .decode()
        .with_context(|| format!("failed to decode first frame image {}", first.display()))?;
    let (width, height) = image.dimensions();
    let color = image.color();

    Ok(LibrarySpec {
        width: Some(width),
        height: Some(height),
        fps: None,
        frames: Some(files.len() as u32),
        duration_seconds: None,
        has_alpha: color.has_alpha(),
        pixel_format: Some(format!("{:?}", color).to_ascii_lowercase()),
    })
}

fn parse_ffprobe_rate(raw: &str) -> Option<f32> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator = numerator.trim().parse::<f32>().ok()?;
        let denominator = denominator.trim().parse::<f32>().ok()?;
        if denominator.abs() <= f32::EPSILON {
            return None;
        }
        let result = numerator / denominator;
        if result.is_finite() && result > 0.0 {
            return Some(result);
        }
        return None;
    }

    let parsed = value.parse::<f32>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
}

fn parse_ffprobe_float(raw: &str) -> Option<f32> {
    let parsed = raw.trim().parse::<f32>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
}

fn parse_ffprobe_u32(raw: &str) -> Option<u32> {
    raw.trim().parse::<u32>().ok().filter(|value| *value > 0)
}

fn pixel_format_has_alpha(pixel_format: &str) -> bool {
    let lowered = pixel_format.to_ascii_lowercase();
    lowered.contains("yuva")
        || lowered.contains("rgba")
        || lowered.contains("argb")
        || lowered.contains("bgra")
        || lowered.contains("abgr")
        || lowered.contains("ya")
}

#[cfg(test)]
mod tests {
    use super::{
        add_asset, compute_sha256_for_path, load_registry, save_registry, validate_library_id,
        verify_item, LibraryAddRequest, LibraryItem, LibraryItemType, LibraryRegistry, LibrarySpec,
    };
    use tempfile::tempdir;

    #[test]
    fn validates_library_id_rules() {
        for valid in ["alpha", "alpha-1", "vcr-demo-asset", "a1-b2-c3"] {
            validate_library_id(valid).expect("valid id should pass");
        }

        for invalid in [
            "",
            "Alpha",
            "snake_case",
            "alpha--beta",
            "alpha-",
            "-alpha",
            "alpha beta",
        ] {
            assert!(
                validate_library_id(invalid).is_err(),
                "invalid id should fail: {invalid}"
            );
        }
    }

    #[test]
    fn registry_write_is_stable_and_sorted() {
        let dir = tempdir().expect("tempdir should create");
        let mut registry = LibraryRegistry {
            version: 1,
            items: vec![
                LibraryItem {
                    id: "zeta-item".to_owned(),
                    item_type: LibraryItemType::Ascii,
                    path: "library/items/zeta-item/source.txt".to_owned(),
                    sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                        .to_owned(),
                    spec: LibrarySpec {
                        width: Some(1),
                        height: Some(1),
                        fps: None,
                        frames: None,
                        duration_seconds: None,
                        has_alpha: false,
                        pixel_format: Some("ascii".to_owned()),
                    },
                    tags: vec!["z".to_owned(), "z".to_owned(), "a".to_owned()],
                    provenance: None,
                },
                LibraryItem {
                    id: "alpha-item".to_owned(),
                    item_type: LibraryItemType::Ascii,
                    path: "library/items/alpha-item/source.txt".to_owned(),
                    sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                        .to_owned(),
                    spec: LibrarySpec {
                        width: Some(1),
                        height: Some(1),
                        fps: None,
                        frames: None,
                        duration_seconds: None,
                        has_alpha: false,
                        pixel_format: Some("ascii".to_owned()),
                    },
                    tags: vec!["m".to_owned(), "a".to_owned()],
                    provenance: None,
                },
            ],
        };

        let path = save_registry(dir.path(), &registry).expect("registry should save");
        let first = std::fs::read_to_string(&path).expect("registry should read");

        registry.items.reverse();
        save_registry(dir.path(), &registry).expect("registry should save again");
        let second = std::fs::read_to_string(&path).expect("registry should read again");

        assert_eq!(first, second, "registry output should be stable");
        let loaded = load_registry(dir.path()).expect("registry should load");
        assert_eq!(loaded.items[0].id, "alpha-item");
        assert_eq!(loaded.items[1].id, "zeta-item");
        assert_eq!(loaded.items[1].tags, vec!["a".to_owned(), "z".to_owned()]);
    }

    #[test]
    fn hash_verification_detects_mutation() {
        let dir = tempdir().expect("tempdir should create");
        let source = dir.path().join("source.txt");
        std::fs::write(&source, "HELLO\n").expect("source should write");

        let summary = add_asset(
            dir.path(),
            &LibraryAddRequest {
                source_path: source,
                id: "tiny-ascii".to_owned(),
                item_type: Some(LibraryItemType::Ascii),
                normalize: None,
            },
        )
        .expect("add should succeed");

        verify_item(dir.path(), &summary.item).expect("verify should pass before mutation");

        std::fs::write(&summary.stored_path, "MUTATED\n").expect("mutation should write");
        let error = verify_item(dir.path(), &summary.item)
            .expect_err("verify should fail after mutation")
            .to_string();
        assert!(error.contains("hash mismatch"));

        let recomputed = compute_sha256_for_path(&summary.stored_path)
            .expect("sha should compute for mutated file");
        assert_ne!(recomputed, summary.item.sha256);
    }
}
