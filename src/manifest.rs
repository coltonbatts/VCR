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

        match layer {
            Layer::Asset(asset_layer) => {
                let resolved = resolve_and_validate_asset_path(
                    &manifest_dir,
                    &asset_layer.source_path,
                    &asset_layer.common.id,
                    "source_path",
                )?;
                asset_layer.source_path = resolved;
            }
            Layer::Image(image_layer) => {
                let resolved = resolve_and_validate_asset_path(
                    &manifest_dir,
                    &image_layer.image.path,
                    &image_layer.common.id,
                    "image.path",
                )?;
                image_layer.image.path = resolved;
            }
            Layer::Procedural(_) => {}
            Layer::Shader(shader_layer) => {
                if let Some(path) = &shader_layer.shader.path {
                    let resolved = resolve_and_validate_asset_path(
                        &manifest_dir,
                        path,
                        &shader_layer.common.id,
                        "shader.path",
                    )?;
                    shader_layer.shader.path = Some(resolved);
                }
            }
            Layer::Text(_) => {}
            Layer::Ascii(ascii_layer) => {
                if let Some(path) = &ascii_layer.ascii.path {
                    let resolved = resolve_and_validate_asset_path(
                        &manifest_dir,
                        path,
                        &ascii_layer.common.id,
                        "ascii.path",
                    )?;
                    ascii_layer.ascii.path = Some(resolved);
                }
                ascii_layer.validate_content_source().with_context(|| {
                    format!("layer '{}': invalid ascii source", ascii_layer.common.id)
                })?;
            }
        }
    }

    manifest.layers.sort_by_key(Layer::z_index);
    Ok(())
}

fn resolve_and_validate_asset_path(
    manifest_dir: &Path,
    source_path: &Path,
    layer_id: &str,
    field_name: &str,
) -> Result<PathBuf> {
    if source_path.is_absolute() {
        bail!(
            "layer '{}' {}: absolute paths are not allowed for security reasons. Use relative paths within the manifest directory. Got: {}",
            layer_id,
            field_name,
            source_path.display()
        );
    }

    let resolved = manifest_dir.join(source_path);

    if !resolved.exists() {
        bail!(
            "layer '{}' {} does not exist: {}",
            layer_id,
            field_name,
            resolved.display()
        );
    }

    // Harden against path traversal: canonicalize both and ensure asset is within manifest dir
    let canonical_manifest_dir = fs::canonicalize(manifest_dir)
        .with_context(|| format!("failed to canonicalize manifest directory {}", manifest_dir.display()))?;
    let canonical_asset_path = fs::canonicalize(&resolved)
        .with_context(|| format!("failed to canonicalize asset path {}", resolved.display()))?;

    if !canonical_asset_path.starts_with(&canonical_manifest_dir) {
        bail!(
            "layer '{}' {}: security violation - asset path '{}' escapes the manifest directory '{}'",
            layer_id,
            field_name,
            source_path.display(),
            manifest_dir.display()
        );
    }

    if !canonical_asset_path.is_file() {
        bail!(
            "layer '{}' {} is not a file: {}",
            layer_id,
            field_name,
            resolved.display()
        );
    }

    Ok(resolved)
}
