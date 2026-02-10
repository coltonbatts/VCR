use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use reqwest::Client;

use crate::workflow::types::{LocalAssetPaths, ProductCardData};

pub async fn download_product_card_assets(
    http: &Client,
    data: &ProductCardData,
    run_dir: &Path,
) -> Result<LocalAssetPaths> {
    fs::create_dir_all(run_dir)
        .with_context(|| format!("failed to create run directory {}", run_dir.display()))?;

    let product_image = run_dir.join("product_image.png");
    let product_name = run_dir.join("product_name.png");
    let price = run_dir.join("price.png");
    let description = data
        .asset_urls
        .description
        .as_ref()
        .map(|_| run_dir.join("description.png"));

    download_asset(http, &data.asset_urls.product_image, &product_image).await?;
    download_asset(http, &data.asset_urls.product_name, &product_name).await?;
    download_asset(http, &data.asset_urls.price, &price).await?;

    if let (Some(url), Some(path)) = (&data.asset_urls.description, &description) {
        download_asset(http, url, path).await?;
    }

    Ok(LocalAssetPaths {
        product_image,
        product_name,
        price,
        description,
    })
}

pub fn relative_manifest_path(path: &Path) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "asset path '{}' does not have a valid UTF-8 file name",
                path.display()
            )
        })?;
    Ok(format!("./{file_name}"))
}

async fn download_asset(http: &Client, source_url: &str, destination_path: &PathBuf) -> Result<()> {
    let response = http
        .get(source_url)
        .send()
        .await
        .with_context(|| format!("failed to download asset URL {source_url}"))?
        .error_for_status()
        .with_context(|| format!("asset URL returned an error status: {source_url}"))?;

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read bytes from {source_url}"))?;
    fs::write(destination_path, &bytes).with_context(|| {
        format!(
            "failed to write asset to destination {}",
            destination_path.display()
        )
    })?;
    Ok(())
}
