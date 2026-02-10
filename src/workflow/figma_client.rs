use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::workflow::types::{
    ProductCardAssetUrls, ProductCardColors, ProductCardData, ProductCardFonts, ProductCardNodeIds,
    TextStyleSpec,
};

const FIGMA_API_BASE: &str = "https://api.figma.com/v1";

#[derive(Debug, Clone)]
pub struct FigmaClient {
    http: Client,
    token: String,
}

impl FigmaClient {
    pub fn new(http: Client, token: String) -> Self {
        Self { http, token }
    }

    pub async fn extract_product_card_data(&self, figma_file: &str) -> Result<ProductCardData> {
        let (file_key, hinted_node_id) = parse_figma_file_context(figma_file).ok_or_else(|| {
            anyhow!(
                "could not parse Figma file key from '{}'; provide a valid Figma URL or raw file key",
                figma_file
            )
        })?;

        let file_response: FigmaFileResponse = self
            .http
            .get(format!("{FIGMA_API_BASE}/files/{file_key}"))
            .header("X-Figma-Token", &self.token)
            .send()
            .await
            .context("failed to call Figma files API")?
            .error_for_status()
            .context("Figma files API returned an error status")?
            .json()
            .await
            .context("failed to decode Figma file response")?;

        let search_root = hinted_node_id
            .as_deref()
            .and_then(|id| find_node_by_id(&file_response.document, id))
            .unwrap_or(&file_response.document);
        let card_frame = find_product_card_frame(search_root)
            .or_else(|| find_product_card_heuristic(search_root));

        let card_frame = if let Some(frame) = card_frame {
            frame
        } else {
            find_product_card_frame(&file_response.document)
                .or_else(|| find_product_card_heuristic(&file_response.document))
                .ok_or_else(|| {
                    anyhow!(
                        "could not find a product card candidate. Expected layers named product_image/product_name/price, or a frame with image + price-like text."
                    )
                })?
        };

        let selection = if let Some(strict) = select_layers_strict(card_frame) {
            strict
        } else {
            select_layers_heuristic(card_frame)?
        };

        let product_image = selection.product_image;
        let product_name = selection.product_name;
        let price = selection.price;
        let description = selection.description;
        let background = selection.background;

        let product_name_text = product_name
            .characters
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("layer 'product_name' exists but has no text content"))?;
        let price_text = price
            .characters
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("layer 'price' exists but has no text content"))?;
        let description_text = description.and_then(|node| {
            node.characters
                .clone()
                .and_then(|value| (!value.trim().is_empty()).then_some(value))
        });

        let mut export_node_ids = vec![
            product_image.id.clone(),
            product_name.id.clone(),
            price.id.clone(),
        ];
        if let Some(node) = description {
            export_node_ids.push(node.id.clone());
        }

        let images = self
            .fetch_node_image_urls(&file_key, &export_node_ids)
            .await?;

        let product_image_url = image_url_for(&images, &product_image.id, "product_image")?;
        let product_name_url = image_url_for(&images, &product_name.id, "product_name")?;
        let price_url = image_url_for(&images, &price.id, "price")?;
        let description_url = description
            .map(|node| image_url_for(&images, &node.id, "description"))
            .transpose()?;

        let accent_hex = first_solid_hex(price).or_else(|| first_solid_hex(product_name));
        let text_hex = first_solid_hex(product_name);
        let background_hex = background
            .and_then(first_solid_hex)
            .or_else(|| first_solid_hex(card_frame));

        Ok(ProductCardData {
            figma_file_key: file_key,
            card_node_id: card_frame.id.clone(),
            card_name: card_frame.name.clone(),
            product_name: product_name_text,
            price: price_text,
            description: description_text,
            colors: ProductCardColors {
                background: background_hex,
                accent: accent_hex,
                text: text_hex,
            },
            fonts: ProductCardFonts {
                product_name: product_name.style.as_ref().map(extract_text_style),
                price: price.style.as_ref().map(extract_text_style),
                description: description
                    .and_then(|node| node.style.as_ref().map(extract_text_style)),
            },
            node_ids: ProductCardNodeIds {
                product_image: product_image.id.clone(),
                product_name: product_name.id.clone(),
                price: price.id.clone(),
                description: description.map(|node| node.id.clone()),
                background: background.map(|node| node.id.clone()),
            },
            asset_urls: ProductCardAssetUrls {
                product_image: product_image_url,
                product_name: product_name_url,
                price: price_url,
                description: description_url,
            },
        })
    }

    async fn fetch_node_image_urls(
        &self,
        file_key: &str,
        node_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        if node_ids.is_empty() {
            bail!("no node ids were provided for Figma image export");
        }

        let ids = node_ids.join(",");
        let response: FigmaImageResponse = self
            .http
            .get(format!("{FIGMA_API_BASE}/images/{file_key}"))
            .header("X-Figma-Token", &self.token)
            .query(&[
                ("ids", ids),
                ("format", "png".to_owned()),
                ("scale", "2".to_owned()),
            ])
            .send()
            .await
            .context("failed to call Figma images API")?
            .error_for_status()
            .context("Figma images API returned an error status")?
            .json()
            .await
            .context("failed to decode Figma images response")?;

        if let Some(error) = response.err {
            bail!("Figma images API reported an error: {error}");
        }

        Ok(response.images)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FigmaFileResponse {
    document: FigmaNode,
}

#[derive(Debug, Clone, Deserialize)]
struct FigmaNode {
    id: String,
    name: String,
    #[serde(rename = "type")]
    node_type: String,
    #[serde(default)]
    children: Vec<FigmaNode>,
    #[serde(default)]
    characters: Option<String>,
    #[serde(default)]
    style: Option<FigmaTextStyle>,
    #[serde(default)]
    fills: Vec<FigmaPaint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FigmaTextStyle {
    #[serde(default)]
    font_family: Option<String>,
    #[serde(default)]
    font_size: Option<f32>,
    #[serde(default)]
    font_weight: Option<u32>,
    #[serde(default)]
    line_height_px: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct FigmaPaint {
    #[serde(rename = "type")]
    paint_type: String,
    #[serde(default)]
    visible: Option<bool>,
    #[serde(default)]
    opacity: Option<f32>,
    #[serde(default)]
    color: Option<FigmaColor>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct FigmaColor {
    r: f32,
    g: f32,
    b: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct FigmaImageResponse {
    #[serde(default)]
    err: Option<String>,
    images: HashMap<String, String>,
}

fn find_product_card_frame(node: &FigmaNode) -> Option<&FigmaNode> {
    if node.node_type.eq_ignore_ascii_case("FRAME")
        && has_named_descendant(node, "product_image")
        && has_named_descendant(node, "product_name")
        && has_named_descendant(node, "price")
    {
        return Some(node);
    }

    node.children.iter().find_map(find_product_card_frame)
}

fn find_product_card_heuristic(node: &FigmaNode) -> Option<&FigmaNode> {
    let mut best: Option<(&FigmaNode, i32)> = None;
    visit_nodes(node, &mut |candidate| {
        if !matches!(
            candidate.node_type.as_str(),
            "FRAME" | "GROUP" | "COMPONENT" | "INSTANCE"
        ) {
            return;
        }
        let text_nodes = collect_text_nodes(candidate);
        let price_count = text_nodes
            .iter()
            .filter(|text| text.characters.as_deref().is_some_and(looks_like_price))
            .count();
        if price_count == 0 {
            return;
        }

        let image_nodes = collect_image_fill_nodes(candidate);
        if image_nodes.is_empty() {
            return;
        }

        let score = score_candidate(candidate, text_nodes.len(), image_nodes.len(), price_count);
        if best.as_ref().is_none_or(|(_, current)| score > *current) {
            best = Some((candidate, score));
        }
    });
    best.map(|(node, _)| node)
}

fn score_candidate(
    node: &FigmaNode,
    text_count: usize,
    image_count: usize,
    price_count: usize,
) -> i32 {
    let mut score = 0_i32;
    if node.node_type == "FRAME" {
        score += 20;
    }
    score += (price_count.min(3) as i32) * 25;
    score += (image_count.min(3) as i32) * 15;
    if (2..=20).contains(&text_count) {
        score += 40;
    } else {
        let distance = (text_count as i32 - 8).abs();
        score -= distance.min(40);
    }
    let lower = node.name.to_lowercase();
    if ["card", "product", "pla", "meta", "variant"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        score += 20;
    }
    score
}

fn visit_nodes<'a>(node: &'a FigmaNode, visitor: &mut impl FnMut(&'a FigmaNode)) {
    visitor(node);
    for child in &node.children {
        visit_nodes(child, visitor);
    }
}

fn find_node_by_id<'a>(node: &'a FigmaNode, id: &str) -> Option<&'a FigmaNode> {
    if node.id == id {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node_by_id(child, id))
}

fn find_named_descendant<'a>(node: &'a FigmaNode, target: &str) -> Option<&'a FigmaNode> {
    if node.name.eq_ignore_ascii_case(target) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_named_descendant(child, target))
}

