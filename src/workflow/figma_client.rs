use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

use crate::workflow::types::{
    NodeBounds, ProductCardAssetUrls, ProductCardColors, ProductCardData, ProductCardFonts,
    ProductCardLayout, ProductCardNodeIds, TextStyleSpec,
};

const FIGMA_API_BASE: &str = "https://api.figma.com/v1";
const PRODUCT_IMAGE_ALIASES: &[&str] = &["product_image", "productimage", "image", "photo"];
const PRODUCT_NAME_ALIASES: &[&str] = &["product_name", "productname", "title", "name"];
const PRICE_ALIASES: &[&str] = &["price", "cost", "amount", "value"];
const DESCRIPTION_ALIASES: &[&str] = &["description", "subtitle", "caption", "url"];
const BACKGROUND_ALIASES: &[&str] = &["background", "bg"];

#[derive(Debug, Clone)]
pub struct FigmaClient {
    http: Client,
    token: String,
    verbose: bool,
}

impl FigmaClient {
    pub fn new(http: Client, token: String, verbose: bool) -> Self {
        Self {
            http,
            token,
            verbose,
        }
    }

    pub async fn extract_product_card_data(
        &self,
        figma_file: &str,
        user_description: &str,
    ) -> Result<ProductCardData> {
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

        if self.verbose {
            eprintln!(
                "[DEBUG] Card candidate: '{}' ({})",
                card_frame.name, card_frame.id
            );
            log_selected_layer("product_image", product_image);
            log_selected_layer("product_name", product_name);
            log_selected_layer("price", price);
            if let Some(node) = description {
                log_selected_layer("description", node);
            }
        }

        let hints = parse_user_description_hints(user_description);

        let product_name_raw = node_text(product_name);
        let price_raw = node_text(price);
        let description_raw = description.and_then(node_text);

        let (product_name_text, product_name_source) = resolve_product_name(
            product_name_raw.as_deref(),
            &hints,
            product_name,
            user_description,
        )?;
        let (price_text, price_source) = resolve_price(price_raw.as_deref(), &hints, price)?;
        let (description_text, description_source) =
            resolve_description(description_raw.as_deref(), description, &hints);

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

        let layout = ProductCardLayout {
            card: extract_node_bounds(card_frame),
            product_image: extract_node_bounds(product_image),
            product_name: extract_node_bounds(product_name),
            price: extract_node_bounds(price),
            description: description.and_then(extract_node_bounds),
        };

        if self.verbose {
            eprintln!("[DEBUG] Extracted text:");
            eprintln!(
                "  - product_name: \"{}\" (source: {})",
                product_name_text, product_name_source
            );
            eprintln!("  - price: \"{}\" (source: {})", price_text, price_source);
            if let Some(value) = &description_text {
                let source = description_source.unwrap_or("figma");
                eprintln!("  - description: \"{}\" (source: {})", value, source);
            }
            if let Some(bounds) = &layout.card {
                eprintln!(
                    "[DEBUG] Card bounds: x={:.1}, y={:.1}, w={:.1}, h={:.1}",
                    bounds.x, bounds.y, bounds.width, bounds.height
                );
            }
        }

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
            layout,
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
    ) -> Result<BTreeMap<String, String>> {
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

fn resolve_product_name(
    extracted: Option<&str>,
    hints: &UserDescriptionHints,
    selected_node: &FigmaNode,
    user_description: &str,
) -> Result<(String, String)> {
    if let Some(raw) = extracted.and_then(validate_text_field) {
        return Ok((
            raw,
            format!(
                "figma layer '{}' ({})",
                selected_node.name, selected_node.id
            ),
        ));
    }
    if let Some(fallback) = hints.product_name.clone() {
        return Ok((fallback, "description parameter fallback".to_owned()));
    }
    if let Some(raw) = extracted.and_then(trimmed_non_empty) {
        return Ok((
            raw,
            format!(
                "figma layer '{}' ({})",
                selected_node.name, selected_node.id
            ),
        ));
    }
    bail!(
        "layer '{}' exists but text extraction failed and description fallback had no product name: '{}'",
        selected_node.name,
        user_description
    )
}

fn resolve_price(
    extracted: Option<&str>,
    hints: &UserDescriptionHints,
    selected_node: &FigmaNode,
) -> Result<(String, String)> {
    if let Some(raw) = extracted.and_then(validate_price_field) {
        return Ok((
            raw,
            format!(
                "figma layer '{}' ({})",
                selected_node.name, selected_node.id
            ),
        ));
    }
    if let Some(fallback) = hints.price.clone() {
        return Ok((fallback, "description parameter fallback".to_owned()));
    }
    if let Some(raw) = extracted.and_then(trimmed_non_empty) {
        return Ok((
            raw,
            format!(
                "figma layer '{}' ({})",
                selected_node.name, selected_node.id
            ),
        ));
    }
    bail!(
        "layer '{}' exists but has no valid price content",
        selected_node.name
    )
}

fn resolve_description(
    extracted: Option<&str>,
    selected_node: Option<&FigmaNode>,
    hints: &UserDescriptionHints,
) -> (Option<String>, Option<&'static str>) {
    if let Some(raw) = extracted.and_then(validate_description_field) {
        return (Some(raw), Some("figma"));
    }
    if let Some(raw) = hints.description.clone() {
        return (Some(raw), Some("description parameter fallback"));
    }
    if selected_node.is_some() {
        return (extracted.and_then(trimmed_non_empty), Some("figma"));
    }
    (None, None)
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
    #[serde(default, rename = "absoluteBoundingBox")]
    absolute_bounding_box: Option<FigmaBoundingBox>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct FigmaBoundingBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
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
    images: BTreeMap<String, String>,
}

fn find_product_card_frame(node: &FigmaNode) -> Option<&FigmaNode> {
    if node.node_type.eq_ignore_ascii_case("FRAME")
        && has_named_descendant(node, PRODUCT_IMAGE_ALIASES)
        && has_named_descendant(node, PRODUCT_NAME_ALIASES)
        && has_named_descendant(node, PRICE_ALIASES)
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

fn has_named_descendant(node: &FigmaNode, aliases: &[&str]) -> bool {
    collect_matching_named_descendants(node, aliases)
        .into_iter()
        .next()
        .is_some()
}

fn collect_matching_named_descendants<'a>(
    node: &'a FigmaNode,
    aliases: &[&str],
) -> Vec<&'a FigmaNode> {
    let mut out = Vec::new();
    visit_nodes(node, &mut |candidate| {
        if layer_alias_score(&candidate.name, aliases) > 0 {
            out.push(candidate);
        }
    });
    out
}

struct SelectedCardNodes<'a> {
    product_image: &'a FigmaNode,
    product_name: &'a FigmaNode,
    price: &'a FigmaNode,
    description: Option<&'a FigmaNode>,
    background: Option<&'a FigmaNode>,
}

