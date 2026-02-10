use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::workflow::assets::relative_manifest_path;
use crate::workflow::types::{LocalAssetPaths, ManifestOutput, ProductCardData};

const DEFAULT_MODEL: &str = "claude-3-5-sonnet-latest";
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct ManifestGenerator {
    http: Client,
    anthropic_api_key: Option<String>,
    model: String,
}

impl ManifestGenerator {
    pub fn new(http: Client, anthropic_api_key: Option<String>, model: Option<String>) -> Self {
        let selected_model = model
            .and_then(|value| (!value.trim().is_empty()).then_some(value))
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        Self {
            http,
            anthropic_api_key,
            model: selected_model,
        }
    }

    pub async fn generate_manifest(
        &self,
        data: &ProductCardData,
        assets: &LocalAssetPaths,
        user_description: &str,
    ) -> Result<ManifestOutput> {
        if let Some(api_key) = &self.anthropic_api_key {
            match self
                .generate_with_claude(api_key, data, assets, user_description)
                .await
            {
                Ok(yaml) => {
                    validate_manifest_yaml(&yaml)?;
                    return Ok(ManifestOutput {
                        yaml,
                        used_claude: true,
                        note: format!("Generated via Claude model '{}'", self.model),
                    });
                }
                Err(error) => {
                    let fallback = generate_fallback_manifest(data, assets)?;
                    return Ok(ManifestOutput {
                        yaml: fallback,
                        used_claude: false,
                        note: format!(
                            "Claude generation failed ({error:#}); used fallback MVP template"
                        ),
                    });
                }
            }
        }

        let fallback = generate_fallback_manifest(data, assets)?;
        Ok(ManifestOutput {
            yaml: fallback,
            used_claude: false,
            note: "ANTHROPIC_API_KEY not set; used fallback MVP template".to_owned(),
        })
    }

    async fn generate_with_claude(
        &self,
        api_key: &str,
        data: &ProductCardData,
        assets: &LocalAssetPaths,
        user_description: &str,
    ) -> Result<String> {
        let prompt = build_claude_prompt(data, assets, user_description)?;
        let request_body = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 1_400,
            system:
                "You are a VCR manifest expert. Output only valid YAML. Do not use markdown fences."
                    .to_owned(),
            messages: vec![AnthropicMessage {
                role: "user".to_owned(),
                content: prompt,
            }],
        };

        let response: AnthropicResponse = self
            .http
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .json(&request_body)
            .send()
            .await
            .context("failed to call Anthropic API")?
            .error_for_status()
            .context("Anthropic API returned an error status")?
            .json()
            .await
            .context("failed to decode Anthropic response")?;

        let text = response
            .content
            .into_iter()
            .find_map(|chunk| chunk.text)
            .ok_or_else(|| anyhow!("Anthropic response had no text content"))?;
        let yaml = strip_markdown_code_fences(&text);
        if yaml.trim().is_empty() {
            bail!("Anthropic response text was empty");
        }
        Ok(yaml)
    }
}

fn build_claude_prompt(
    data: &ProductCardData,
    assets: &LocalAssetPaths,
    user_description: &str,
) -> Result<String> {
    let figma_json =
        serde_json::to_string_pretty(data).context("failed to serialize extracted Figma JSON")?;
    let asset_json = serde_json::json!({
        "product_image": relative_manifest_path(&assets.product_image)?,
        "product_name": relative_manifest_path(&assets.product_name)?,
        "price": relative_manifest_path(&assets.price)?,
        "description": assets
            .description
            .as_ref()
            .map(|path| relative_manifest_path(path.as_path()))
            .transpose()?,
    });

    Ok(format!(
        "Build a VCR YAML manifest for a product card animation.\n\
         Requirements:\n\
         - Resolution 2560x1440, 24fps, duration 4 seconds (96 frames)\n\
         - Transparent overall background (no full-frame opaque background layer)\n\
         - Product image enters from left with smooth motion\n\
         - Product name, price, and optional description fade in with staggered timing\n\
         - Keep motion organic and readable\n\
         - Use only fields supported by this VCR engine\n\
         - Use these local image assets exactly as paths in image.path\n\
         - Output ONLY YAML with top-level keys: version, environment, params, modulators, groups, layers\n\n\
         User description:\n{user_description}\n\n\
         Extracted Figma data JSON:\n{figma_json}\n\n\
         Local asset paths:\n{}\n",
        serde_json::to_string_pretty(&asset_json)
            .context("failed to serialize local asset path JSON")?
    ))
}

fn strip_markdown_code_fences(input: &str) -> String {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let without_lang = rest
            .split_once('\n')
            .map_or("", |(_, body)| body)
            .trim_end();
        return without_lang.trim_end_matches("```").trim().to_owned();
    }
    trimmed.to_owned()
}