fn has_named_descendant(node: &FigmaNode, target: &str) -> bool {
    find_named_descendant(node, target).is_some()
}

struct SelectedCardNodes<'a> {
    product_image: &'a FigmaNode,
    product_name: &'a FigmaNode,
    price: &'a FigmaNode,
    description: Option<&'a FigmaNode>,
    background: Option<&'a FigmaNode>,
}

fn select_layers_strict(card_frame: &FigmaNode) -> Option<SelectedCardNodes<'_>> {
    let product_image = find_named_descendant(card_frame, "product_image")?;
    let product_name = find_named_descendant(card_frame, "product_name")?;
    let price = find_named_descendant(card_frame, "price")?;
    let description = find_named_descendant(card_frame, "description");
    let background = find_named_descendant(card_frame, "background");
    Some(SelectedCardNodes {
        product_image,
        product_name,
        price,
        description,
        background,
    })
}

fn select_layers_heuristic(card_frame: &FigmaNode) -> Result<SelectedCardNodes<'_>> {
    let text_nodes = collect_text_nodes(card_frame);
    let price = text_nodes
        .iter()
        .copied()
        .find(|node| node.characters.as_deref().is_some_and(looks_like_price))
        .ok_or_else(|| {
            anyhow!("could not infer a price text layer from selected card candidate")
        })?;

    let product_name = text_nodes
        .iter()
        .copied()
        .find(|node| {
            let text = node.characters.as_deref().unwrap_or_default();
            !looks_like_price(text) && looks_like_product_name(text)
        })
        .or_else(|| {
            text_nodes
                .iter()
                .copied()
                .find(|node| !std::ptr::eq(*node, price))
        })
        .ok_or_else(|| anyhow!("could not infer a product name text layer"))?;

    let description = text_nodes.iter().copied().find(|node| {
        !std::ptr::eq(*node, price)
            && !std::ptr::eq(*node, product_name)
            && node
                .characters
                .as_deref()
                .is_some_and(|value| value.trim().len() >= 8)
    });

    let image_nodes = collect_image_fill_nodes(card_frame);
    let product_image = image_nodes
        .first()
        .copied()
        .ok_or_else(|| anyhow!("could not infer an image layer with image fill"))?;

    let background = find_named_descendant(card_frame, "background").or_else(|| {
        collect_rectangles(card_frame).into_iter().find(|node| {
            node.fills
                .iter()
                .any(|fill| fill.paint_type.eq_ignore_ascii_case("SOLID"))
        })
    });

    Ok(SelectedCardNodes {
        product_image,
        product_name,
        price,
        description,
        background,
    })
}

