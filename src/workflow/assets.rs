use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use image::{ImageBuffer, Rgba};
use reqwest::Client;

use crate::workflow::types::{LocalAssetPaths, NodeBounds, ProductCardData, TextStyleSpec};

const FIGMA_EXPORT_SCALE: f32 = 2.0;
const TEXT_PADDING_PX: u32 = 12;
const MIN_TEXT_WIDTH_PX: u32 = 96;
const MIN_TEXT_HEIGHT_PX: u32 = 56;
const SWIFT_TEXT_RENDER_SCRIPT: &str = r##"
import AppKit
import Foundation

func parseColor(_ value: String) -> NSColor {
    let hex = value.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
    var number: UInt64 = 0
    guard Scanner(string: hex).scanHexInt64(&number) else {
        return NSColor.white
    }

    switch hex.count {
    case 6:
        let r = CGFloat((number & 0xFF0000) >> 16) / 255.0
        let g = CGFloat((number & 0x00FF00) >> 8) / 255.0
        let b = CGFloat(number & 0x0000FF) / 255.0
        return NSColor(red: r, green: g, blue: b, alpha: 1.0)
    case 8:
        let r = CGFloat((number & 0xFF000000) >> 24) / 255.0
        let g = CGFloat((number & 0x00FF0000) >> 16) / 255.0
        let b = CGFloat((number & 0x0000FF00) >> 8) / 255.0
        let a = CGFloat(number & 0x000000FF) / 255.0
        return NSColor(red: r, green: g, blue: b, alpha: a)
    default:
        return NSColor.white
    }
}

let args = CommandLine.arguments
guard let marker = args.firstIndex(of: "--"), args.count >= marker + 10 else {
    fputs("invalid renderer arguments\n", stderr)
    exit(2)
}

let textPath = args[marker + 1]
let outputPath = args[marker + 2]
let width = max(Int(args[marker + 3]) ?? 2, 2)
let height = max(Int(args[marker + 4]) ?? 2, 2)
let fontName = args[marker + 5]
let fontSize = max(CGFloat(Double(args[marker + 6]) ?? 24.0), 6.0)
let lineHeight = max(CGFloat(Double(args[marker + 7]) ?? 28.0), 8.0)
let colorHex = args[marker + 8]
let weight = Int(args[marker + 9]) ?? 400

let text = (try? String(contentsOfFile: textPath, encoding: .utf8)) ?? ""

let image = NSImage(size: NSSize(width: width, height: height))
image.lockFocusFlipped(true)

NSColor.clear.setFill()
NSBezierPath(rect: NSRect(x: 0, y: 0, width: width, height: height)).fill()

let fallbackFonts = [fontName, "Geist Pixel", "ArialMT", "Helvetica", "Helvetica Neue"]
var font: NSFont? = nil
for candidate in fallbackFonts {
    if let selected = NSFont(name: candidate, size: fontSize) {
        font = selected
        break
    }
}
if font == nil {
    font = NSFont.systemFont(ofSize: fontSize, weight: weight >= 600 ? .bold : .regular)
}
if weight >= 600, let selected = font {
    font = NSFontManager.shared.convert(selected, toHaveTrait: .boldFontMask)
}

let paragraph = NSMutableParagraphStyle()
paragraph.lineBreakMode = .byWordWrapping
paragraph.minimumLineHeight = lineHeight
paragraph.maximumLineHeight = lineHeight

let attributes: [NSAttributedString.Key: Any] = [
    .font: font!,
    .foregroundColor: parseColor(colorHex),
    .paragraphStyle: paragraph,
]
let attributed = NSAttributedString(string: text, attributes: attributes)
let padding: CGFloat = 12
let drawRect = NSRect(
    x: padding,
    y: padding,
    width: max(CGFloat(width) - padding * 2.0, 1.0),
    height: max(CGFloat(height) - padding * 2.0, 1.0)
)
attributed.draw(with: drawRect, options: [.usesLineFragmentOrigin, .usesFontLeading])

image.unlockFocus()

guard
    let tiff = image.tiffRepresentation,
    let rep = NSBitmapImageRep(data: tiff),
    let png = rep.representation(using: .png, properties: [:])
