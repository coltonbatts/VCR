use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::json;

use crate::error_codes::CodedError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AspectPreset {
    Cinema,
    Social,
    Phone,
}

impl AspectPreset {
    pub fn from_keyword(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cinema" => Ok(Self::Cinema),
            "social" => Ok(Self::Social),
            "phone" => Ok(Self::Phone),
            _ => Err(anyhow!(CodedError::usage(
                "INVALID_ASPECT_PRESET",
                format!("invalid aspect preset '{value}'"),
            )
            .with_details(json!({
                "provided": value,
                "allowed": ["cinema", "social", "phone"]
            })))),
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::Cinema => "cinema",
            Self::Social => "social",
            Self::Phone => "phone",
        }
    }

    pub fn dimensions_px(self) -> (u32, u32) {
        match self {
            Self::Cinema => (1920, 1080),
            Self::Social => (1080, 1350),
            Self::Phone => (1080, 1920),
        }
    }

    fn safe_area_percent(self) -> u32 {
        match self {
            Self::Cinema => 5,
            Self::Social => 6,
            Self::Phone => 7,
        }
    }

    /// Integer-only safe-area inset with floor rounding:
    /// inset_px = floor(dimension_px * inset_percent / 100).
    pub fn safe_insets_px(self) -> SafeInsetsPx {
        let (width, height) = self.dimensions_px();
        let pct = self.safe_area_percent();
        SafeInsetsPx {
            left: width.saturating_mul(pct) / 100,
            right: width.saturating_mul(pct) / 100,
            top: height.saturating_mul(pct) / 100,
            bottom: height.saturating_mul(pct) / 100,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SafeInsetsPx {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LetterboxLayout {
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub safe_insets: SafeInsetsPx,
    pub content_window_x: u32,
    pub content_window_y: u32,
    pub content_window_width: u32,
    pub content_window_height: u32,
    pub integer_scale: u32,
    pub scaled_width: u32,
    pub scaled_height: u32,
    pub content_x: u32,
    pub content_y: u32,
    pub padding_left: u32,
    pub padding_right: u32,
    pub padding_top: u32,
    pub padding_bottom: u32,
}

pub fn compute_letterbox_layout(
    aspect: AspectPreset,
    source_width: u32,
    source_height: u32,
) -> Result<LetterboxLayout> {
    if source_width == 0 || source_height == 0 {
        return Err(anyhow!("source dimensions must be > 0"));
    }

    let (canvas_width, canvas_height) = aspect.dimensions_px();
    let safe = aspect.safe_insets_px();
    let content_window_width = canvas_width
        .saturating_sub(safe.left)
        .saturating_sub(safe.right);
    let content_window_height = canvas_height
        .saturating_sub(safe.top)
        .saturating_sub(safe.bottom);
    if content_window_width == 0 || content_window_height == 0 {
        return Err(anyhow!("aspect safe-area leaves no content window"));
    }

    let scale_x = content_window_width / source_width;
    let scale_y = content_window_height / source_height;
    let integer_scale = scale_x.min(scale_y);
    if integer_scale == 0 {
        return Err(anyhow!(
            "grid raster {}x{} does not fit inside {} content window {}x{} without fractional scaling",
            source_width,
            source_height,
            aspect.keyword(),
            content_window_width,
            content_window_height
        ));
    }

    let scaled_width = source_width.saturating_mul(integer_scale);
    let scaled_height = source_height.saturating_mul(integer_scale);
    let rem_x = content_window_width.saturating_sub(scaled_width);
    let rem_y = content_window_height.saturating_sub(scaled_height);

    // Deterministic center tie-break:
    // odd remainder keeps the extra pixel on right/bottom.
    let padding_left = rem_x / 2;
    let padding_right = rem_x - padding_left;
    let padding_top = rem_y / 2;
    let padding_bottom = rem_y - padding_top;

    Ok(LetterboxLayout {
        canvas_width,
        canvas_height,
        safe_insets: safe,
        content_window_x: safe.left,
        content_window_y: safe.top,
        content_window_width,
        content_window_height,
        integer_scale,
        scaled_width,
        scaled_height,
        content_x: safe.left.saturating_add(padding_left),
        content_y: safe.top.saturating_add(padding_top),
        padding_left,
        padding_right,
        padding_top,
        padding_bottom,
    })
}
