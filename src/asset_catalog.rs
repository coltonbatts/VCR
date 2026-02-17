use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::library::{
    default_registry_path, load_registry, parse_library_reference, prepare_asset_for_storage,
    validate_library_id, verify_item, LibraryAddRequest, LibraryItem, LibraryItemType,
    ManifestSourceUsage,
};

pub const PACKS_ROOT_REL_PATH: &str = "packs";
pub const PACK_MANIFEST_FILE: &str = "pack.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackReference<'a> {
    pub pack_id: &'a str,
    pub asset_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetReference<'a> {
    Library(&'a str),
    Pack(PackReference<'a>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    #[serde(default = "default_pack_manifest_version")]
    pub version: u32,
    #[serde(default)]
    pub pack_id: String,
    #[serde(default)]
    pub items: Vec<LibraryItem>,
}

impl Default for PackManifest {
    fn default() -> Self {
        Self {
            version: default_pack_manifest_version(),
            pack_id: String::new(),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetCatalogOrigin {
    Library,
    Pack { pack_id: String },
}

#[derive(Debug, Clone)]
pub struct AssetCatalogEntry {
    pub reference: String,
    pub origin: AssetCatalogOrigin,
    pub item: LibraryItem,
}

#[derive(Debug, Clone)]
pub struct PackAddSummary {
    pub asset_reference: String,
    pub item: LibraryItem,
    pub stored_path: PathBuf,
    pub pack_manifest_path: PathBuf,
}

pub fn parse_pack_reference(raw: &str) -> Option<PackReference<'_>> {
    let trimmed = raw.trim();
    let without_prefix = trimmed.strip_prefix("pack:")?;
    let (pack_id_raw, asset_id_raw) = without_prefix.split_once('/')?;

    if asset_id_raw.contains('/') {
        return None;
    }

    let pack_id = pack_id_raw.trim();
    let asset_id = asset_id_raw.trim();
    if pack_id.is_empty() || asset_id.is_empty() {
        return None;
    }

    Some(PackReference { pack_id, asset_id })
}

pub fn parse_asset_reference(raw: &str) -> Option<AssetReference<'_>> {
    let trimmed = raw.trim();
    if let Some(id) = parse_library_reference(trimmed).filter(|id| !id.is_empty()) {
        return Some(AssetReference::Library(id));
    }
    parse_pack_reference(trimmed).map(AssetReference::Pack)
}

pub fn normalize_asset_reference(raw: &str) -> Option<String> {
    match parse_asset_reference(raw)? {
        AssetReference::Library(id) => Some(format!("library:{id}")),
        AssetReference::Pack(reference) => {
            Some(format!("pack:{}/{}", reference.pack_id, reference.asset_id))
        }
    }
}

pub fn suggest_asset_id(path: &Path) -> String {
    let seed = if path.is_dir() {
        path.file_name().and_then(OsStr::to_str).unwrap_or("asset")
    } else {
        path.file_stem().and_then(OsStr::to_str).unwrap_or("asset")
    };

    let mut out = String::new();
    let mut previous_dash = false;
    for ch in seed.chars() {
        if ch.is_ascii_alphanumeric() {
            let lowered = ch.to_ascii_lowercase();
            if out.is_empty() && !lowered.is_ascii_lowercase() {
                out.push('a');
                out.push('-');
            }
            out.push(lowered);
            previous_dash = false;
        } else if !out.is_empty() && !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }

    let normalized = out.trim_matches('-').to_owned();
    if normalized.is_empty() {
        return "asset".to_owned();
    }
    if validate_library_id(&normalized).is_ok() {
        return normalized;
    }
    "asset".to_owned()
}

pub fn suggest_library_asset_id(workspace_root: &Path, source_path: &Path) -> Result<String> {
    let base = suggest_asset_id(source_path);
    let registry = load_registry(workspace_root)?;
    let existing = registry
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();
    Ok(choose_unique_id(&base, &existing))
}

pub fn suggest_pack_asset_id(
    workspace_root: &Path,
    pack_id: &str,
    source_path: &Path,
) -> Result<String> {
    validate_library_id(pack_id)?;
    let base = suggest_asset_id(source_path);
    let existing = load_pack_item_ids(workspace_root, pack_id)?;
    Ok(choose_unique_id(&base, &existing))
}

pub fn load_asset_catalog(workspace_root: &Path) -> Result<Vec<AssetCatalogEntry>> {
    let mut out = Vec::new();

    let registry = load_registry(workspace_root)?;
    for item in registry.items {
        out.push(AssetCatalogEntry {
            reference: format!("library:{}", item.id),
            origin: AssetCatalogOrigin::Library,
            item,
        });
    }

    for (pack_id, manifest) in load_all_pack_manifests(workspace_root)? {
        for raw_item in manifest.items {
            let mut item = raw_item;
            item.path = pack_item_workspace_path(&pack_id, &item.path)?;
            out.push(AssetCatalogEntry {
                reference: format!("pack:{}/{}", pack_id, item.id),
                origin: AssetCatalogOrigin::Pack {
                    pack_id: pack_id.clone(),
                },
                item,
            });
        }
    }

    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    Ok(out)
}

pub fn search_asset_catalog<'a>(
    entries: &'a [AssetCatalogEntry],
    term: &str,
) -> Vec<&'a AssetCatalogEntry> {
    let needle = term.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return entries.iter().collect();
    }

    entries
        .iter()
        .filter(|entry| {
            entry.reference.to_ascii_lowercase().contains(&needle)
                || entry.item.id.to_ascii_lowercase().contains(&needle)
                || entry.item.path.to_ascii_lowercase().contains(&needle)
                || entry
                    .item
                    .tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&needle))
        })
        .collect()
}

