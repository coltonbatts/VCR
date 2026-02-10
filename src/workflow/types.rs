use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardData {
    pub figma_file_key: String,
    pub card_node_id: String,
    pub card_name: String,
    pub product_name: String,
    pub price: String,
    pub description: Option<String>,
    pub colors: ProductCardColors,
    pub fonts: ProductCardFonts,
    pub node_ids: ProductCardNodeIds,
    pub asset_urls: ProductCardAssetUrls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardColors {
    pub background: Option<String>,
    pub accent: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardFonts {
    pub product_name: Option<TextStyleSpec>,
    pub price: Option<TextStyleSpec>,
    pub description: Option<TextStyleSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextStyleSpec {
    pub family: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<u32>,
    pub line_height: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardNodeIds {
    pub product_image: String,
    pub product_name: String,
    pub price: String,
    pub description: Option<String>,
    pub background: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductCardAssetUrls {
    pub product_image: String,
    pub product_name: String,
    pub price: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LocalAssetPaths {
    pub product_image: PathBuf,
    pub product_name: PathBuf,
    pub price: PathBuf,
    pub description: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ManifestOutput {
    pub yaml: String,
    pub used_claude: bool,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct RenderOutput {
    pub output_path: PathBuf,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct FrameUploadResult {
    pub uploaded: bool,
    pub link: Option<String>,
    pub note: String,
}