else {
    fputs("failed creating PNG data\n", stderr)
    exit(3)
}

do {
    try png.write(to: URL(fileURLWithPath: outputPath))
} catch {
    fputs("failed writing PNG: \(error)\n", stderr)
    exit(4)
}
"##;

pub async fn download_product_card_assets(
    http: &Client,
    data: &ProductCardData,
    run_dir: &Path,
    verbose: bool,
) -> Result<LocalAssetPaths> {
    fs::create_dir_all(run_dir)
        .with_context(|| format!("failed to create run directory {}", run_dir.display()))?;

    let product_image = run_dir.join("product_image.png");
    let product_name = run_dir.join("product_name.png");
    let price = run_dir.join("price.png");
    let description = if data.description.is_some() || data.asset_urls.description.is_some() {
        Some(run_dir.join("description.png"))
    } else {
        None
    };

    download_asset(http, &data.asset_urls.product_image, &product_image).await?;

    materialize_text_asset(
        http,
        TextAssetSpec {
            label: "product_name",
            text: &data.product_name,
            style: data.fonts.product_name.as_ref(),
            color: data.colors.text.as_deref(),
            bounds: data.layout.product_name.as_ref(),
            fallback_url: Some(data.asset_urls.product_name.as_str()),
            output_path: &product_name,
            run_dir,
        },
        verbose,
    )
    .await?;

    materialize_text_asset(
        http,
        TextAssetSpec {
            label: "price",
            text: &data.price,
            style: data.fonts.price.as_ref(),
            color: data
                .colors
                .accent
                .as_deref()
                .or(data.colors.text.as_deref()),
            bounds: data.layout.price.as_ref(),
            fallback_url: Some(data.asset_urls.price.as_str()),
            output_path: &price,
            run_dir,
        },
        verbose,
    )
    .await?;

    if let Some(path) = &description {
        let description_text = data.description.as_deref().unwrap_or_default();
        materialize_text_asset(
            http,
            TextAssetSpec {
                label: "description",
                text: description_text,
                style: data.fonts.description.as_ref(),
                color: data.colors.text.as_deref(),
                bounds: data.layout.description.as_ref(),
                fallback_url: data.asset_urls.description.as_deref(),
                output_path: path,
                run_dir,
            },
            verbose,
        )
        .await?;
    }

    Ok(LocalAssetPaths {
        product_image,
        product_name,
        price,
        description,
    })
}

struct TextAssetSpec<'a> {
    label: &'a str,
    text: &'a str,
    style: Option<&'a TextStyleSpec>,
    color: Option<&'a str>,
    bounds: Option<&'a NodeBounds>,
    fallback_url: Option<&'a str>,
    output_path: &'a Path,
    run_dir: &'a Path,
}

async fn materialize_text_asset(
    http: &Client,
    spec: TextAssetSpec<'_>,
    verbose: bool,
) -> Result<()> {
    let rendered = render_text_asset_png(
        spec.text,
        spec.style,
        spec.color,
        spec.bounds,
        spec.output_path,
        spec.label,
        spec.run_dir,
        verbose,
    );

    match rendered {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Some(url) = spec.fallback_url {
                if verbose {
                    eprintln!(
                        "[DEBUG] Text render failed for '{}' ({}); falling back to Figma export URL",
                        spec.label, error
                    );
                }
                download_asset(http, url, &spec.output_path.to_path_buf()).await
            } else {
                Err(error)
            }
        }
    }
}