fn collect_text_nodes(node: &FigmaNode) -> Vec<&FigmaNode> {
    let mut out = Vec::new();
    visit_nodes(node, &mut |candidate| {
        if candidate.node_type == "TEXT"
            && candidate
                .characters
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        {
            out.push(candidate);
        }
    });
    out
}

fn collect_image_fill_nodes(node: &FigmaNode) -> Vec<&FigmaNode> {
    let mut out = Vec::new();
    visit_nodes(node, &mut |candidate| {
        let has_image_fill = candidate
            .fills
            .iter()
            .any(|fill| fill.paint_type.eq_ignore_ascii_case("IMAGE"));
        if has_image_fill {
            out.push(candidate);
        }
    });
    out
}

fn collect_rectangles(node: &FigmaNode) -> Vec<&FigmaNode> {
    let mut out = Vec::new();
    visit_nodes(node, &mut |candidate| {
        if candidate.node_type == "RECTANGLE" {
            out.push(candidate);
        }
    });
    out
}

fn looks_like_price(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('$') && trimmed.chars().any(|char| char.is_ascii_digit()) {
        return true;
    }
    let has_decimal = trimmed.contains('.');
    has_decimal
        && trimmed
            .chars()
            .all(|char| char.is_ascii_digit() || matches!(char, '.' | ',' | ' '))
}

fn looks_like_product_name(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 3 || trimmed.len() > 120 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    ![
        "sponsored",
        "merchant",
        "in store",
        "badge",
        "free by",
        "light, rating",
    ]
    .iter()
    .any(|blocked| lower == *blocked)
}

