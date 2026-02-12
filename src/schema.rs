use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize};

pub type Parameters = BTreeMap<String, f32>;
pub type ModulatorMap = BTreeMap<String, ModulatorDefinition>;

const DEFAULT_MANIFEST_VERSION: u32 = 1;
const DEFAULT_ENV_ATTACK: f32 = 12.0;
const DEFAULT_ENV_DECAY: f32 = 24.0;
const MAX_RESOLUTION: u32 = 8192;
const MAX_FRAME_COUNT: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Float,
    Int,
    Color,
    Vec2,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ParamValue {
    Float(f32),
    Int(i64),
    Color(ColorRgba),
    Vec2(Vec2),
    Bool(bool),
}

impl ParamValue {
    pub fn as_expression_scalar(&self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Int(value) => Some(*value as f32),
            Self::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            Self::Color(_) | Self::Vec2(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamDefinition {
    pub param_type: ParamType,
    pub default: ParamValue,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default = "default_manifest_version")]
    pub version: u32,
    pub environment: Environment,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub params: Parameters,
    #[serde(default)]
    pub modulators: ModulatorMap,
    #[serde(default)]
    pub groups: Vec<Group>,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub post: Vec<PostEffect>,
    #[serde(default)]
    pub ascii_post: Option<AsciiPostConfig>,
    #[serde(skip)]
    pub param_definitions: BTreeMap<String, ParamDefinition>,
    #[serde(skip)]
    pub resolved_params: BTreeMap<String, ParamValue>,
    #[serde(skip)]
    pub applied_param_overrides: BTreeMap<String, ParamValue>,
    #[serde(skip)]
    pub manifest_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub resolution: Resolution,
    pub fps: u32,
    pub duration: Duration,
    #[serde(default)]
    pub color_space: ColorSpace,
}

impl Environment {
    pub fn validate(&self) -> Result<()> {
        if self.resolution.width == 0 || self.resolution.height == 0 {
            bail!(
                "resolution must be positive, got {}x{}",
                self.resolution.width,
                self.resolution.height
            );
        }

        if self.resolution.width > MAX_RESOLUTION || self.resolution.height > MAX_RESOLUTION {
            bail!(
                "resolution exceeds maximum allowed ({}x{}), got {}x{}",
                MAX_RESOLUTION,
                MAX_RESOLUTION,
                self.resolution.width,
                self.resolution.height
            );
        }

        if self.fps == 0 {
            bail!("fps must be > 0");
        }

        match self.duration {
            Duration::Seconds(seconds) => {
                if seconds <= 0.0 {
                    bail!("duration in seconds must be > 0");
                }
            }
            Duration::Frames { frames } => {
                if frames == 0 {
                    bail!("duration frames must be > 0");
                }
                if frames > MAX_FRAME_COUNT {
                    bail!(
                        "duration frames exceeds maximum allowed ({}), got {}",
                        MAX_FRAME_COUNT,
                        frames
                    );
                }
            }
        }

        let total_frames = self.total_frames();
        if total_frames > MAX_FRAME_COUNT {
            bail!(
                "total calculated frames exceeds maximum allowed ({}), got {}",
                MAX_FRAME_COUNT,
                total_frames
            );
        }

        Ok(())
    }