fn generate_fallback_manifest(data: &ProductCardData, assets: &LocalAssetPaths) -> Result<String> {
    let image_path = relative_manifest_path(&assets.product_image)?;
    let name_path = relative_manifest_path(&assets.product_name)?;
    let price_path = relative_manifest_path(&assets.price)?;
    let description_path = assets
        .description
        .as_ref()
        .map(|path| relative_manifest_path(path.as_path()))
        .transpose()?;

    let accent = data
        .colors
        .accent
        .clone()
        .unwrap_or_else(|| "#FFFFFF".to_owned());
    let text_color = data
        .colors
        .text
        .clone()
        .unwrap_or_else(|| "#FFFFFF".to_owned());

    let mut yaml = String::new();
    yaml.push_str("version: 1\n");
    yaml.push_str("environment:\n");
    yaml.push_str("  resolution:\n");
    yaml.push_str("    width: 2560\n");
    yaml.push_str("    height: 1440\n");
    yaml.push_str("  fps: 24\n");
    yaml.push_str("  duration:\n");
    yaml.push_str("    frames: 96\n");
    yaml.push_str("params: {}\n");
    yaml.push_str("modulators: {}\n");
    yaml.push_str("groups: []\n");
    yaml.push_str("layers:\n");
    yaml.push_str("  - id: product_image\n");
    yaml.push_str("    z_index: 1\n");
    yaml.push_str("    pos_x: \"lerp(-920, 180, smoothstep(0, 22, t))\"\n");
    yaml.push_str("    pos_y: 220\n");
    yaml.push_str("    opacity: \"smoothstep(0, 14, t) * (1.0 - smoothstep(86, 96, t))\"\n");
    yaml.push_str("    image:\n");
    yaml.push_str(&format!("      path: \"{image_path}\"\n"));
    yaml.push('\n');
    yaml.push_str("  - id: product_name\n");
    yaml.push_str("    z_index: 2\n");
    yaml.push_str("    pos_x: 1320\n");
    yaml.push_str("    pos_y: 430\n");
    yaml.push_str("    opacity: \"smoothstep(18, 34, t) * (1.0 - smoothstep(86, 96, t))\"\n");
    yaml.push_str("    image:\n");
    yaml.push_str(&format!("      path: \"{name_path}\"\n"));
    yaml.push('\n');
    yaml.push_str("  - id: price\n");
    yaml.push_str("    z_index: 3\n");
    yaml.push_str("    pos_x: 1320\n");
    yaml.push_str("    pos_y: 590\n");
    yaml.push_str("    opacity: \"smoothstep(24, 40, t) * (1.0 - smoothstep(86, 96, t))\"\n");
    yaml.push_str("    image:\n");
    yaml.push_str(&format!("      path: \"{price_path}\"\n"));

    if let Some(path) = description_path {
        yaml.push('\n');
        yaml.push_str("  - id: description\n");
        yaml.push_str("    z_index: 4\n");
        yaml.push_str("    pos_x: 1320\n");
        yaml.push_str("    pos_y: 730\n");
        yaml.push_str("    opacity: \"smoothstep(30, 48, t) * (1.0 - smoothstep(86, 96, t))\"\n");
        yaml.push_str("    image:\n");
        yaml.push_str(&format!("      path: \"{path}\"\n"));
    }

    yaml.push('\n');
    yaml.push_str("# Extracted visual hints from Figma (tracked for downstream automation)\n");
    yaml.push_str(&format!("# accent_color: {accent}\n"));
    yaml.push_str(&format!("# text_color: {text_color}\n"));

    validate_manifest_yaml(&yaml)?;
    Ok(yaml)
}

fn validate_manifest_yaml(yaml: &str) -> Result<()> {
    let parsed: Value = serde_yaml::from_str(yaml).context("manifest is not valid YAML")?;
    let mapping = parsed
        .as_mapping()
        .ok_or_else(|| anyhow!("manifest root must be a YAML object"))?;
    for required in ["version", "environment", "layers"] {
        if !mapping.contains_key(required) {
            bail!("manifest missing required top-level key '{required}'");
        }
    }
    let layers = mapping
        .get("layers")
        .and_then(Value::as_sequence)
        .ok_or_else(|| anyhow!("manifest.layers must be a YAML sequence"))?;
    if layers.is_empty() {
        bail!("manifest.layers must contain at least one layer");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicContent {
    #[serde(default)]
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::workflow::types::{
        LocalAssetPaths, ProductCardAssetUrls, ProductCardColors, ProductCardData,
        ProductCardFonts, ProductCardNodeIds,
    };

    use super::generate_fallback_manifest;

    #[test]
    fn fallback_manifest_is_valid_yaml() {
        let data = ProductCardData {
            figma_file_key: "abc".to_owned(),
            card_node_id: "1:2".to_owned(),
            card_name: "Card-1".to_owned(),
            product_name: "Pink Skirt".to_owned(),
            price: "$29.99".to_owned(),
            description: Some("Limited edition".to_owned()),
            colors: ProductCardColors {
                background: Some("#101010".to_owned()),
                accent: Some("#FF00FF".to_owned()),
                text: Some("#FFFFFF".to_owned()),
            },
            fonts: ProductCardFonts {
                product_name: None,
                price: None,
                description: None,
            },
            node_ids: ProductCardNodeIds {
                product_image: "1:3".to_owned(),
                product_name: "1:4".to_owned(),
                price: "1:5".to_owned(),
                description: Some("1:6".to_owned()),
                background: Some("1:7".to_owned()),
            },
            asset_urls: ProductCardAssetUrls {
                product_image: "https://example.com/image.png".to_owned(),
                product_name: "https://example.com/name.png".to_owned(),
                price: "https://example.com/price.png".to_owned(),
                description: Some("https://example.com/description.png".to_owned()),
            },
        };
        let assets = LocalAssetPaths {
            product_image: PathBuf::from("/tmp/run/product_image.png"),
            product_name: PathBuf::from("/tmp/run/product_name.png"),
            price: PathBuf::from("/tmp/run/price.png"),
            description: Some(PathBuf::from("/tmp/run/description.png")),
        };

        let yaml = generate_fallback_manifest(&data, &assets).expect("fallback manifest");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("valid YAML");
        assert!(parsed.get("layers").is_some());
    }
}