fn image_url_for(images: &HashMap<String, String>, node_id: &str, label: &str) -> Result<String> {
    images
        .get(node_id)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("Figma did not return an exported image URL for layer '{label}'"))
}

fn extract_text_style(style: &FigmaTextStyle) -> TextStyleSpec {
    TextStyleSpec {
        family: style.font_family.clone(),
        size: style.font_size,
        weight: style.font_weight,
        line_height: style.line_height_px,
    }
}

fn first_solid_hex(node: &FigmaNode) -> Option<String> {
    node.fills.iter().find_map(|fill| {
        if !fill.paint_type.eq_ignore_ascii_case("SOLID") {
            return None;
        }
        if fill.visible == Some(false) {
            return None;
        }
        let color = fill.color?;
        let opacity = fill.opacity.unwrap_or(1.0);
        Some(rgb_to_hex(color, opacity))
    })
}

fn rgb_to_hex(color: FigmaColor, opacity: f32) -> String {
    let alpha = opacity.clamp(0.0, 1.0);
    let red = channel_to_u8(color.r);
    let green = channel_to_u8(color.g);
    let blue = channel_to_u8(color.b);
    if (alpha - 1.0).abs() <= f32::EPSILON {
        format!("#{red:02X}{green:02X}{blue:02X}")
    } else {
        let a = channel_to_u8(alpha);
        format!("#{red:02X}{green:02X}{blue:02X}{a:02X}")
    }
}

fn channel_to_u8(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn parse_figma_file_key(input: &str) -> Option<String> {
    parse_figma_file_context(input).map(|(file_key, _)| file_key)
}

fn parse_figma_file_context(input: &str) -> Option<(String, Option<String>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if !trimmed.contains("://") && !trimmed.contains('/') {
        return Some((trimmed.to_owned(), None));
    }

    let url = Url::parse(trimmed).ok()?;
    let path_parts = url.path_segments()?.collect::<Vec<_>>();
    let markers = ["file", "design", "proto"];
    let marker_index = path_parts
        .iter()
        .position(|segment| markers.iter().any(|marker| marker == segment))?;
    let file_key = path_parts
        .get(marker_index + 1)
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())?;

    let node_id = url
        .query_pairs()
        .find_map(|(key, value)| (key == "node-id").then_some(value.to_string()))
        .and_then(normalize_node_id);

    Some((file_key, node_id))
}

fn normalize_node_id(raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains(':') {
        return Some(trimmed.to_owned());
    }
    if trimmed.contains('-') {
        return Some(trimmed.replacen('-', ":", 1));
    }
    Some(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        channel_to_u8, normalize_node_id, parse_figma_file_context, parse_figma_file_key,
        rgb_to_hex, FigmaColor,
    };

    #[test]
    fn parse_figma_file_key_from_design_url() {
        let url = "https://www.figma.com/design/ABC123xyz/Product-Cards?node-id=1-2";
        let key = parse_figma_file_key(url).expect("expected file key");
        assert_eq!(key, "ABC123xyz");
    }

    #[test]
    fn parse_figma_file_key_from_file_url() {
        let url = "https://www.figma.com/file/XYZ987/Product-Cards";
        let key = parse_figma_file_key(url).expect("expected file key");
        assert_eq!(key, "XYZ987");
    }

    #[test]
    fn parse_figma_file_key_from_raw_key() {
        let key = parse_figma_file_key("abc123rawkey").expect("expected file key");
        assert_eq!(key, "abc123rawkey");
    }

    #[test]
    fn parse_figma_context_extracts_node_id() {
        let url = "https://www.figma.com/design/ABC123xyz/Product-Cards?node-id=2-7723&t=abc";
        let (file_key, node_id) = parse_figma_file_context(url).expect("expected file context");
        assert_eq!(file_key, "ABC123xyz");
        assert_eq!(node_id.as_deref(), Some("2:7723"));
    }

    #[test]
    fn normalize_node_id_handles_dash() {
        assert_eq!(
            normalize_node_id("12-44".to_owned()).as_deref(),
            Some("12:44")
        );
    }

    #[test]
    fn rgb_to_hex_encodes_alpha_when_not_opaque() {
        let hex = rgb_to_hex(
            FigmaColor {
                r: 1.0,
                g: 0.0,
                b: 0.5,
            },
            0.5,
        );
        assert_eq!(hex, "#FF008080");
    }

    #[test]
    fn channel_conversion_clamps_range() {
        assert_eq!(channel_to_u8(-0.2), 0);
        assert_eq!(channel_to_u8(1.4), 255);
    }
}