    pub fn total_frames(&self) -> u32 {
        match self.duration {
            Duration::Seconds(seconds) => {
                let frames = (seconds * self.fps as f32).ceil();
                frames.max(1.0) as u32
            }
            Duration::Frames { frames } => frames.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[serde(alias = "rec709", alias = "rec_709")]
    Rec709,
    #[serde(alias = "rec2020", alias = "rec_2020")]
    Rec2020,
    DisplayP3,
}

impl Default for ColorSpace {
    fn default() -> Self {
        Self::Rec709
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
pub enum Duration {
    Seconds(f32),
    Frames { frames: u32 },
}

#[derive(Debug, Clone, Copy)]
pub struct TimingControls {
    pub start_time: Option<f32>,
    pub end_time: Option<f32>,
    pub time_offset: f32,
    pub time_scale: f32,
}

impl Default for TimingControls {
    fn default() -> Self {
        Self {
            start_time: None,
            end_time: None,
            time_offset: 0.0,
            time_scale: 1.0,
        }
    }
}

impl TimingControls {
    pub fn validate(self, label: &str) -> Result<()> {
        if let Some(start_time) = self.start_time {
            if !start_time.is_finite() {
                bail!("{label}.start_time must be finite");
            }
        }
        if let Some(end_time) = self.end_time {
            if !end_time.is_finite() {
                bail!("{label}.end_time must be finite");
            }
        }
        if let (Some(start_time), Some(end_time)) = (self.start_time, self.end_time) {
            if end_time < start_time {
                bail!("{label}.end_time ({end_time}) must be >= {label}.start_time ({start_time})");
            }
        }

        if !self.time_offset.is_finite() {
            bail!("{label}.time_offset must be finite");
        }
        if !self.time_scale.is_finite() || self.time_scale <= 0.0 {
            bail!("{label}.time_scale must be > 0");
        }

        Ok(())
    }

    pub fn remap_frame(self, input_frame: f32, fps: u32) -> Option<f32> {
        let seconds = input_frame / fps as f32;
        if let Some(start_time) = self.start_time {
            if seconds < start_time {
                return None;
            }
        }
        if let Some(end_time) = self.end_time {
            if seconds > end_time {
                return None;
            }
        }

        Some((input_frame + self.time_offset * fps as f32) * self.time_scale)
    }

    pub fn is_default(self) -> bool {
        self.start_time.is_none()
            && self.end_time.is_none()
            && self.time_offset.abs() <= f32::EPSILON
            && (self.time_scale - 1.0).abs() <= f32::EPSILON
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulatorDefinition {
    pub expression: ScalarExpression,
}

impl ModulatorDefinition {
    fn validate(&self, name: &str, params: &Parameters, seed: u64) -> Result<()> {
        let context = ExpressionContext::new(0.0, params, seed);
        let probe = self
            .expression
            .evaluate_with_context(&context)
            .map_err(|error| anyhow!("modulator '{name}': {error}"))?;
        validate_number(&format!("modulator '{name}' expression result"), probe)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ModulatorWeights {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default, alias = "scale_x")]
    pub scale_x: f32,
    #[serde(default, alias = "scale_y")]
    pub scale_y: f32,
    #[serde(default, alias = "rotation_degrees")]
    pub rotation: f32,
    #[serde(default)]
    pub opacity: f32,
}

impl ModulatorWeights {
    pub fn is_zero(self) -> bool {
        self.x.abs() <= f32::EPSILON
            && self.y.abs() <= f32::EPSILON
            && self.scale_x.abs() <= f32::EPSILON
            && self.scale_y.abs() <= f32::EPSILON
            && self.rotation.abs() <= f32::EPSILON
            && self.opacity.abs() <= f32::EPSILON
    }

    pub fn validate(self, label: &str) -> Result<()> {
        for (field, value) in [
            ("x", self.x),
            ("y", self.y),
            ("scale_x", self.scale_x),
            ("scale_y", self.scale_y),
            ("rotation", self.rotation),
            ("opacity", self.opacity),
        ] {
            if !value.is_finite() {
                bail!("{label}.{field} must be finite");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulatorBinding {
    pub source: String,
    #[serde(default)]
    pub weights: ModulatorWeights,
}

impl ModulatorBinding {
    fn validate(&self, label: &str, modulators: &ModulatorMap) -> Result<()> {
        if self.source.trim().is_empty() {
            bail!("{label}.source cannot be empty");
        }

        self.weights.validate(&format!("{label}.weights"))?;
        if self.weights.is_zero() {
            bail!(
                "{label}.weights must include at least one non-zero component (x, y, scale_x, scale_y, rotation, opacity)"
            );
        }

        if !modulators.contains_key(&self.source) {
            bail!(
                "{label}.source '{}' is undefined. Define it in top-level modulators",
                self.source
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub stable_id: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub position: PropertyValue<Vec2>,
    #[serde(default, alias = "position_x")]
    pub pos_x: Option<ScalarProperty>,
    #[serde(default, alias = "position_y")]
    pub pos_y: Option<ScalarProperty>,
    #[serde(default = "default_scale")]
    pub scale: PropertyValue<Vec2>,
    #[serde(default)]
    pub rotation_degrees: ScalarProperty,
    #[serde(default = "default_opacity_property")]
    pub opacity: ScalarProperty,
    #[serde(default)]
    pub start_time: Option<f32>,
    #[serde(default)]
    pub end_time: Option<f32>,
    #[serde(default)]
    pub time_offset: f32,
    #[serde(default = "default_time_scale")]
    pub time_scale: f32,
    #[serde(default)]
    pub modulators: Vec<ModulatorBinding>,
}

impl Group {
    pub fn timing_controls(&self) -> TimingControls {
        TimingControls {
            start_time: self.start_time,
            end_time: self.end_time,
            time_offset: self.time_offset,
            time_scale: self.time_scale,
        }
    }

    pub fn validate(
        &self,
        params: &Parameters,
        seed: u64,
        modulators: &ModulatorMap,
    ) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("group id cannot be empty");
        }
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                bail!("group '{}' name cannot be empty", self.id);
            }
        }
        if let Some(stable_id) = &self.stable_id {
            if stable_id.trim().is_empty() {
                bail!("group '{}' stable_id cannot be empty", self.id);
            }
        }

        self.position
            .validate("position")
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        if let Some(position_x) = &self.pos_x {
            position_x
                .validate_with_context("pos_x", params, seed)
                .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        }
        if let Some(position_y) = &self.pos_y {
            position_y
                .validate_with_context("pos_y", params, seed)
                .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        }
        self.scale
            .validate("scale")
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        self.rotation_degrees
            .validate_with_context("rotation_degrees", params, seed)
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        self.opacity
            .validate_with_context("opacity", params, seed)
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        self.timing_controls()
            .validate("timing")
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;

        for (index, modulator) in self.modulators.iter().enumerate() {
            modulator
                .validate(&format!("modulators[{index}]"), modulators)
                .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        }

        Ok(())
    }

    pub fn has_static_properties(&self) -> bool {
        self.position.is_static()
            && self.pos_x.as_ref().map_or(true, ScalarProperty::is_static)
            && self.pos_y.as_ref().map_or(true, ScalarProperty::is_static)
            && self.scale.is_static()
            && self.rotation_degrees.is_static()
            && self.opacity.is_static()
            && self.modulators.is_empty()
            && self.timing_controls().is_default()
    }

    pub fn sample_position_with_context(
        &self,
        frame: f32,
        context: &ExpressionContext<'_>,
    ) -> Result<Vec2> {
        let mut position = self.position.sample_at(frame);
        if let Some(pos_x) = &self.pos_x {
            position.x = pos_x.evaluate_with_context(&context.with_time(frame))?;
        }
        if let Some(pos_y) = &self.pos_y {
            position.y = pos_y.evaluate_with_context(&context.with_time(frame))?;
        }
        Ok(position)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Center,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerCommon {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub stable_id: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default)]
    pub position: PropertyValue<Vec2>,
    #[serde(default, alias = "position_x")]
    pub pos_x: Option<ScalarProperty>,
    #[serde(default, alias = "position_y")]
    pub pos_y: Option<ScalarProperty>,
    #[serde(default = "default_scale")]
    pub scale: PropertyValue<Vec2>,
    #[serde(default)]
    pub rotation_degrees: ScalarProperty,
    #[serde(default = "default_opacity_property")]
    pub opacity: ScalarProperty,
    #[serde(default)]
    pub start_time: Option<f32>,
    #[serde(default)]
    pub end_time: Option<f32>,
    #[serde(default)]
    pub time_offset: f32,
    #[serde(default = "default_time_scale")]
    pub time_scale: f32,
    #[serde(default)]
    pub modulators: Vec<ModulatorBinding>,
    #[serde(default)]
    pub anchor: Anchor,
}

impl LayerCommon {
    pub fn validate_with_context(
        &self,
        params: &Parameters,
        seed: u64,
        modulators: &ModulatorMap,
    ) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("layer id cannot be empty");
        }
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                bail!("layer '{}' name cannot be empty", self.id);
            }
        }
        if let Some(stable_id) = &self.stable_id {
            if stable_id.trim().is_empty() {
                bail!("layer '{}' stable_id cannot be empty", self.id);
            }
        }

        self.position
            .validate("position")
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        if let Some(position_x) = &self.pos_x {
            position_x
                .validate_with_context("pos_x", params, seed)
                .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        }
        if let Some(position_y) = &self.pos_y {
            position_y
                .validate_with_context("pos_y", params, seed)
                .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        }
        self.scale
            .validate("scale")
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        self.rotation_degrees
            .validate_with_context("rotation_degrees", params, seed)
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        self.opacity
            .validate_with_context("opacity", params, seed)
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        self.timing_controls()
            .validate("timing")
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;

        for (index, modulator) in self.modulators.iter().enumerate() {
            modulator
                .validate(&format!("modulators[{index}]"), modulators)
                .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        }

        Ok(())
    }

    pub fn timing_controls(&self) -> TimingControls {
        TimingControls {
            start_time: self.start_time,
            end_time: self.end_time,
            time_offset: self.time_offset,
            time_scale: self.time_scale,
        }
    }

    pub fn has_static_properties(&self) -> bool {
        self.position.is_static()
            && self.pos_x.as_ref().map_or(true, ScalarProperty::is_static)
            && self.pos_y.as_ref().map_or(true, ScalarProperty::is_static)
            && self.scale.is_static()
            && self.rotation_degrees.is_static()
            && self.opacity.is_static()
            && self.modulators.is_empty()
            && self.timing_controls().is_default()
    }
}

#[derive(Debug, Clone)]
pub enum Layer {
    Asset(AssetLayer),
    Image(ImageLayer),
    Procedural(ProceduralLayer),
    Shader(ShaderLayer),
    Text(TextLayer),
    Ascii(AsciiLayer),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerWire {
    #[serde(flatten)]
    common: LayerCommon,
    #[serde(default)]
    source_path: Option<PathBuf>,
    #[serde(default)]
    image: Option<ImageSource>,
    #[serde(default)]
    procedural: Option<ProceduralSource>,
    #[serde(default)]
    shader: Option<ShaderSource>,
    #[serde(default)]
    text: Option<TextSource>,
    #[serde(default)]
    ascii: Option<AsciiSource>,
}

impl<'de> Deserialize<'de> for Layer {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LayerWire::deserialize(deserializer)?;
        let layer_id = if wire.common.id.trim().is_empty() {
            "<unknown>"
        } else {
            wire.common.id.as_str()
        };

        let mut present_sources = Vec::with_capacity(6);
        if wire.source_path.is_some() {
            present_sources.push("source_path");
        }
        if wire.image.is_some() {
            present_sources.push("image");
        }
        if wire.procedural.is_some() {
            present_sources.push("procedural");
        }
        if wire.shader.is_some() {
            present_sources.push("shader");
        }
        if wire.text.is_some() {
            present_sources.push("text");
        }
        if wire.ascii.is_some() {
            present_sources.push("ascii");
        }

        if present_sources.is_empty() {
            return Err(DeError::custom(format!(
                "layer '{layer_id}' must define exactly one source block: `source_path` (legacy image path), `image`, `procedural`, `shader`, `text`, or `ascii`"
            )));
        }

        if present_sources.len() > 1 {
            return Err(DeError::custom(format!(
                "layer '{layer_id}' defines multiple source blocks ({}) but exactly one is required: `source_path` (legacy image path), `image`, `procedural`, `shader`, `text`, or `ascii`",
                present_sources.join(", ")
            )));
        }

        let LayerWire {
            common,
            source_path,
            image,
            procedural,
            shader,
            text,
            ascii,
        } = wire;

        match (source_path, image, procedural, shader, text, ascii) {
            (Some(source_path), None, None, None, None, None) => Ok(Self::Asset(AssetLayer {
                common,
                source_path,
            })),
            (None, Some(image), None, None, None, None) => {
                Ok(Self::Image(ImageLayer { common, image }))
            }
            (None, None, Some(procedural), None, None, None) => {
                Ok(Self::Procedural(ProceduralLayer { common, procedural }))
            }
            (None, None, None, Some(shader), None, None) => {
                Ok(Self::Shader(ShaderLayer { common, shader }))
            }
            (None, None, None, None, Some(text), None) => {
                Ok(Self::Text(TextLayer { common, text }))
            }
            (None, None, None, None, None, Some(ascii)) => {
                Ok(Self::Ascii(AsciiLayer { common, ascii }))
            }
            _ => Err(DeError::custom(
                "failed to decode layer source; define exactly one source block",
            )),
        }
    }
}

impl Layer {
    pub fn id(&self) -> &str {
        self.common().id.as_str()
    }

    pub fn z_index(&self) -> i32 {
        self.common().z_index
    }

    pub fn common(&self) -> &LayerCommon {
        match self {
            Self::Asset(layer) => &layer.common,
            Self::Image(layer) => &layer.common,
            Self::Procedural(layer) => &layer.common,
            Self::Shader(layer) => &layer.common,
            Self::Text(layer) => &layer.common,
            Self::Ascii(layer) => &layer.common,
        }
    }

    pub fn validate(
        &self,
        params: &Parameters,
        seed: u64,
        modulators: &ModulatorMap,
    ) -> Result<()> {
        self.common()
            .validate_with_context(params, seed, modulators)?;
        match self {
            Self::Asset(layer) => layer.validate(),
            Self::Image(layer) => layer.validate(),
            Self::Procedural(layer) => layer.validate(params, seed),
            Self::Shader(layer) => layer.validate(params, seed),
            Self::Text(layer) => layer.validate(),
            Self::Ascii(layer) => layer.validate(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsciiLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub ascii: AsciiSource,
}

impl AsciiLayer {
    fn validate(&self) -> Result<()> {
        self.ascii.validate_schema(&self.common.id)
    }

    pub fn validate_content_source(&self) -> Result<()> {
        self.ascii
            .compile_base_cells(&self.common.id)
            .map(|_| ())
            .map_err(|error| anyhow!("layer '{}': {error}", self.common.id))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsciiSource {
    pub grid: AsciiGrid,
    pub cell: AsciiCellMetrics,
    pub font_variant: AsciiFontVariant,
    pub foreground: ColorRgba,
    pub background: ColorRgba,
    #[serde(default)]
    pub inline: Option<Vec<String>>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub cells: Vec<AsciiCellOverride>,
    #[serde(default)]
    pub reveal: Option<AsciiReveal>,
}

impl AsciiSource {
    fn validate_schema(&self, layer_id: &str) -> Result<()> {
        if self.grid.rows == 0 || self.grid.columns == 0 {
            bail!("layer '{layer_id}': ascii.grid rows and columns must both be > 0");
        }
        if self.cell.width == 0 || self.cell.height == 0 {
            bail!("layer '{layer_id}': ascii.cell width and height must both be > 0");
        }
        if !self.cell.pixel_aspect_ratio.is_finite() || self.cell.pixel_aspect_ratio <= 0.0 {
            bail!("layer '{layer_id}': ascii.cell.pixel_aspect_ratio must be finite and > 0");
        }
        self.foreground
            .validate("ascii.foreground")
            .map_err(|error| anyhow!("layer '{layer_id}': {error}"))?;
        self.background
            .validate("ascii.background")
            .map_err(|error| anyhow!("layer '{layer_id}': {error}"))?;

        match (&self.inline, &self.path) {
            (Some(_), Some(_)) => {
                bail!("layer '{layer_id}': ascii must set exactly one of inline or path")
            }
            (None, None) => {
                bail!("layer '{layer_id}': ascii must set exactly one of inline or path")
            }
            (None, Some(path)) => {
                if path.as_os_str().is_empty() {
                    bail!("layer '{layer_id}': ascii.path cannot be empty");
                }
            }
            (Some(lines), None) => {
                validate_ascii_rows(
                    lines,
                    self.grid.rows,
                    self.grid.columns,
                    &format!("layer '{layer_id}' ascii.inline"),
                )?;
            }
        }

        for (index, cell) in self.cells.iter().enumerate() {
            cell.validate(
                self.grid,
                &format!("layer '{layer_id}' ascii.cells[{index}]"),
            )?;
        }
        if let Some(reveal) = &self.reveal {
            reveal.validate(&format!("layer '{layer_id}' ascii.reveal"))?;
        }
        Ok(())
    }

    pub fn compile_base_cells(&self, layer_id: &str) -> Result<Vec<u8>> {
        let rows = match (&self.inline, &self.path) {
            (Some(lines), None) => lines.clone(),
            (None, Some(path)) => parse_ascii_file_rows(path)
                .with_context(|| format!("layer '{layer_id}': failed to read ascii.path"))?,
            _ => bail!("layer '{layer_id}': ascii must set exactly one of inline or path"),
        };

        validate_ascii_rows(
            &rows,
            self.grid.rows,
            self.grid.columns,
            &format!("layer '{layer_id}' ascii source"),
        )?;

        let mut compiled = Vec::with_capacity((self.grid.rows * self.grid.columns) as usize);
        for row in rows {
            compiled.extend_from_slice(row.as_bytes());
        }
        Ok(compiled)
    }

    pub fn pixel_dimensions(&self) -> Result<(u32, u32)> {
        let width = self
            .grid
            .columns
            .checked_mul(self.cell.width)
            .ok_or_else(|| anyhow!("ascii grid width overflows u32"))?;
        let height = self
            .grid
            .rows
            .checked_mul(self.cell.height)
            .ok_or_else(|| anyhow!("ascii grid height overflows u32"))?;
        Ok((width, height))
    }

    pub fn is_dynamic(&self) -> bool {
        self.reveal.is_some() || self.cells.iter().any(AsciiCellOverride::is_time_varying)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsciiGrid {
    pub rows: u32,
    pub columns: u32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsciiCellMetrics {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_pixel_aspect_ratio")]
    pub pixel_aspect_ratio: f32,
}

fn default_pixel_aspect_ratio() -> f32 {
    1.0
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsciiFontVariant {
    GeistPixelRegular,
    GeistPixelMedium,
    GeistPixelBold,
    GeistPixelLight,
    GeistPixelMono,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsciiCellOverride {
    pub row: u32,
    pub column: u32,
    #[serde(default)]
    pub character: Option<String>,
    #[serde(default)]
    pub foreground: Option<ColorRgba>,
    #[serde(default)]
    pub background: Option<ColorRgba>,
    #[serde(default)]
    pub visible_from_frame: Option<u32>,
    #[serde(default)]
    pub visible_until_frame: Option<u32>,
}

impl AsciiCellOverride {
    fn validate(&self, grid: AsciiGrid, label: &str) -> Result<()> {
        if self.row >= grid.rows {
            bail!(
                "{label}.row ({}) must be < grid.rows ({})",
                self.row,
                grid.rows
            );
        }
        if self.column >= grid.columns {
            bail!(
                "{label}.column ({}) must be < grid.columns ({})",
                self.column,
                grid.columns
            );
        }

        if let Some(character) = &self.character {
            let byte = parse_single_ascii_character(character, &format!("{label}.character"))?;
            if !is_printable_ascii(byte) {
                bail!(
                    "{label}.character must be printable ASCII (0x20..0x7E), got 0x{:02X}",
                    byte
                );
            }
        }
        if let Some(foreground) = &self.foreground {
            foreground.validate(&format!("{label}.foreground"))?;
        }
        if let Some(background) = &self.background {
            background.validate(&format!("{label}.background"))?;
        }

        if let (Some(start), Some(end)) = (self.visible_from_frame, self.visible_until_frame) {
            if end <= start {
                bail!("{label}.visible_until_frame ({end}) must be > visible_from_frame ({start})");
            }
        }

        Ok(())
    }

    fn is_time_varying(&self) -> bool {
        self.visible_from_frame.is_some() || self.visible_until_frame.is_some()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AsciiReveal {
    RowMajor {
        start_frame: u32,
        frames_per_cell: u32,
        #[serde(default)]
        direction: AsciiRevealDirection,
    },
    ColumnMajor {
        start_frame: u32,
        frames_per_cell: u32,
        #[serde(default)]
        direction: AsciiRevealDirection,
    },
}

impl AsciiReveal {
    fn validate(&self, label: &str) -> Result<()> {
        let frames_per_cell = match self {
            Self::RowMajor {
                frames_per_cell, ..
            }
            | Self::ColumnMajor {
                frames_per_cell, ..
            } => *frames_per_cell,
        };
        if frames_per_cell == 0 {
            bail!("{label}.frames_per_cell must be > 0");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AsciiRevealDirection {
    #[default]
    Forward,
    Reverse,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub text: TextSource,
}

impl TextLayer {
    fn validate(&self) -> Result<()> {
        if self.text.content.is_empty() {
            bail!("layer '{}' text.content cannot be empty", self.common.id);
        }
        self.text.color.validate("text.color")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSource {
    pub content: String,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default = "default_text_color")]
    pub color: ColorRgba,
}

fn default_font_family() -> String {
    "GeistPixel-Line".to_owned()
}

fn default_font_size() -> f32 {
    48.0
}

fn default_text_color() -> ColorRgba {
    ColorRgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub source_path: PathBuf,
}

impl AssetLayer {
    fn validate(&self) -> Result<()> {
        if self.source_path.as_os_str().is_empty() {
            bail!("layer '{}' source_path cannot be empty", self.common.id);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub image: ImageSource,
}

impl ImageLayer {
    fn validate(&self) -> Result<()> {
        if self.image.path.as_os_str().is_empty() {
            bail!("layer '{}' image.path cannot be empty", self.common.id);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSource {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub procedural: ProceduralSource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub shader: ShaderSource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderSource {
    #[serde(default)]
    pub fragment: Option<String>,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub uniforms: BTreeMap<String, ScalarProperty>,
}

impl ShaderLayer {
    fn validate(&self, params: &Parameters, seed: u64) -> Result<()> {
        let label = format!("layer '{}'", self.common.id);
        match (&self.shader.fragment, &self.shader.path) {
            (Some(_), None) | (None, Some(_)) => {}
            (Some(_), Some(_)) => {
                bail!("{label}: shader must have exactly one of fragment or path")
            }
            (None, None) => bail!("{label}: shader must have one of fragment or path"),
        }
        if self.shader.uniforms.len() > 8 {
            bail!("{label}: shader supports at most 8 custom uniforms");
        }
        for (name, prop) in &self.shader.uniforms {
            prop.validate_with_context(&format!("{label}.uniforms.{name}"), params, seed)?;
        }
        Ok(())
    }
}

impl ProceduralLayer {
    fn validate(&self, params: &Parameters, seed: u64) -> Result<()> {
        self.procedural
            .validate(params, seed)
            .map_err(|error| anyhow!("layer '{}': {error}", self.common.id))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralSource {
    SolidColor {
        color: AnimatableColor,
    },
    Gradient {
        start_color: AnimatableColor,
        end_color: AnimatableColor,
        #[serde(default)]
        direction: GradientDirection,
    },
    Triangle {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        color: AnimatableColor,
    },
    Circle {
        center: Vec2,
        radius: ScalarProperty,
        color: AnimatableColor,
    },
    RoundedRect {
        center: Vec2,
        size: Vec2,
        corner_radius: ScalarProperty,
        color: AnimatableColor,
    },
    Ring {
        center: Vec2,
        outer_radius: ScalarProperty,
        inner_radius: ScalarProperty,
        color: AnimatableColor,
    },
    Line {
        start: Vec2,
        end: Vec2,
        thickness: ScalarProperty,
        color: AnimatableColor,
    },
    Polygon {
        center: Vec2,
        radius: ScalarProperty,
        sides: u32,
        color: AnimatableColor,
    },
}

impl ProceduralSource {
    fn validate(&self, params: &Parameters, seed: u64) -> Result<()> {
        match self {
            Self::SolidColor { color } => color.validate("color", params, seed),
            Self::Gradient {
                start_color,
                end_color,
                ..
            } => {
                start_color.validate("start_color", params, seed)?;
                end_color.validate("end_color", params, seed)
            }
            Self::Triangle { color, .. } => color.validate("color", params, seed),
            Self::Circle { radius, color, .. } => {
                radius.validate_with_context("radius", params, seed)?;
                color.validate("color", params, seed)
            }
            Self::RoundedRect {
                corner_radius,
                color,
                ..
            } => {
                corner_radius.validate_with_context("corner_radius", params, seed)?;
                color.validate("color", params, seed)
            }
            Self::Ring {
                outer_radius,
                inner_radius,
                color,
                ..
            } => {
                outer_radius.validate_with_context("outer_radius", params, seed)?;
                inner_radius.validate_with_context("inner_radius", params, seed)?;
                color.validate("color", params, seed)
            }
            Self::Line {
                thickness, color, ..
            } => {
                thickness.validate_with_context("thickness", params, seed)?;
                color.validate("color", params, seed)
            }
            Self::Polygon {
                radius,
                sides,
                color,
                ..
            } => {
                radius.validate_with_context("radius", params, seed)?;
                if *sides < 3 {
                    bail!("polygon sides must be >= 3");
                }
                color.validate("color", params, seed)
            }
        }
    }

    pub fn is_static(&self) -> bool {
        match self {
            Self::SolidColor { color } => color.is_static(),
            Self::Gradient {
                start_color,
                end_color,
                ..
            } => start_color.is_static() && end_color.is_static(),
            Self::Triangle { color, .. } => color.is_static(),
            Self::Circle { radius, color, .. } => radius.is_static() && color.is_static(),
            Self::RoundedRect {
                corner_radius,
                color,
                ..
            } => corner_radius.is_static() && color.is_static(),
            Self::Ring {
                outer_radius,
                inner_radius,
                color,
                ..
            } => outer_radius.is_static() && inner_radius.is_static() && color.is_static(),
            Self::Line {
                thickness, color, ..
            } => thickness.is_static() && color.is_static(),
            Self::Polygon { radius, color, .. } => radius.is_static() && color.is_static(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientDirection {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ColorRgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    #[serde(default = "default_alpha")]
    pub a: f32,
}

impl ColorRgba {
    pub fn as_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn validate(&self, label: &str) -> Result<()> {
        for (channel, value) in [("r", self.r), ("g", self.g), ("b", self.b), ("a", self.a)] {
            if !value.is_finite() {
                bail!("{label}.{channel} must be finite");
            }
        }
        Ok(())
    }
}

fn default_alpha() -> f32 {
    1.0
}

/// Color with animatable r/g/b/a channels. Accepts both static `{r: 0.5, ...}` and
/// expression strings like `{r: "sin(t)", g: 0.5, b: 0, a: 1}`.
#[derive(Debug, Clone)]
pub struct AnimatableColor {
    pub r: ScalarProperty,
    pub g: ScalarProperty,
    pub b: ScalarProperty,
    pub a: ScalarProperty,
}

impl AnimatableColor {
    pub fn evaluate(&self, context: &ExpressionContext<'_>) -> Result<ColorRgba> {
        Ok(ColorRgba {
            r: self.r.evaluate_with_context(context)?,
            g: self.g.evaluate_with_context(context)?,
            b: self.b.evaluate_with_context(context)?,
            a: self.a.evaluate_with_context(context)?,
        })
    }

    pub fn is_static(&self) -> bool {
        self.r.is_static() && self.g.is_static() && self.b.is_static() && self.a.is_static()
    }

    pub fn validate(&self, label: &str, params: &Parameters, seed: u64) -> Result<()> {
        self.r
            .validate_with_context(&format!("{label}.r"), params, seed)?;
        self.g
            .validate_with_context(&format!("{label}.g"), params, seed)?;
        self.b
            .validate_with_context(&format!("{label}.b"), params, seed)?;
        self.a
            .validate_with_context(&format!("{label}.a"), params, seed)?;
        Ok(())
    }
}

impl<'de> Deserialize<'de> for AnimatableColor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ColorFields {
            r: ScalarProperty,
            g: ScalarProperty,
            b: ScalarProperty,
            #[serde(default = "default_alpha_property")]
            a: ScalarProperty,
        }

        fn default_alpha_property() -> ScalarProperty {
            ScalarProperty::Static(1.0)
        }

        let fields = ColorFields::deserialize(deserializer)?;
        Ok(AnimatableColor {
            r: fields.r,
            g: fields.g,
            b: fields.b,
            a: fields.a,
        })
    }
}

impl From<ColorRgba> for AnimatableColor {
    fn from(c: ColorRgba) -> Self {
        AnimatableColor {
            r: ScalarProperty::Static(c.r),
            g: ScalarProperty::Static(c.g),
            b: ScalarProperty::Static(c.b),
            a: ScalarProperty::Static(c.a),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vec2Object {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(untagged)]
enum Vec2Repr {
    Object(Vec2Object),
    Array([f32; 2]),
}

impl<'de> Deserialize<'de> for Vec2 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Vec2Repr::deserialize(deserializer)?;
        let vec = match value {
            Vec2Repr::Object(object) => Self {
                x: object.x,
                y: object.y,
            },
            Vec2Repr::Array([x, y]) => Self { x, y },
        };

        if !vec.x.is_finite() {
            return Err(D::Error::custom("position.x must be finite"));
        }
        if !vec.y.is_finite() {
            return Err(D::Error::custom("position.y must be finite"));
        }

        Ok(vec)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue<T> {
    Static(T),
    Mapping(KeyValue<T>),
}

impl<T: Clone + Interpolate> PropertyValue<T> {
    pub fn sample_at(&self, frame: f32) -> T {
        match self {
            Self::Static(value) => value.clone(),
            Self::Mapping(mapping) => mapping.sample_at(frame),
        }
    }
}

impl<T> PropertyValue<T> {
    pub fn validate(&self, label: &str) -> Result<()> {
        if let Self::Mapping(mapping) = self {
            if mapping.end_frame <= mapping.start_frame {
                bail!(
                    "{label} mapping requires end_frame ({}) > start_frame ({})",
                    mapping.end_frame,
                    mapping.start_frame
                );
            }
        }

        Ok(())
    }

    pub fn is_static(&self) -> bool {
        matches!(self, Self::Static(_))
    }
}

impl Default for PropertyValue<Vec2> {
    fn default() -> Self {
        Self::Static(Vec2::default())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ScalarProperty {
    Static(f32),
    Mapping(KeyValue<f32>),
    Expression(ScalarExpression),
}

impl ScalarProperty {
    pub fn evaluate_with_context(&self, context: &ExpressionContext<'_>) -> Result<f32> {
        match self {
            Self::Static(value) => Ok(*value),
            Self::Mapping(mapping) => Ok(mapping.sample_at(context.t)),
            Self::Expression(expression) => expression.evaluate_with_context(context),
        }
    }

    pub fn validate_with_context(&self, label: &str, params: &Parameters, seed: u64) -> Result<()> {
        match self {
            Self::Static(value) => validate_number(label, *value),
            Self::Mapping(mapping) => {
                if mapping.end_frame <= mapping.start_frame {
                    bail!(
                        "{label} mapping requires end_frame ({}) > start_frame ({})",
                        mapping.end_frame,
                        mapping.start_frame
                    );
                }
                validate_number(&format!("{label}.from"), mapping.from)?;
                validate_number(&format!("{label}.to"), mapping.to)
            }
            Self::Expression(expression) => {
                let context = ExpressionContext::new(0.0, params, seed);
                let probe = expression.evaluate_with_context(&context)?;
                validate_number(label, probe)
            }
        }
    }

    pub fn is_static(&self) -> bool {
        matches!(self, Self::Static(_))
    }
}

impl Default for ScalarProperty {
    fn default() -> Self {
        Self::Static(0.0)
    }
}

#[derive(Debug, Clone)]
pub struct ScalarExpression {
    source: String,
    ast: ExpressionNode,
}

impl ScalarExpression {
    pub fn evaluate_with_context(&self, context: &ExpressionContext<'_>) -> Result<f32> {
        let value = self
            .ast
            .evaluate(context)
            .map_err(|error| anyhow!("invalid expression '{}': {error}", self.source))?;
        validate_number("expression result", value)?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ScalarExpression {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        let ast = ExpressionParser::new(&source)
            .parse()
            .map_err(D::Error::custom)?;
        Ok(Self { source, ast })
    }
}

#[derive(Debug, Clone)]
enum ExpressionNode {
    Constant(f32),
    Variable(String),
    Call {
        name: String,
        args: Vec<ExpressionNode>,
    },
    UnaryNeg(Box<ExpressionNode>),
    Add(Box<ExpressionNode>, Box<ExpressionNode>),
    Sub(Box<ExpressionNode>, Box<ExpressionNode>),
    Mul(Box<ExpressionNode>, Box<ExpressionNode>),
    Div(Box<ExpressionNode>, Box<ExpressionNode>),
    Mod(Box<ExpressionNode>, Box<ExpressionNode>),
    Pow(Box<ExpressionNode>, Box<ExpressionNode>),
}

impl ExpressionNode {
    fn evaluate(&self, context: &ExpressionContext<'_>) -> Result<f32> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Variable(identifier) => match identifier.as_str() {
                "t" => Ok(context.t),
                _ => context
                    .params
                    .get(identifier)
                    .copied()
                    .ok_or_else(|| anyhow!("unknown variable '{identifier}'")),
            },
            Self::Call { name, args } => evaluate_function(name, args, context),
            Self::UnaryNeg(value) => Ok(-value.evaluate(context)?),
            Self::Add(left, right) => Ok(left.evaluate(context)? + right.evaluate(context)?),
            Self::Sub(left, right) => Ok(left.evaluate(context)? - right.evaluate(context)?),
            Self::Mul(left, right) => Ok(left.evaluate(context)? * right.evaluate(context)?),
            Self::Div(left, right) => {
                let divisor = right.evaluate(context)?;
                if divisor.abs() <= f32::EPSILON {
                    bail!("expression attempted division by zero");
                }
                Ok(left.evaluate(context)? / divisor)
            }
            Self::Mod(left, right) => {
                let divisor = right.evaluate(context)?;
                if divisor.abs() <= f32::EPSILON {
                    bail!("expression attempted modulo by zero");
                }
                Ok(left.evaluate(context)? % divisor)
            }
            Self::Pow(left, right) => {
                let value = left.evaluate(context)?.powf(right.evaluate(context)?);
                validate_number("pow result", value)?;
                Ok(value)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExpressionContext<'a> {
    pub t: f32,
    pub params: &'a Parameters,
    pub seed: u64,
}

impl<'a> ExpressionContext<'a> {
    pub fn new(t: f32, params: &'a Parameters, seed: u64) -> Self {
        Self { t, params, seed }
    }

    pub fn with_time(self, t: f32) -> Self {
        Self { t, ..self }
    }
}

struct ExpressionParser<'a> {
    source: &'a str,
    bytes: &'a [u8],
    index: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            index: 0,
        }
    }

    fn parse(mut self) -> Result<ExpressionNode> {
        let expression = self.parse_add_sub()?;
        self.skip_whitespace();
        if self.index != self.bytes.len() {
            bail!(
                "unexpected token '{}' at position {}",
                self.peek_char().unwrap_or('?'),
                self.index
            );
        }
        Ok(expression)
    }

    fn parse_add_sub(&mut self) -> Result<ExpressionNode> {
        let mut node = self.parse_mul_div_mod()?;
        loop {
            self.skip_whitespace();
            match self.peek_char() {
                Some('+') => {
                    self.index += 1;
                    let right = self.parse_mul_div_mod()?;
                    node = ExpressionNode::Add(Box::new(node), Box::new(right));
                }
                Some('-') => {
                    self.index += 1;
                    let right = self.parse_mul_div_mod()?;
                    node = ExpressionNode::Sub(Box::new(node), Box::new(right));
                }
                _ => return Ok(node),
            }
        }
    }

    fn parse_mul_div_mod(&mut self) -> Result<ExpressionNode> {
        let mut node = self.parse_power()?;
        loop {
            self.skip_whitespace();
            match self.peek_char() {
                Some('*') => {
                    self.index += 1;
                    let right = self.parse_power()?;
                    node = ExpressionNode::Mul(Box::new(node), Box::new(right));
                }
                Some('/') => {
                    self.index += 1;
                    let right = self.parse_power()?;
                    node = ExpressionNode::Div(Box::new(node), Box::new(right));
                }
                Some('%') => {
                    self.index += 1;
                    let right = self.parse_power()?;
                    node = ExpressionNode::Mod(Box::new(node), Box::new(right));
                }
                _ => return Ok(node),
            }
        }
    }

    fn parse_power(&mut self) -> Result<ExpressionNode> {
        let left = self.parse_unary()?;
        self.skip_whitespace();
        if self.peek_char() == Some('^') {
            self.index += 1;
            let right = self.parse_power()?;
            Ok(ExpressionNode::Pow(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_unary(&mut self) -> Result<ExpressionNode> {
        self.skip_whitespace();
        match self.peek_char() {
            Some('+') => {
                self.index += 1;
                self.parse_unary()
            }
            Some('-') => {
                self.index += 1;
                Ok(ExpressionNode::UnaryNeg(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<ExpressionNode> {
        self.skip_whitespace();
        match self.peek_char() {
            Some('(') => {
                self.index += 1;
                let expression = self.parse_add_sub()?;
                self.skip_whitespace();
                if self.peek_char() != Some(')') {
                    bail!("expected ')' at position {}", self.index);
                }
                self.index += 1;
                Ok(expression)
            }
            Some('0'..='9') | Some('.') => self.parse_number(),
            Some('a'..='z') | Some('A'..='Z') | Some('_') => self.parse_identifier_or_call(),
            Some(token) => bail!("unexpected token '{token}' at position {}", self.index),
            None => bail!("unexpected end of expression"),
        }
    }

    fn parse_number(&mut self) -> Result<ExpressionNode> {
        let start = self.index;

        while matches!(self.peek_char(), Some('0'..='9')) {
            self.index += 1;
        }

        if self.peek_char() == Some('.') {
            self.index += 1;
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.index += 1;
            }
        }

        if matches!(self.peek_char(), Some('e') | Some('E')) {
            self.index += 1;
            if matches!(self.peek_char(), Some('+') | Some('-')) {
                self.index += 1;
            }
            let exponent_start = self.index;
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.index += 1;
            }
            if exponent_start == self.index {
                bail!("invalid exponent at position {}", self.index);
            }
        }

        let token = &self.source[start..self.index];
        let value = token
            .parse::<f32>()
            .map_err(|error| anyhow!("invalid number '{token}': {error}"))?;
        validate_number("number literal", value)?;
        Ok(ExpressionNode::Constant(value))
    }

    fn parse_identifier_or_call(&mut self) -> Result<ExpressionNode> {
        let start = self.index;
        while matches!(
            self.peek_char(),
            Some('a'..='z') | Some('A'..='Z') | Some('_') | Some('0'..='9')
        ) {
            self.index += 1;
        }
        let identifier = &self.source[start..self.index];

        self.skip_whitespace();
        if self.peek_char() != Some('(') {
            return Ok(ExpressionNode::Variable(identifier.to_owned()));
        }

        self.index += 1;
        let mut args = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek_char() == Some(')') {
                self.index += 1;
                break;
            }

            args.push(self.parse_add_sub()?);
            self.skip_whitespace();
            match self.peek_char() {
                Some(',') => {
                    self.index += 1;
                }
                Some(')') => {
                    self.index += 1;
                    break;
                }
                Some(token) => {
                    bail!(
                        "expected ',' or ')' after function argument, found '{}' at position {}",
                        token,
                        self.index
                    );
                }
                None => bail!("unterminated function call for '{identifier}'"),
            }
        }

        Ok(ExpressionNode::Call {
            name: identifier.to_owned(),
            args,
        })
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_char(), Some(' ' | '\t' | '\n' | '\r')) {
            self.index += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.bytes.get(self.index).map(|byte| *byte as char)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyValue<T> {
    pub start_frame: u32,
    pub end_frame: u32,
    pub from: T,
    pub to: T,
    #[serde(default)]
    pub easing: EasingCurve,
}

impl<T: Clone + Interpolate> KeyValue<T> {
    pub fn sample_at(&self, frame: f32) -> T {
        let start_frame = self.start_frame as f32;
        let end_frame = self.end_frame as f32;
        if frame <= start_frame {
            return self.from.clone();
        }
        if frame >= end_frame {
            return self.to.clone();
        }

        let span = end_frame - start_frame;
        let progress = (frame - start_frame) / span;
        let eased = self.easing.apply(progress.clamp(0.0, 1.0));
        T::interpolate(&self.from, &self.to, eased)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EasingCurve {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl EasingCurve {
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - ((-2.0 * t + 2.0).powi(2) / 2.0)
                }
            }
        }
    }
}

pub trait Interpolate {
    fn interpolate(from: &Self, to: &Self, t: f32) -> Self;
}

impl Interpolate for f32 {
    fn interpolate(from: &Self, to: &Self, t: f32) -> Self {
        *from + (*to - *from) * t
    }
}

impl Interpolate for Vec2 {
    fn interpolate(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            x: <f32 as Interpolate>::interpolate(&from.x, &to.x, t),
            y: <f32 as Interpolate>::interpolate(&from.y, &to.y, t),
        }
    }
}

pub fn validate_manifest_manifest_level(manifest: &Manifest) -> Result<()> {
    if manifest.version != DEFAULT_MANIFEST_VERSION {
        bail!(
            "unsupported manifest version {} (expected {}). Add a migration or set version: {}",
            manifest.version,
            DEFAULT_MANIFEST_VERSION,
            DEFAULT_MANIFEST_VERSION
        );
    }

    for (name, value) in &manifest.params {
        if !valid_identifier(name) {
            bail!("invalid param name '{name}'. Use identifiers like energy, phase, tension_2");
        }
        if name == "t" {
            bail!("param name 't' is reserved for frame time in expressions");
        }
        validate_number(&format!("param '{name}'"), *value)?;
    }

    for (name, modulator) in &manifest.modulators {
        if !valid_identifier(name) {
            bail!("invalid modulator name '{name}'. Use identifiers like wobble or pulse_1");
        }
        modulator.validate(name, &manifest.params, manifest.seed)?;
    }

    let mut seen_group_ids = HashSet::with_capacity(manifest.groups.len());
    for group in &manifest.groups {
        group.validate(&manifest.params, manifest.seed, &manifest.modulators)?;
        if !seen_group_ids.insert(group.id.as_str()) {
            bail!("duplicate group id '{}'", group.id);
        }
    }

    for group in &manifest.groups {
        if let Some(parent) = &group.parent {
            if !seen_group_ids.contains(parent.as_str()) {
                bail!(
                    "group '{}' references unknown parent '{}'. Define the parent group first",
                    group.id,
                    parent
                );
            }
        }
    }

    for group in &manifest.groups {
        let mut seen = HashSet::new();
        seen.insert(group.id.as_str());
        let mut current = group.parent.as_deref();
        while let Some(parent_id) = current {
            if !seen.insert(parent_id) {
                bail!(
                    "group '{}' has a cyclic parent chain involving '{}'",
                    group.id,
                    parent_id
                );
            }

            current = manifest
                .groups
                .iter()
                .find(|candidate| candidate.id == parent_id)
                .and_then(|candidate| candidate.parent.as_deref());
        }
    }

    if let Some(ascii_post) = &manifest.ascii_post {
        ascii_post.validate()?;
    }

    Ok(())
}

fn evaluate_function(
    name: &str,
    args: &[ExpressionNode],
    context: &ExpressionContext<'_>,
) -> Result<f32> {
    let evaluated = args
        .iter()
        .map(|arg| arg.evaluate(context))
        .collect::<Result<Vec<_>>>()?;

    let normalized = normalize_identifier(name);
    match normalized.as_str() {
        "clamp" => {
            expect_arity(name, &evaluated, 3)?;
            let min = evaluated[1];
            let max = evaluated[2];
            if min > max {
                bail!("function {name} requires min <= max");
            }
            Ok(evaluated[0].clamp(min, max))
        }
        "lerp" => {
            expect_arity(name, &evaluated, 3)?;
            Ok(evaluated[0] + (evaluated[1] - evaluated[0]) * evaluated[2])
        }
        "smoothstep" => {
            expect_arity(name, &evaluated, 3)?;
            let edge_0 = evaluated[0];
            let edge_1 = evaluated[1];
            let x = evaluated[2];
            if (edge_1 - edge_0).abs() <= f32::EPSILON {
                bail!("function {name} requires edge0 and edge1 to differ");
            }
            let t = ((x - edge_0) / (edge_1 - edge_0)).clamp(0.0, 1.0);
            Ok(t * t * (3.0 - 2.0 * t))
        }
        "easeinout" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(EasingCurve::EaseInOut.apply(evaluated[0].clamp(0.0, 1.0)))
        }
        "step" => {
            expect_arity(name, &evaluated, 2)?;
            Ok(if evaluated[1] >= evaluated[0] {
                1.0
            } else {
                0.0
            })
        }
        "fract" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0] - evaluated[0].floor())
        }
        "floor" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0].floor())
        }
        "ceil" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0].ceil())
        }
        "round" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0].round())
        }
        "saw" => {
            if evaluated.is_empty() || evaluated.len() > 2 {
                bail!("function {name} expects 1 or 2 arguments");
            }
            let frequency = evaluated.get(1).copied().unwrap_or(1.0);
            let t = evaluated[0] * frequency;
            Ok(t - t.floor())
        }
        "tri" => {
            if evaluated.is_empty() || evaluated.len() > 2 {
                bail!("function {name} expects 1 or 2 arguments");
            }
            let frequency = evaluated.get(1).copied().unwrap_or(1.0);
            let t = evaluated[0] * frequency;
            Ok(2.0 * (t - (t + 0.5).floor()).abs())
        }
        "random" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(hash_to_unit_range(evaluated[0] as i64, context.seed))
        }
        "glitch" => {
            if evaluated.is_empty() || evaluated.len() > 2 {
                bail!("function {name} expects 1 or 2 arguments");
            }
            let t = evaluated[0];
            let intensity = evaluated.get(1).copied().unwrap_or(1.0);
            let n = noise_1d(t * 10.0, context.seed);
            if n > 0.8 / intensity.max(0.1) {
                Ok(noise_1d(t * 100.0, context.seed.wrapping_add(1)))
            } else {
                Ok(0.0)
            }
        }
        "sin" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0].sin())
        }
        "cos" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0].cos())
        }
        "abs" => {
            expect_arity(name, &evaluated, 1)?;
            Ok(evaluated[0].abs())
        }
        "noise1d" => {
            if evaluated.is_empty() || evaluated.len() > 2 {
                bail!("function {name} expects 1 or 2 arguments");
            }
            let x = evaluated[0];
            let seed_offset = evaluated.get(1).copied().unwrap_or(0.0).round() as i64;
            Ok(noise_1d(x, context.seed.wrapping_add(seed_offset as u64)))
        }
        "env" => {
            if evaluated.len() != 1 && evaluated.len() != 3 {
                bail!("function {name} expects 1 or 3 arguments");
            }
            let time = evaluated[0];
            let attack = evaluated.get(1).copied().unwrap_or(DEFAULT_ENV_ATTACK);
            let decay = evaluated.get(2).copied().unwrap_or(DEFAULT_ENV_DECAY);
            envelope(time, attack, decay)
        }
        _ => bail!("unsupported function '{name}'"),
    }
}

fn expect_arity(name: &str, args: &[f32], expected: usize) -> Result<()> {
    if args.len() != expected {
        bail!(
            "function {name} expects {expected} argument(s), got {}",
            args.len()
        );
    }
    Ok(())
}

fn noise_1d(x: f32, seed: u64) -> f32 {
    let x0 = x.floor() as i64;
    let x1 = x0 + 1;
    let frac = x - x.floor();
    let smooth = frac * frac * (3.0 - 2.0 * frac);
    let a = hash_to_unit_range(x0, seed);
    let b = hash_to_unit_range(x1, seed);
    (a + (b - a) * smooth) * 2.0 - 1.0
}

fn hash_to_unit_range(x: i64, seed: u64) -> f32 {
    let mut value = (x as u64).wrapping_add(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    (value as f64 / u64::MAX as f64) as f32
}

fn envelope(time: f32, attack: f32, decay: f32) -> Result<f32> {
    if !attack.is_finite() || attack <= 0.0 {
        bail!("env attack must be finite and > 0");
    }
    if !decay.is_finite() || decay <= 0.0 {
        bail!("env decay must be finite and > 0");
    }

    if time <= 0.0 {
        return Ok(0.0);
    }
    if time < attack {
        return Ok((time / attack).clamp(0.0, 1.0));
    }

    let decay_progress = (time - attack) / decay;
    Ok((1.0 - decay_progress).clamp(0.0, 1.0))
}

fn normalize_identifier(identifier: &str) -> String {
    identifier
        .chars()
        .filter(|character| *character != '_')
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn valid_identifier(identifier: &str) -> bool {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn default_manifest_version() -> u32 {
    DEFAULT_MANIFEST_VERSION
}

fn default_scale() -> PropertyValue<Vec2> {
    PropertyValue::Static(Vec2 { x: 1.0, y: 1.0 })
}

fn default_opacity_property() -> ScalarProperty {
    ScalarProperty::Static(1.0)
}

fn default_time_scale() -> f32 {
    1.0
}

fn parse_ascii_file_rows(path: &PathBuf) -> Result<Vec<String>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut rows = normalized
        .split('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    Ok(rows)
}

fn validate_ascii_rows(
    rows: &[String],
    expected_rows: u32,
    expected_columns: u32,
    label: &str,
) -> Result<()> {
    if rows.len() != expected_rows as usize {
        bail!(
            "{label} must have exactly {} row(s), got {}",
            expected_rows,
            rows.len()
        );
    }

    for (row_index, row) in rows.iter().enumerate() {
        let bytes = row.as_bytes();
        if bytes.len() != expected_columns as usize {
            bail!(
                "{label} row {} must have exactly {} columns, got {}",
                row_index,
                expected_columns,
                bytes.len()
            );
        }

        for (column_index, byte) in bytes.iter().enumerate() {
            if !is_printable_ascii(*byte) {
                bail!(
                    "{label} row {} column {} is not printable ASCII (0x20..0x7E): 0x{:02X}",
                    row_index,
                    column_index,
                    byte
                );
            }
        }
    }
    Ok(())
}

fn parse_single_ascii_character(value: &str, label: &str) -> Result<u8> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 {
        bail!("{label} must be exactly one ASCII character");
    }
    Ok(bytes[0])
}

fn is_printable_ascii(byte: u8) -> bool {
    (0x20..=0x7E).contains(&byte)
}

fn validate_number(label: &str, value: f32) -> Result<()> {
    if !value.is_finite() {
        bail!("{label} must be finite");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Post-processing effect schema
// ---------------------------------------------------------------------------

/// A single post-processing effect entry in the manifest `post:` array.
#[derive(Debug, Clone, Deserialize)]
pub struct PostEffect {
    #[serde(flatten)]
    pub kind: PostEffectKind,
}

/// Discriminated union of supported post-processing shaders.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "shader", rename_all = "snake_case")]
pub enum PostEffectKind {
    Passthrough,
    Levels {
        #[serde(default = "default_gamma")]
        gamma: f32,
        #[serde(default)]
        lift: f32,
        #[serde(default = "default_gain")]
        gain: f32,
    },
    Sobel {
        #[serde(default = "default_sobel_strength")]
        strength: f32,
    },
}

fn default_gamma() -> f32 {
    1.0
}
fn default_gain() -> f32 {
    1.0
}
fn default_sobel_strength() -> f32 {
    1.0
}

// ---------------------------------------------------------------------------
// ASCII post-processing pipeline configuration
// ---------------------------------------------------------------------------

const DEFAULT_ASCII_POST_RAMP: &str = " .:-=+*#%@";

/// Configuration for the GPU-based ASCII post-processing pipeline.
///
/// When enabled, the composited frame is analyzed at terminal-cell resolution,
/// luminance is mapped to glyph indices, and a debug grayscale visualization
/// is rendered as the final output.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsciiPostConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ascii_post_cols")]
    pub cols: u32,
    #[serde(default = "default_ascii_post_rows")]
    pub rows: u32,
    #[serde(default = "default_ascii_post_ramp")]
    pub ramp: String,
}

impl Default for AsciiPostConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cols: 120,
            rows: 45,
            ramp: DEFAULT_ASCII_POST_RAMP.to_owned(),
        }
    }
}

impl AsciiPostConfig {
    /// Validate the ASCII post-processing configuration.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.cols == 0 {
            bail!("ascii_post.cols must be > 0");
        }
        if self.rows == 0 {
            bail!("ascii_post.rows must be > 0");
        }
        let ramp_chars: Vec<char> = self.ramp.chars().collect();
        if ramp_chars.len() < 2 {
            bail!(
                "ascii_post.ramp must have at least 2 characters, got {}",
                ramp_chars.len()
            );
        }
        Ok(())
    }

    /// Number of distinct glyphs in the ramp.
    pub fn ramp_len(&self) -> usize {
        self.ramp.chars().count()
    }
}

fn default_ascii_post_cols() -> u32 {
    120
}
fn default_ascii_post_rows() -> u32 {
    45
}
fn default_ascii_post_ramp() -> String {
    DEFAULT_ASCII_POST_RAMP.to_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        default_manifest_version, validate_manifest_manifest_level, ExpressionContext, Layer,
        Manifest, PostEffectKind, ScalarExpression, ScalarProperty,
    };

    fn parse_expression(source: &str) -> ScalarExpression {
        serde_yaml::from_str::<ScalarExpression>(&format!("\"{source}\""))
            .expect("expression should parse")
    }

    #[test]
    fn expression_supports_builtins_and_params() {
        let expression = parse_expression("lerp(energy, clamp(t, 0, 10), easeInOut(0.5)) + sin(0)");
        let mut params = std::collections::BTreeMap::new();
        params.insert("energy".to_owned(), 2.0);

        let context = ExpressionContext::new(4.0, &params, 7);
        let value = expression
            .evaluate_with_context(&context)
            .expect("expression should evaluate");

        // easeInOut(0.5) == 0.5, sin(0) == 0
        assert!((value - 3.0).abs() < 0.0001);
    }

    #[test]
    fn expression_clamp_rejects_inverted_bounds() {
        let expression = parse_expression("clamp(t, 5, 1)");
        let params = std::collections::BTreeMap::new();

        let error = expression
            .evaluate_with_context(&ExpressionContext::new(2.0, &params, 0))
            .expect_err("inverted clamp bounds should fail");
        assert!(error.to_string().contains("requires min <= max"));
    }

    #[test]
    fn expression_noise_is_deterministic() {
        let expression = parse_expression("noise1d(t * 0.1)");
        let params = std::collections::BTreeMap::new();

        let a = expression
            .evaluate_with_context(&ExpressionContext::new(12.0, &params, 99))
            .expect("noise should evaluate");
        let b = expression
            .evaluate_with_context(&ExpressionContext::new(12.0, &params, 99))
            .expect("noise should evaluate");
        let c = expression
            .evaluate_with_context(&ExpressionContext::new(12.0, &params, 100))
            .expect("noise should evaluate");

        assert!((a - b).abs() < f32::EPSILON);
        assert!((a - c).abs() > 0.0001);
    }

    #[test]
    fn expression_unknown_variable_returns_error() {
        let expression = parse_expression("energy + missing_param");
        let mut params = std::collections::BTreeMap::new();
        params.insert("energy".to_owned(), 1.0);

        let error = expression
            .evaluate_with_context(&ExpressionContext::new(0.0, &params, 0))
            .expect_err("missing_param should fail validation");
        assert!(error.to_string().contains("missing_param"));
    }

    #[test]
    fn manifest_parses_groups_params_and_modulators() {
        let manifest = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 1920, height: 1080 }
  fps: 24
  duration: { frames: 48 }
seed: 42
params:
  energy: 0.8
modulators:
  wobble:
    expression: "noise1d(t * 0.1) * energy"
groups:
  - id: root
    position: [10, 20]
layers:
  - id: gradient
    group: root
    modulators:
      - source: wobble
        weights:
          x: 30
    procedural:
      kind: gradient
      start_color: { r: 0.1, g: 0.2, b: 0.3, a: 1.0 }
      end_color: { r: 0.7, g: 0.2, b: 0.5, a: 1.0 }
"#,
        )
        .expect("manifest should parse");

        assert_eq!(manifest.version, default_manifest_version());
        assert_eq!(manifest.groups.len(), 1);
        assert_eq!(manifest.modulators.len(), 1);
        validate_manifest_manifest_level(&manifest).expect("manifest level validation should pass");
    }

    #[test]
    fn scalar_property_expression_uses_context() {
        let property = ScalarProperty::Expression(parse_expression("energy * cos(t)"));
        let mut params = std::collections::BTreeMap::new();
        params.insert("energy".to_owned(), 2.0);

        let value = property
            .evaluate_with_context(&ExpressionContext::new(0.0, &params, 0))
            .expect("property should evaluate");
        assert!((value - 2.0).abs() < 0.0001);
    }

    #[test]
    fn manifest_parses_image_layer_shape() {
        let manifest = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: title
    image:
      path: "assets/title.png"
"#,
        )
        .expect("manifest should parse");

        assert!(matches!(manifest.layers.first(), Some(Layer::Image(_))));
    }

    #[test]
    fn manifest_parses_legacy_source_path_layer_shape() {
        let manifest = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: title
    source_path: "assets/title.png"
"#,
        )
        .expect("manifest should parse");

        assert!(matches!(manifest.layers.first(), Some(Layer::Asset(_))));
    }

    #[test]
    fn manifest_parses_ascii_layer_shape() {
        let manifest = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: terminal_grid
    ascii:
      grid: { rows: 2, columns: 4 }
      cell: { width: 12, height: 16 }
      font_variant: geist_pixel_regular
      foreground: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
      background: { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
      inline:
        - "ABCD"
        - "1234"
"#,
        )
        .expect("manifest should parse");

        assert!(matches!(manifest.layers.first(), Some(Layer::Ascii(_))));
    }

    #[test]
    fn ascii_layer_rejects_non_printable_characters() {
        let manifest = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: terminal_grid
    ascii:
      grid: { rows: 1, columns: 1 }
      cell: { width: 8, height: 8 }
      font_variant: geist_pixel_regular
      foreground: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
      background: { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
      inline:
        - "é"
"#,
        )
        .expect("manifest should parse shape");

        let layer = manifest.layers.first().expect("expected one layer");
        let error = layer
            .validate(&manifest.params, manifest.seed, &manifest.modulators)
            .expect_err("non-printable ASCII should be rejected");
        let message = error.to_string();
        assert!(
            message.contains("ASCII") || message.contains("columns"),
            "unexpected validation message: {message}"
        );
    }

    #[test]
    fn manifest_layer_requires_source_block() {
        let error = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: missing_source
    position: [10, 20]
"#,
        )
        .expect_err("manifest should fail without layer source");

        let message = error.to_string();
        assert!(message.contains("layer 'missing_source'"));
        assert!(message.contains("must define exactly one source block"));
    }

    #[test]
    fn manifest_layer_rejects_multiple_source_blocks() {
        let error = serde_yaml::from_str::<Manifest>(
            r#"
version: 1
environment:
  resolution: { width: 320, height: 180 }
  fps: 24
  duration: { frames: 12 }
layers:
  - id: too_many_sources
    source_path: "assets/a.png"
    image:
      path: "assets/b.png"
"#,
        )
        .expect_err("manifest should fail when layer has multiple sources");

        let message = error.to_string();
        assert!(message.contains("layer 'too_many_sources'"));
        assert!(message.contains("multiple source blocks"));
        assert!(message.contains("source_path, image"));
    }

    #[test]
    fn manifest_parses_empty_post_section() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
"#,
        )
        .expect("manifest without post section should parse");
        assert!(manifest.post.is_empty());
    }

    #[test]
    fn manifest_parses_passthrough_post_effect() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
post:
  - shader: passthrough
"#,
        )
        .expect("manifest with passthrough post should parse");
        assert_eq!(manifest.post.len(), 1);
        assert!(matches!(
            manifest.post[0].kind,
            PostEffectKind::Passthrough
        ));
    }

    #[test]
    fn manifest_parses_levels_post_effect_with_defaults() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
post:
  - shader: levels
"#,
        )
        .expect("manifest with levels post should parse");
        assert_eq!(manifest.post.len(), 1);
        match &manifest.post[0].kind {
            PostEffectKind::Levels { gamma, lift, gain } => {
                assert!((gamma - 1.0).abs() < f32::EPSILON);
                assert!((lift - 0.0).abs() < f32::EPSILON);
                assert!((gain - 1.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Levels, got {other:?}"),
        }
    }

    #[test]
    fn manifest_parses_levels_post_effect_with_custom_params() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
post:
  - shader: levels
    gamma: 1.5
    lift: 0.1
    gain: 0.8
"#,
        )
        .expect("manifest with customized levels should parse");
        match &manifest.post[0].kind {
            PostEffectKind::Levels { gamma, lift, gain } => {
                assert!((gamma - 1.5).abs() < f32::EPSILON);
                assert!((lift - 0.1).abs() < f32::EPSILON);
                assert!((gain - 0.8).abs() < f32::EPSILON);
            }
            other => panic!("expected Levels, got {other:?}"),
        }
    }

    #[test]
    fn manifest_parses_sobel_post_effect() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
post:
  - shader: sobel
    strength: 2.5
"#,
        )
        .expect("manifest with sobel post should parse");
        match &manifest.post[0].kind {
            PostEffectKind::Sobel { strength } => {
                assert!((strength - 2.5).abs() < f32::EPSILON);
            }
            other => panic!("expected Sobel, got {other:?}"),
        }
    }

    #[test]
    fn manifest_parses_chained_post_effects() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
post:
  - shader: levels
    gamma: 1.2
    lift: 0.05
    gain: 0.9
  - shader: sobel
    strength: 1.0
"#,
        )
        .expect("manifest with chained post effects should parse");
        assert_eq!(manifest.post.len(), 2);
        assert!(matches!(
            manifest.post[0].kind,
            PostEffectKind::Levels { .. }
        ));
        assert!(matches!(
            manifest.post[1].kind,
            PostEffectKind::Sobel { .. }
        ));
    }

    // ---- ascii_post schema tests ----

    #[test]
    fn manifest_ascii_post_is_none_by_default() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
"#,
        )
        .expect("manifest without ascii_post should parse");
        assert!(manifest.ascii_post.is_none());
    }

    #[test]
    fn manifest_ascii_post_parses_enabled_config() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 640, height: 360 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
ascii_post:
  enabled: true
  cols: 80
  rows: 24
  ramp: " .:-=+*#%@"
"#,
        )
        .expect("manifest with ascii_post should parse");
        let config = manifest.ascii_post.as_ref().expect("ascii_post should be Some");
        assert!(config.enabled);
        assert_eq!(config.cols, 80);
        assert_eq!(config.rows, 24);
        assert_eq!(config.ramp, " .:-=+*#%@");
        assert_eq!(config.ramp_len(), 10);
    }

