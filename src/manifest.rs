use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::schema::{validate_manifest_manifest_level, Layer, Manifest};

pub fn load_and_validate_manifest(path: &Path) -> Result<Manifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let mut manifest: Manifest = serde_yaml::from_str(&contents).map_err(|error| {
        let location = error
            .location()
            .map(|location| format!("line {}, column {}", location.line(), location.column()))
            .unwrap_or_else(|| "unknown location".to_owned());
        anyhow!(
            "failed to parse yaml in {} at {}: {}",
            path.display(),
            location,
            error
        )
    })?;

    validate_manifest(&mut manifest, path)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &mut Manifest, manifest_path: &Path) -> Result<()> {
    manifest.environment.validate()?;
    validate_manifest_manifest_level(manifest)?;

    if manifest.layers.is_empty() {
        bail!("manifest must define at least one layer");
    }

    let manifest_dir = manifest_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut seen_ids = HashSet::with_capacity(manifest.layers.len());
    let known_groups = manifest
        .groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<HashSet<_>>();

    for layer in &mut manifest.layers {
        layer
            .validate(&manifest.params, manifest.seed, &manifest.modulators)
            .with_context(|| format!("failed validating layer '{}'", layer.id()))?;

        if !seen_ids.insert(layer.id().to_owned()) {
            bail!("duplicate layer id '{}'", layer.id());
        }

        if let Some(group) = layer.common().group.as_deref() {
            if !known_groups.contains(group) {
                bail!(
                    "layer '{}' references unknown group '{}'. Define it in top-level groups",
                    layer.id(),
                    group
                );
            }
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
