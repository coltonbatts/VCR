use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::schema::{Layer, Manifest};

pub fn load_and_validate_manifest(path: &Path) -> Result<Manifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let mut manifest: Manifest = serde_yaml::from_str(&contents)
        .with_context(|| format!("failed to parse yaml in {}", path.display()))?;

    validate_manifest(&mut manifest, path)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &mut Manifest, manifest_path: &Path) -> Result<()> {
    manifest.environment.validate()?;

    if manifest.layers.is_empty() {
        bail!("manifest must define at least one layer");
    }

    let manifest_dir = manifest_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut seen_ids = HashSet::with_capacity(manifest.layers.len());

    for layer in &mut manifest.layers {
        layer.validate()?;

        if !seen_ids.insert(layer.id().to_owned()) {
            bail!("duplicate layer id '{}'", layer.id());
        }

        if let Layer::Asset(asset_layer) = layer {
            let resolved = if asset_layer.source_path.is_absolute() {
                asset_layer.source_path.clone()
            } else {
                manifest_dir.join(&asset_layer.source_path)
            };

            if !resolved.exists() {
                bail!(
                    "layer '{}' source_path does not exist: {}",
                    asset_layer.common.id,
                    resolved.display()
                );
            }

            if !resolved.is_file() {
                bail!(
                    "layer '{}' source_path is not a file: {}",
                    asset_layer.common.id,
                    resolved.display()
                );
            }

            asset_layer.source_path = resolved;
        }
    }

    manifest.layers.sort_by_key(Layer::z_index);
    Ok(())
}