    #[test]
    fn manifest_ascii_post_uses_defaults_when_minimal() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
ascii_post:
  enabled: true
"#,
        )
        .expect("minimal ascii_post should parse");
        let config = manifest.ascii_post.as_ref().unwrap();
        assert!(config.enabled);
        assert_eq!(config.cols, 120);
        assert_eq!(config.rows, 45);
        assert_eq!(config.ramp, " .:-=+*#%@");
    }

    #[test]
    fn manifest_ascii_post_validates_cols_zero_rejected() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
ascii_post:
  enabled: true
  cols: 0
"#,
        )
        .expect("should parse");
        let err = manifest
            .ascii_post
            .as_ref()
            .unwrap()
            .validate()
            .expect_err("cols=0 should fail validation");
        assert!(err.to_string().contains("cols must be > 0"));
    }

    #[test]
    fn manifest_ascii_post_validates_rows_zero_rejected() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
ascii_post:
  enabled: true
  rows: 0
"#,
        )
        .expect("should parse");
        let err = manifest
            .ascii_post
            .as_ref()
            .unwrap()
            .validate()
            .expect_err("rows=0 should fail validation");
        assert!(err.to_string().contains("rows must be > 0"));
    }

    #[test]
    fn manifest_ascii_post_validates_ramp_too_short_rejected() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
ascii_post:
  enabled: true
  ramp: "X"
"#,
        )
        .expect("should parse");
        let err = manifest
            .ascii_post
            .as_ref()
            .unwrap()
            .validate()
            .expect_err("ramp with 1 char should fail validation");
        assert!(err.to_string().contains("at least 2 characters"));
    }

    #[test]
    fn manifest_ascii_post_disabled_skips_validation() {
        let manifest: Manifest = serde_yaml::from_str(
            r#"
version: 1
environment:
  resolution: { width: 8, height: 8 }
  fps: 24
  duration: { frames: 1 }
layers:
  - id: bg
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }
ascii_post:
  enabled: false
  cols: 0
  rows: 0
  ramp: "X"
"#,
        )
        .expect("should parse");
        // When disabled, validation should pass even with invalid values
        manifest
            .ascii_post
            .as_ref()
            .unwrap()
            .validate()
            .expect("disabled ascii_post should skip validation");
    }
}