fn select_layers_strict(card_frame: &FigmaNode) -> Option<SelectedCardNodes<'_>> {
    let text_nodes = collect_text_nodes(card_frame);

    let product_image = collect_image_fill_nodes(card_frame)
        .into_iter()
        .max_by_key(|node| layer_alias_score(&node.name, PRODUCT_IMAGE_ALIASES))?;

    let price = text_nodes
        .iter()
        .copied()
        .filter(|node| layer_alias_score(&node.name, PRICE_ALIASES) > 0)
        .find(|node| node.characters.as_deref().is_some_and(looks_like_price))?;

    let product_name = text_nodes
        .iter()
        .copied()
        .filter(|node| !std::ptr::eq(*node, price))
        .filter(|node| layer_alias_score(&node.name, PRODUCT_NAME_ALIASES) > 0)
        .max_by_key(|node| {
            let mut score = layer_alias_score(&node.name, PRODUCT_NAME_ALIASES);
            if node
                .characters
                .as_deref()
                .is_some_and(|value| validate_text_field(value).is_some())
            {
                score += 200;
            }
            score
        })?;

    let description = text_nodes
        .iter()
        .copied()
        .filter(|node| !std::ptr::eq(*node, price) && !std::ptr::eq(*node, product_name))
        .filter(|node| layer_alias_score(&node.name, DESCRIPTION_ALIASES) > 0)
        .find(|node| {
            node.characters
                .as_deref()
                .is_some_and(|value| validate_description_field(value).is_some())
        });

    let background = collect_matching_named_descendants(card_frame, BACKGROUND_ALIASES)
        .into_iter()
        .next()
        .or_else(|| {
            collect_rectangles(card_frame).into_iter().find(|node| {
                node.fills
                    .iter()
                    .any(|fill| fill.paint_type.eq_ignore_ascii_case("SOLID"))
            })
        });

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
        .filter(|node| layer_alias_score(&node.name, PRICE_ALIASES) > 0)
        .find(|node| node.characters.as_deref().is_some_and(looks_like_price))
        .or_else(|| {
            text_nodes
                .iter()
                .copied()
                .find(|node| node.characters.as_deref().is_some_and(looks_like_price))
        })
        .or_else(|| {
            text_nodes
                .iter()
                .copied()
                .filter(|node| layer_alias_score(&node.name, PRICE_ALIASES) > 0)
                .max_by_key(|node| {
                    node.style
                        .as_ref()
                        .and_then(|style| style.font_size)
                        .unwrap_or(0.0) as i32
                })
        })
        .ok_or_else(|| {
            anyhow!("could not infer a price text layer from selected card candidate")
        })?;

    let product_name = text_nodes
        .iter()
        .copied()
        .filter(|node| !std::ptr::eq(*node, price))
        .filter(|node| layer_alias_score(&node.name, PRODUCT_NAME_ALIASES) > 0)
        .find(|node| {
            node.characters
                .as_deref()
                .is_some_and(|value| validate_text_field(value).is_some())
        })
        .or_else(|| {
            text_nodes
                .iter()
                .copied()
                .filter(|node| !std::ptr::eq(*node, price))
                .find(|node| {
                    node.characters.as_deref().is_some_and(|text| {
                        validate_text_field(text).is_some() && looks_like_product_name(text)
                    })
                })
        })
        .or_else(|| {
            text_nodes
                .iter()
                .copied()
                .filter(|node| !std::ptr::eq(*node, price))
                .max_by_key(|node| layer_alias_score(&node.name, PRODUCT_NAME_ALIASES))
        })
        .or_else(|| {
            text_nodes
                .iter()
                .copied()
                .find(|node| !std::ptr::eq(*node, price))
        })
        .ok_or_else(|| anyhow!("could not infer a product name text layer"))?;

    let description = text_nodes
        .iter()
        .copied()
        .filter(|node| !std::ptr::eq(*node, price) && !std::ptr::eq(*node, product_name))
        .find(|node| {
            layer_alias_score(&node.name, DESCRIPTION_ALIASES) > 0
                && node
                    .characters
                    .as_deref()
                    .is_some_and(|value| validate_description_field(value).is_some())
        })
        .or_else(|| {
            text_nodes.iter().copied().find(|node| {
                !std::ptr::eq(*node, price)
                    && !std::ptr::eq(*node, product_name)
                    && node.characters.as_deref().is_some_and(|value| {
                        validate_description_field(value).is_some() && value.trim().len() >= 8
                    })
            })
        });

    let image_nodes = collect_image_fill_nodes(card_frame);
    let product_image = image_nodes
        .iter()
        .copied()
        .max_by_key(|node| layer_alias_score(&node.name, PRODUCT_IMAGE_ALIASES))
        .or_else(|| image_nodes.first().copied())
        .ok_or_else(|| anyhow!("could not infer an image layer with image fill"))?;

    let background = collect_matching_named_descendants(card_frame, BACKGROUND_ALIASES)
        .into_iter()
        .next()
        .or_else(|| {
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

fn layer_alias_score(layer_name: &str, aliases: &[&str]) -> i32 {
    let normalized = normalize_layer_name(layer_name);
    aliases
        .iter()
        .map(|alias| normalize_layer_name(alias))
        .map(|alias| {
            if normalized == alias {
                120
            } else if normalized.starts_with(&alias) {
                90
            } else if normalized.contains(&alias) {
                70
            } else {
                0
            }
        })
        .max()
        .unwrap_or_default()
}

fn normalize_layer_name(name: &str) -> String {
    name.chars()
        .filter(|char| char.is_ascii_alphanumeric())
        .flat_map(|char| char.to_lowercase())
        .collect::<String>()
}

fn looks_like_price(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('$') && trimmed.chars().any(|char| char.is_ascii_digit()) {
        return true;
    }
    let has_decimal = trimmed.contains('.') || trimmed.contains(',');
    has_decimal
        && trimmed
            .chars()
            .all(|char| char.is_ascii_digit() || matches!(char, '.' | ',' | ' '))
}

fn validate_price_field(text: &str) -> Option<String> {
    let trimmed = trimmed_non_empty(text)?;
    if looks_like_price(&trimmed) {
        return Some(trimmed);
    }
    None
}

fn validate_text_field(text: &str) -> Option<String> {
    let trimmed = trimmed_non_empty(text)?;
    let lower = trimmed.to_lowercase();

    let blocked_exact = ["result website title two", "no title", "site title"];
    if blocked_exact.iter().any(|value| lower == *value) {
        return None;
    }
    if lower.contains("result website title") {
        return None;
    }
    if lower.starts_with("www.") {
        return None;
    }

    Some(trimmed)
}

fn validate_description_field(text: &str) -> Option<String> {
    let trimmed = trimmed_non_empty(text)?;
    let lower = trimmed.to_lowercase();
    if lower == "site title" || lower == "no title" || lower.starts_with("www.") {
        return None;
    }
    Some(trimmed)
}

fn node_text(node: &FigmaNode) -> Option<String> {
    node.characters.as_deref().and_then(trimmed_non_empty)
}

fn trimmed_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

#[derive(Debug, Default)]
struct UserDescriptionHints {
    product_name: Option<String>,
    price: Option<String>,
    description: Option<String>,
}

fn parse_user_description_hints(input: &str) -> UserDescriptionHints {
    let cleaned = input.trim();
    if cleaned.is_empty() {
        return UserDescriptionHints::default();
    }

    let price = extract_price_from_text(cleaned);

    let mut name_candidate = cleaned.to_owned();
    if let Some((_, rhs)) = name_candidate.split_once(':') {
        name_candidate = rhs.to_owned();
    }

    if let Some(price_value) = &price {
        name_candidate = name_candidate.replace(price_value, " ");
    }

    let segments = name_candidate
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    let product_name = segments
        .iter()
        .find(|segment| {
            let lower = segment.to_lowercase();
            !lower.contains("product card")
                && !lower.contains("price")
                && !looks_like_price(segment)
        })
        .map(|segment| {
            segment
                .trim_matches(|char: char| char == '-' || char == '.')
                .to_string()
        })
        .filter(|value| !value.is_empty());

    let description = if segments.len() >= 2 {
        Some(segments[1..].join(", ")).and_then(|value| validate_description_field(&value))
    } else {
        None
    };

    UserDescriptionHints {
        product_name,
        price,
        description,
    }
}

fn extract_price_from_text(input: &str) -> Option<String> {
    let chars = input.char_indices().collect::<Vec<_>>();
    for (index, current) in &chars {
        if *current != '$' {
            continue;
        }

        let mut end = *index + current.len_utf8();
        for (inner_index, value) in chars.iter().skip_while(|(inner, _)| inner <= index) {
            if value.is_ascii_digit() || matches!(value, '.' | ',') {
                end = *inner_index + value.len_utf8();
            } else {
                break;
            }
        }
        let maybe_price = input[*index..end].trim();
        if maybe_price.chars().any(|char| char.is_ascii_digit()) {
            return Some(maybe_price.to_owned());
        }
    }

    for token in input
        .split(|char: char| char.is_whitespace() || matches!(char, ',' | ';' | ')' | '('))
        .filter(|token| !token.is_empty())
    {
        if looks_like_price(token) {
            return Some(token.to_owned());
        }
    }

    None
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

fn extract_node_bounds(node: &FigmaNode) -> Option<NodeBounds> {
    let bounds = node.absolute_bounding_box?;
    if !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
    {
        return None;
    }
    Some(NodeBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    })
}

fn log_selected_layer(label: &str, node: &FigmaNode) {
    let text = node
        .characters
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("<none>");
    let bounds = node
        .absolute_bounding_box
        .map(|bbox| {
            format!(
                "x={:.1}, y={:.1}, w={:.1}, h={:.1}",
                bbox.x, bbox.y, bbox.width, bbox.height
            )
        })
        .unwrap_or_else(|| "<missing>".to_owned());
    let font = node
        .style
        .as_ref()
        .and_then(|style| style.font_family.clone())
        .unwrap_or_else(|| "<none>".to_owned());
    eprintln!(
        "[DEBUG] Layer {} -> id={} name='{}' text='{}' font='{}' bounds={}",
        label, node.id, node.name, text, font, bounds
    );
}

fn image_url_for(images: &BTreeMap<String, String>, node_id: &str, label: &str) -> Result<String> {
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
        parse_user_description_hints, rgb_to_hex, validate_text_field, FigmaColor,
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

    #[test]
    fn invalid_placeholder_text_is_rejected() {
        assert_eq!(validate_text_field("Result website title two"), None);
        assert_eq!(validate_text_field("No title"), None);
        assert_eq!(validate_text_field("www.example.com"), None);
    }

    #[test]
    fn description_hints_parse_name_and_price() {
        let hints = parse_user_description_hints("product card: pink skirt, $29.99");
        assert_eq!(hints.product_name.as_deref(), Some("pink skirt"));
        assert_eq!(hints.price.as_deref(), Some("$29.99"));
    }
}