pub fn resolve_manifest_asset_reference(
    manifest_path: &Path,
    raw_reference: &str,
    usage: ManifestSourceUsage,
) -> Result<PathBuf> {
    let normalized_reference = normalize_asset_reference(raw_reference).ok_or_else(|| {
        anyhow!(
            "invalid asset reference '{}': expected library:<id> or pack:<pack-id>/<asset-id>",
            raw_reference
        )
    })?;

    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let workspace_root = find_asset_catalog_root(manifest_dir)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| find_asset_catalog_root(&cwd))
        })
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| anyhow!("failed to resolve workspace root for asset catalog"))?;

    let catalog = load_asset_catalog(&workspace_root)?;
    let entry = catalog
        .iter()
        .find(|entry| entry.reference == normalized_reference)
        .ok_or_else(|| {
            anyhow!(
                "unknown asset reference '{}': run `vcr assets`",
                normalized_reference
            )
        })?;

    match usage {
        ManifestSourceUsage::Image => {
            if !matches!(entry.item.item_type, LibraryItemType::Image) {
                bail!(
                    "asset reference '{}' has type '{}' but this layer expects an image asset",
                    entry.reference,
                    entry.item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Ascii => {
            if !matches!(entry.item.item_type, LibraryItemType::Ascii) {
                bail!(
                    "asset reference '{}' has type '{}' but this layer expects an ascii asset",
                    entry.reference,
                    entry.item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Sequence => {
            if !matches!(entry.item.item_type, LibraryItemType::Frames) {
                bail!(
                    "asset reference '{}' has type '{}' but this layer expects frames",
                    entry.reference,
                    entry.item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Lottie => {
            if !matches!(entry.item.item_type, LibraryItemType::Lottie) {
                bail!(
                    "asset reference '{}' has type '{}' but this layer expects a lottie asset",
                    entry.reference,
                    entry.item.item_type.as_str()
                );
            }
        }
        ManifestSourceUsage::Video => {
            if !matches!(entry.item.item_type, LibraryItemType::Video) {
                bail!(
                    "asset reference '{}' has type '{}' but this layer expects a video asset",
                    entry.reference,
                    entry.item.item_type.as_str()
                );
            }
        }
    }

    verify_item(&workspace_root, &entry.item)?;
    Ok(workspace_root.join(&entry.item.path))
}

pub fn add_asset_to_pack(
    workspace_root: &Path,
    pack_id: &str,
    request: &LibraryAddRequest,
) -> Result<PackAddSummary> {
    validate_library_id(pack_id)?;
    validate_library_id(&request.id)?;

    let item_root = workspace_root
        .join(PACKS_ROOT_REL_PATH)
        .join(pack_id)
        .join("items")
        .join(&request.id);

    let prepared = prepare_asset_for_storage(workspace_root, request, &item_root)?;

    let pack_root = workspace_root.join(PACKS_ROOT_REL_PATH).join(pack_id);
    let pack_relative_path = prepared
        .stored_path
        .strip_prefix(&pack_root)
        .map_err(|_| {
            anyhow!(
                "failed to store pack item '{}' under pack root {}",
                request.id,
                pack_root.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");

    let mut manifest =
        read_pack_manifest(workspace_root, pack_id)?.unwrap_or_else(|| PackManifest {
            version: default_pack_manifest_version(),
            pack_id: pack_id.to_owned(),
            items: Vec::new(),
        });

    let mut pack_item = prepared.item.clone();
    pack_item.path = pack_relative_path;

    if let Some(existing) = manifest
        .items
        .iter_mut()
        .find(|item| item.id == pack_item.id)
    {
        *existing = pack_item;
    } else {
        manifest.items.push(pack_item);
    }

    let pack_manifest_path = write_pack_manifest(workspace_root, &manifest)?;
    Ok(PackAddSummary {
        asset_reference: format!("pack:{pack_id}/{}", request.id),
        item: prepared.item,
        stored_path: prepared.stored_path,
        pack_manifest_path,
    })
}

pub fn find_asset_catalog_root(start_dir: &Path) -> Option<PathBuf> {
    for ancestor in start_dir.ancestors() {
        if default_registry_path(ancestor).is_file() || ancestor.join(PACKS_ROOT_REL_PATH).is_dir()
        {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub fn load_pack_item_ids(workspace_root: &Path, pack_id: &str) -> Result<BTreeSet<String>> {
    validate_library_id(pack_id)?;
    let manifest = read_pack_manifest(workspace_root, pack_id)?;
    let ids = manifest
        .map(|manifest| {
            manifest
                .items
                .into_iter()
                .map(|item| item.id)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    Ok(ids)
}

fn load_all_pack_manifests(workspace_root: &Path) -> Result<Vec<(String, PackManifest)>> {
    let packs_root = workspace_root.join(PACKS_ROOT_REL_PATH);
    if !packs_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut pack_ids = fs::read_dir(&packs_root)
        .with_context(|| format!("failed to read packs directory {}", packs_root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                Some(entry.file_name().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    pack_ids.sort();

    let mut out = Vec::new();
    for pack_id in pack_ids {
        if let Some(manifest) = read_pack_manifest(workspace_root, &pack_id)? {
            out.push((pack_id, manifest));
        }
    }
    Ok(out)
}

fn read_pack_manifest(workspace_root: &Path, pack_id: &str) -> Result<Option<PackManifest>> {
    let path = pack_manifest_path(workspace_root, pack_id);
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read pack manifest {}", path.display()))?;
    let mut manifest: PackManifest = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse pack manifest JSON {}", path.display()))?;

    if manifest.pack_id.trim().is_empty() {
        manifest.pack_id = pack_id.to_owned();
    }
    if manifest.pack_id != pack_id {
        bail!(
            "pack manifest '{}' declares pack_id '{}' but directory is '{}'",
            path.display(),
            manifest.pack_id,
            pack_id
        );
    }
    normalize_pack_manifest_in_place(&mut manifest)?;
    Ok(Some(manifest))
}

fn write_pack_manifest(workspace_root: &Path, manifest: &PackManifest) -> Result<PathBuf> {
    let mut normalized = manifest.clone();
    normalize_pack_manifest_in_place(&mut normalized)?;

    let path = pack_manifest_path(workspace_root, &normalized.pack_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pack directory {}", parent.display()))?;
    }

    let json =
        serde_json::to_string_pretty(&normalized).context("failed to serialize pack manifest")?;
    fs::write(&path, format!("{json}\n"))
        .with_context(|| format!("failed to write pack manifest {}", path.display()))?;
    Ok(path)
}

fn normalize_pack_manifest_in_place(manifest: &mut PackManifest) -> Result<()> {
    if manifest.version == 0 {
        bail!("pack manifest version must be >= 1");
    }

    validate_library_id(&manifest.pack_id)?;

    manifest.items.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen_ids = BTreeSet::new();
    for item in &mut manifest.items {
        validate_library_id(&item.id)?;
        if !seen_ids.insert(item.id.clone()) {
            bail!(
                "duplicate asset id '{}' in pack '{}'",
                item.id,
                manifest.pack_id
            );
        }

        validate_pack_item_path(&manifest.pack_id, &item.id, &item.path)?;
        if item.sha256.len() != 64 || !item.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!(
                "pack '{}' item '{}' has invalid sha256 '{}': expected 64 lowercase hex chars",
                manifest.pack_id,
                item.id,
                item.sha256
            );
        }

        item.tags.sort();
        item.tags.dedup();
    }

    Ok(())
}

fn validate_pack_item_path(pack_id: &str, item_id: &str, raw_path: &str) -> Result<()> {
    if raw_path.trim().is_empty() {
        bail!("pack '{}' item '{}' path cannot be empty", pack_id, item_id);
    }
    if raw_path.contains('\\') {
        bail!(
            "pack '{}' item '{}' path '{}' must use '/' separators",
            pack_id,
            item_id,
            raw_path
        );
    }

    let path = Path::new(raw_path);
    if path.is_absolute() {
        bail!(
            "pack '{}' item '{}' path must be pack-relative, got absolute path '{}'",
            pack_id,
            item_id,
            raw_path
        );
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "pack '{}' item '{}' path '{}' cannot contain '..'",
            pack_id,
            item_id,
            raw_path
        );
    }

    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(value)) if value == OsStr::new("items") => {}
        _ => {
            bail!(
                "pack '{}' item '{}' path '{}' must start with 'items/{}/'",
                pack_id,
                item_id,
                raw_path,
                item_id
            )
        }
    }

    match components.next() {
        Some(Component::Normal(value)) if value == OsStr::new(item_id) => {}
        _ => {
            bail!(
                "pack '{}' item '{}' path '{}' must start with 'items/{}/'",
                pack_id,
                item_id,
                raw_path,
                item_id
            )
        }
    }

    if components.next().is_none() {
        bail!(
            "pack '{}' item '{}' path '{}' must include a file or subdirectory under 'items/{}/'",
            pack_id,
            item_id,
            raw_path,
            item_id
        );
    }

    Ok(())
}

fn pack_manifest_path(workspace_root: &Path, pack_id: &str) -> PathBuf {
    workspace_root
        .join(PACKS_ROOT_REL_PATH)
        .join(pack_id)
        .join(PACK_MANIFEST_FILE)
}

fn pack_item_workspace_path(pack_id: &str, pack_relative_path: &str) -> Result<String> {
    validate_pack_item_path(
        pack_id,
        infer_item_id_from_path(pack_relative_path)?,
        pack_relative_path,
    )?;
    Ok(Path::new(PACKS_ROOT_REL_PATH)
        .join(pack_id)
        .join(pack_relative_path)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn infer_item_id_from_path(path: &str) -> Result<&str> {
    let mut components = Path::new(path).components();
    let _items = components.next();
    let Some(Component::Normal(item_id)) = components.next() else {
        bail!("invalid pack item path '{}': missing item id segment", path);
    };
    item_id
        .to_str()
        .ok_or_else(|| anyhow!("invalid UTF-8 in pack item path '{}'", path))
}

fn choose_unique_id(base: &str, existing: &BTreeSet<String>) -> String {
    if !existing.contains(base) {
        return base.to_owned();
    }

    let mut index = 2_u32;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        index = index.saturating_add(1);
    }
}

fn default_pack_manifest_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::{parse_asset_reference, parse_pack_reference, suggest_asset_id, AssetReference};
    use std::path::Path;

    #[test]
    fn parses_pack_reference_shape() {
        let parsed = parse_pack_reference("pack:demo-pack/lower-third").expect("should parse");
        assert_eq!(parsed.pack_id, "demo-pack");
        assert_eq!(parsed.asset_id, "lower-third");
        assert!(parse_pack_reference("pack:demo-pack").is_none());
        assert!(parse_pack_reference("pack:/lower-third").is_none());
        assert!(parse_pack_reference("pack:demo-pack/").is_none());
    }

    #[test]
    fn parses_library_or_pack_references() {
        assert!(matches!(
            parse_asset_reference("library:logo"),
            Some(AssetReference::Library("logo"))
        ));
        assert!(matches!(
            parse_asset_reference("pack:brand/logo"),
            Some(AssetReference::Pack(_))
        ));
    }

    #[test]
    fn suggests_kebab_case_asset_ids() {
        let id = suggest_asset_id(Path::new("./My Cool LOGO!.png"));
        assert_eq!(id, "my-cool-logo");

        let id = suggest_asset_id(Path::new("./123.png"));
        assert_eq!(id, "a-123");
    }
}