fn render_text_asset_png(
    text: &str,
    style: Option<&TextStyleSpec>,
    color_hex: Option<&str>,
    bounds: Option<&NodeBounds>,
    destination_path: &Path,
    label: &str,
    run_dir: &Path,
    verbose: bool,
) -> Result<()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        let image = ImageBuffer::from_pixel(2, 2, Rgba([0_u8, 0_u8, 0_u8, 0_u8]));
        image.save(destination_path).with_context(|| {
            format!(
                "failed writing empty text asset {}",
                destination_path.display()
            )
        })?;
        return Ok(());
    }

    let (canvas_width, canvas_height) = target_canvas_size(trimmed, style, bounds);
    let font_family = style
        .and_then(|value| value.family.as_deref())
        .unwrap_or("Arial");
    let font_size =
        style.and_then(|value| value.size).unwrap_or(28.0).max(8.0) * FIGMA_EXPORT_SCALE;
    let line_height = style
        .and_then(|value| value.line_height)
        .unwrap_or((font_size / FIGMA_EXPORT_SCALE) * 1.25)
        .max(10.0)
        * FIGMA_EXPORT_SCALE;
    let weight = style.and_then(|value| value.weight).unwrap_or(400);
    let color = sanitize_color_hex(color_hex).unwrap_or_else(|| "#FFFFFF".to_owned());

    let text_file = run_dir.join(format!("{}__render_input.txt", label));
    fs::write(&text_file, trimmed)
        .with_context(|| format!("failed writing text input file {}", text_file.display()))?;

    if verbose {
        eprintln!(
            "[DEBUG] Rendering '{}' via swift with font='{}' size={:.1} line_height={:.1} canvas={}x{}",
            label, font_family, font_size, line_height, canvas_width, canvas_height
        );
    }

    let swift_cache_dir = run_dir.join(".swift-module-cache");
    fs::create_dir_all(&swift_cache_dir).with_context(|| {
        format!(
            "failed to create swift module cache directory {}",
            swift_cache_dir.display()
        )
    })?;

    let output = Command::new("swift")
        .env("SWIFT_MODULECACHE_PATH", &swift_cache_dir)
        .env("CLANG_MODULE_CACHE_PATH", &swift_cache_dir)
        .arg("-e")
        .arg(SWIFT_TEXT_RENDER_SCRIPT)
        .arg("--")
        .arg(&text_file)
        .arg(destination_path)
        .arg(canvas_width.to_string())
        .arg(canvas_height.to_string())
        .arg(font_family)
        .arg(format!("{font_size:.2}"))
        .arg(format!("{line_height:.2}"))
        .arg(color)
        .arg(weight.to_string())
        .output()
        .with_context(|| "failed launching swift text renderer")?;

    let _ = fs::remove_file(&text_file);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "swift text renderer failed for '{}'. stdout: {} stderr: {}",
            label,
            stdout.trim(),
            stderr.trim()
        );
    }

    Ok(())
}

fn target_canvas_size(
    text: &str,
    style: Option<&TextStyleSpec>,
    bounds: Option<&NodeBounds>,
) -> (u32, u32) {
    if let Some(value) = bounds {
        let width = ((value.width * FIGMA_EXPORT_SCALE).round() as u32).max(MIN_TEXT_WIDTH_PX);
        let mut height =
            ((value.height * FIGMA_EXPORT_SCALE).round() as u32).max(MIN_TEXT_HEIGHT_PX);
        let line_height = style
            .and_then(|data| data.line_height)
            .unwrap_or(style.and_then(|data| data.size).unwrap_or(28.0) * 1.25)
            * FIGMA_EXPORT_SCALE;
        let line_count = text.lines().count().max(1) as f32;
        let required_height = (line_count * line_height).ceil() as u32 + TEXT_PADDING_PX * 2;
        if required_height > height {
            height = required_height;
        }
        return (width + TEXT_PADDING_PX * 2, height + TEXT_PADDING_PX * 2);
    }

    let estimated_font_size =
        style.and_then(|data| data.size).unwrap_or(28.0).max(8.0) * FIGMA_EXPORT_SCALE;
    let estimated_width = ((text.chars().count() as f32 * estimated_font_size * 0.58).ceil()
        as u32)
        .max(MIN_TEXT_WIDTH_PX);
    let estimated_height =
        (estimated_font_size.ceil() as u32 + TEXT_PADDING_PX * 2).max(MIN_TEXT_HEIGHT_PX);
    (estimated_width + TEXT_PADDING_PX * 2, estimated_height)
}

fn sanitize_color_hex(input: Option<&str>) -> Option<String> {
    let raw = input?.trim();
    if raw.is_empty() {
        return None;
    }
    let prefixed = if raw.starts_with('#') {
        raw.to_owned()
    } else {
        format!("#{raw}")
    };
    let len = prefixed.trim_start_matches('#').len();
    if len == 6 || len == 8 {
        return Some(prefixed);
    }
    None
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
