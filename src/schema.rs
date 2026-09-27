use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use serde::{de::DeserializeOwned, de::Error as DeError, Deserialize, Deserializer, Serialize};

pub type Parameters = BTreeMap<String, f32>;
pub type ModulatorMap = BTreeMap<String, ModulatorDefinition>;

const DEFAULT_MANIFEST_VERSION: u32 = 1;
/// First manifest version where expression `t` and keyframe times are in seconds.
pub const SECONDS_TIME_MANIFEST_VERSION: u32 = 2;
const LATEST_MANIFEST_VERSION: u32 = SECONDS_TIME_MANIFEST_VERSION;
const DEFAULT_ENV_ATTACK_FRAMES: f32 = 12.0;
const DEFAULT_ENV_DECAY_FRAMES: f32 = 24.0;
const DEFAULT_ENV_ATTACK_SECONDS: f32 = 0.5;
const DEFAULT_ENV_DECAY_SECONDS: f32 = 1.0;
/// Names that expressions resolve as time builtins. In version 2 manifests they are
/// reserved; in version 1 a param with the same name wins for `frame`/`fps` so that
/// legacy manifests keep their meaning.
const TIME_BUILTINS: [&str; 3] = ["t", "frame", "fps"];

/// Unit that expression `t` is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    /// Version 1: `t` is the (layer-local) frame number, so animation speed depends on fps.
    Frames,
    /// Version 2+: `t` is (layer-local) seconds, so animation is fps-independent.
    Seconds,
}

/// Everything needed to turn a frame index into expression/keyframe time.
///
/// All evaluation happens on a layer-local frame (after group/layer timing remaps);
/// `TimeBase` decides how that frame is exposed to expressions and how keyframe times
/// expressed in seconds are mapped onto it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeBase {
    pub fps: u32,
    pub unit: TimeUnit,
}

impl TimeBase {
    pub fn for_manifest_version(version: u32, fps: u32) -> Self {
        let unit = if version >= SECONDS_TIME_MANIFEST_VERSION {
            TimeUnit::Seconds
        } else {
            TimeUnit::Frames
        };
        Self { fps, unit }
    }

    pub fn legacy_frames(fps: u32) -> Self {
        Self {
            fps,
            unit: TimeUnit::Frames,
        }
    }

    pub fn seconds(fps: u32) -> Self {
        Self {
            fps,
            unit: TimeUnit::Seconds,
        }
    }

    pub fn fps_f32(self) -> f32 {
        self.fps.max(1) as f32
    }

    /// Version 2 evaluates procedural/shader/ascii source animation in layer-local time
    /// (honouring start_time/time_offset/time_scale and group timing). Version 1 kept the
    /// historical behaviour of evaluating sources at the global frame.
    pub fn sources_use_local_time(self) -> bool {
        self.unit == TimeUnit::Seconds
    }
}
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
    #[serde(skip)]
    pub param_definitions: BTreeMap<String, ParamDefinition>,
    #[serde(skip)]
    pub resolved_params: BTreeMap<String, ParamValue>,
    #[serde(skip)]
    pub applied_param_overrides: BTreeMap<String, ParamValue>,
    #[serde(skip)]
    pub manifest_hash: String,
}

impl Manifest {
    pub fn time_base(&self) -> TimeBase {
        TimeBase::for_manifest_version(self.version, self.environment.fps)
    }

    /// Context used to probe expressions at time zero during validation.
    pub fn probe_context(&self) -> ExpressionContext<'_> {
        ExpressionContext::new(0.0, self.time_base(), &self.params, self.seed)
    }
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
    fn validate(&self, name: &str, probe: &ExpressionContext<'_>) -> Result<()> {
        let value = self
            .expression
            .evaluate_with_context(probe)
            .map_err(|error| anyhow!("modulator '{name}': {error}"))?;
        validate_number(&format!("modulator '{name}' expression result"), value)
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

    pub fn validate(&self, probe: &ExpressionContext<'_>, modulators: &ModulatorMap) -> Result<()> {
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
                .validate_with_context("pos_x", probe)
                .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        }
        if let Some(position_y) = &self.pos_y {
            position_y
                .validate_with_context("pos_y", probe)
                .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        }
        self.scale
            .validate("scale")
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        self.rotation_degrees
            .validate_with_context("rotation_degrees", probe)
            .map_err(|error| anyhow!("group '{}': {error}", self.id))?;
        self.opacity
            .validate_with_context("opacity", probe)
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

    pub fn sample_position_with_context(&self, context: &ExpressionContext<'_>) -> Result<Vec2> {
        let mut position = self.position.sample(context);
        if let Some(pos_x) = &self.pos_x {
            position.x = pos_x.evaluate_with_context(context)?;
        }
        if let Some(pos_y) = &self.pos_y {
            position.y = pos_y.evaluate_with_context(context)?;
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
        probe: &ExpressionContext<'_>,
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
                .validate_with_context("pos_x", probe)
                .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        }
        if let Some(position_y) = &self.pos_y {
            position_y
                .validate_with_context("pos_y", probe)
                .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        }
        self.scale
            .validate("scale")
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        self.rotation_degrees
            .validate_with_context("rotation_degrees", probe)
            .map_err(|error| anyhow!("layer '{}': {error}", self.id))?;
        self.opacity
            .validate_with_context("opacity", probe)
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

    pub fn validate(&self, probe: &ExpressionContext<'_>, modulators: &ModulatorMap) -> Result<()> {
        self.common().validate_with_context(probe, modulators)?;
        match self {
            Self::Asset(layer) => layer.validate(),
            Self::Image(layer) => layer.validate(),
            Self::Procedural(layer) => layer.validate(probe),
            Self::Shader(layer) => layer.validate(probe),
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
    fn validate(&self, probe: &ExpressionContext<'_>) -> Result<()> {
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
            prop.validate_with_context(&format!("{label}.uniforms.{name}"), probe)?;
        }
        Ok(())
    }
}

impl ProceduralLayer {
    fn validate(&self, probe: &ExpressionContext<'_>) -> Result<()> {
        self.procedural
            .validate(probe)
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
    fn validate(&self, probe: &ExpressionContext<'_>) -> Result<()> {
        match self {
            Self::SolidColor { color } => color.validate("color", probe),
            Self::Gradient {
                start_color,
                end_color,
                ..
            } => {
                start_color.validate("start_color", probe)?;
                end_color.validate("end_color", probe)
            }
            Self::Triangle { color, .. } => color.validate("color", probe),
            Self::Circle { radius, color, .. } => {
                radius.validate_with_context("radius", probe)?;
                color.validate("color", probe)
            }
            Self::RoundedRect {
                corner_radius,
                color,
                ..
            } => {
                corner_radius.validate_with_context("corner_radius", probe)?;
                color.validate("color", probe)
            }
            Self::Ring {
                outer_radius,
                inner_radius,
                color,
                ..
            } => {
                outer_radius.validate_with_context("outer_radius", probe)?;
                inner_radius.validate_with_context("inner_radius", probe)?;
                color.validate("color", probe)
            }
            Self::Line {
                thickness, color, ..
            } => {
                thickness.validate_with_context("thickness", probe)?;
                color.validate("color", probe)
            }
            Self::Polygon {
                radius,
                sides,
                color,
                ..
            } => {
                radius.validate_with_context("radius", probe)?;
                if *sides < 3 {
                    bail!("polygon sides must be >= 3");
                }
                color.validate("color", probe)
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

/// Animatable color. Accepts a static color `{r: 0.5, g: 0.2, b: 0.1, a: 1}`, per-channel
/// animation (`{r: "sin(t)", g: {keyframes: [...]}, b: 0, a: 1}`), or a whole-color keyframe
/// track (`{keyframes: [{time: 0, value: {r: 1, g: 0, b: 0}}, ...]}`).
#[derive(Debug, Clone)]
// Parsed once per manifest; boxing the larger variant would buy nothing measurable.
#[allow(clippy::large_enum_variant)]
pub enum AnimatableColor {
    Channels {
        r: ScalarProperty,
        g: ScalarProperty,
        b: ScalarProperty,
        a: ScalarProperty,
    },
    Keyframes(KeyframeTrack<ColorRgba>),
}

impl AnimatableColor {
    pub fn evaluate(&self, context: &ExpressionContext<'_>) -> Result<ColorRgba> {
        match self {
            Self::Channels { r, g, b, a } => Ok(ColorRgba {
                r: r.evaluate_with_context(context)?,
                g: g.evaluate_with_context(context)?,
                b: b.evaluate_with_context(context)?,
                a: a.evaluate_with_context(context)?,
            }),
            Self::Keyframes(track) => Ok(track.sample(context)),
        }
    }

    pub fn is_static(&self) -> bool {
        match self {
            Self::Channels { r, g, b, a } => {
                r.is_static() && g.is_static() && b.is_static() && a.is_static()
            }
            Self::Keyframes(_) => false,
        }
    }

    pub fn validate(&self, label: &str, probe: &ExpressionContext<'_>) -> Result<()> {
        match self {
            Self::Channels { r, g, b, a } => {
                r.validate_with_context(&format!("{label}.r"), probe)?;
                g.validate_with_context(&format!("{label}.g"), probe)?;
                b.validate_with_context(&format!("{label}.b"), probe)?;
                a.validate_with_context(&format!("{label}.a"), probe)?;
                Ok(())
            }
            Self::Keyframes(track) => {
                track.validate(label, |value_label, color| color.validate(value_label))
            }
        }
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

        let value = serde_yaml::Value::deserialize(deserializer)?;
        if value
            .as_mapping()
            .is_some_and(|map| map.contains_key("keyframes"))
        {
            return match decode_animated::<ColorRgba, D::Error>(value)? {
                AnimatedWire::Track(track) => Ok(Self::Keyframes(track)),
                AnimatedWire::Static(color) => Ok(color.into()),
            };
        }

        let fields: ColorFields = serde_yaml::from_value(value).map_err(D::Error::custom)?;
        Ok(Self::Channels {
            r: fields.r,
            g: fields.g,
            b: fields.b,
            a: fields.a,
        })
    }
}

impl From<ColorRgba> for AnimatableColor {
    fn from(c: ColorRgba) -> Self {
        Self::Channels {
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

/// Animatable non-scalar property (position, scale): a static value, a `keyframes:` track,
/// or the legacy single-segment `{start_frame, end_frame, from, to, easing}` mapping.
#[derive(Debug, Clone)]
pub enum PropertyValue<T> {
    Static(T),
    Keyframes(KeyframeTrack<T>),
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for PropertyValue<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        Ok(match decode_animated::<T, D::Error>(value)? {
            AnimatedWire::Static(value) => Self::Static(value),
            AnimatedWire::Track(track) => Self::Keyframes(track),
        })
    }
}

impl<T: Clone + Interpolate> PropertyValue<T> {
    pub fn sample(&self, context: &ExpressionContext<'_>) -> T {
        match self {
            Self::Static(value) => value.clone(),
            Self::Keyframes(track) => track.sample(context),
        }
    }
}

impl<T> PropertyValue<T> {
    pub fn validate(&self, label: &str) -> Result<()> {
        if let Self::Keyframes(track) = self {
            // Values (Vec2) are checked for finiteness while decoding.
            track.validate(label, |_, _| Ok(()))?;
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

/// Animatable scalar: a number, an expression string, a `keyframes:` track, or the legacy
/// single-segment mapping.
#[derive(Debug, Clone)]
pub enum ScalarProperty {
    Static(f32),
    Keyframes(KeyframeTrack<f32>),
    Expression(ScalarExpression),
}

impl<'de> Deserialize<'de> for ScalarProperty {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_yaml::Value::deserialize(deserializer)?;
        if value.is_string() {
            return serde_yaml::from_value(value)
                .map(Self::Expression)
                .map_err(D::Error::custom);
        }
        Ok(match decode_animated::<f32, D::Error>(value)? {
            AnimatedWire::Static(value) => Self::Static(value),
            AnimatedWire::Track(track) => Self::Keyframes(track),
        })
    }
}

impl ScalarProperty {
    pub fn evaluate_with_context(&self, context: &ExpressionContext<'_>) -> Result<f32> {
        match self {
            Self::Static(value) => Ok(*value),
            Self::Keyframes(track) => Ok(track.sample(context)),
            Self::Expression(expression) => expression.evaluate_with_context(context),
        }
    }

    pub fn validate_with_context(&self, label: &str, probe: &ExpressionContext<'_>) -> Result<()> {
        match self {
            Self::Static(value) => validate_number(label, *value),
            Self::Keyframes(track) => track.validate(label, |value_label, value| {
                validate_number(value_label, *value)
            }),
            Self::Expression(expression) => {
                let value = expression.evaluate_with_context(probe)?;
                validate_number(label, value)
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
            Self::Variable(identifier) => context.resolve_variable(identifier),
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
    /// Layer-local frame (fractional after time_scale remaps).
    pub frame: f32,
    pub time_base: TimeBase,
    pub params: &'a Parameters,
    pub seed: u64,
}

impl<'a> ExpressionContext<'a> {
    pub fn new(frame: f32, time_base: TimeBase, params: &'a Parameters, seed: u64) -> Self {
        Self {
            frame,
            time_base,
            params,
            seed,
        }
    }

    pub fn with_frame(self, frame: f32) -> Self {
        Self { frame, ..self }
    }

    /// Value of the expression variable `t` (frames in version 1, seconds in version 2).
    pub fn t(&self) -> f32 {
        match self.time_base.unit {
            TimeUnit::Frames => self.frame,
            TimeUnit::Seconds => self.seconds(),
        }
    }

    pub fn seconds(&self) -> f32 {
        self.frame / self.time_base.fps_f32()
    }

    fn resolve_variable(&self, identifier: &str) -> Result<f32> {
        if identifier == "t" {
            return Ok(self.t());
        }
        // Version 1 manifests may already define params named `frame`/`fps`; keep their meaning.
        if let Some(value) = self.params.get(identifier) {
            return Ok(*value);
        }
        match identifier {
            "frame" => Ok(self.frame),
            "fps" => Ok(self.time_base.fps_f32()),
            _ => Err(anyhow!("unknown variable '{identifier}'")),
        }
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

/// A point on a timeline, either an explicit frame number or seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyTime {
    Frame(f32),
    Seconds(f32),
}

impl KeyTime {
    pub fn to_frame(self, time_base: TimeBase) -> f32 {
        match self {
            Self::Frame(frame) => frame,
            Self::Seconds(seconds) => seconds * time_base.fps_f32(),
        }
    }

    fn raw(self) -> f32 {
        match self {
            Self::Frame(value) | Self::Seconds(value) => value,
        }
    }

    fn unit_label(self) -> &'static str {
        match self {
            Self::Frame(_) => "frame",
            Self::Seconds(_) => "time",
        }
    }

    fn same_unit(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Frame(_), Self::Frame(_)) | (Self::Seconds(_), Self::Seconds(_))
        )
    }
}

/// One key on a track. `easing` shapes the segment from this key to the next one
/// (it is ignored on the last key).
#[derive(Debug, Clone)]
pub struct Keyframe<T> {
    pub at: KeyTime,
    pub value: T,
    pub easing: EasingCurve,
}

/// Multi-keyframe animation track. Always holds at least one key.
///
/// Before the first key the first value holds; after the last key the last value holds.
/// The legacy single-segment mapping `{start_frame, end_frame, from, to, easing}` is sugar
/// for a two-key track.
#[derive(Debug, Clone)]
pub struct KeyframeTrack<T> {
    keys: Vec<Keyframe<T>>,
}

impl<T> KeyframeTrack<T> {
    pub fn new(keys: Vec<Keyframe<T>>) -> Result<Self> {
        if keys.is_empty() {
            bail!("keyframes must contain at least one key");
        }
        Ok(Self { keys })
    }

    pub fn keys(&self) -> &[Keyframe<T>] {
        &self.keys
    }

    /// Checks key ordering, unit consistency, easing parameters and each value.
    pub fn validate(
        &self,
        label: &str,
        mut validate_value: impl FnMut(&str, &T) -> Result<()>,
    ) -> Result<()> {
        let first_unit = self.keys[0].at;
        for (index, key) in self.keys.iter().enumerate() {
            let key_label = format!("{label}.keyframes[{index}]");
            if !key.at.same_unit(first_unit) {
                bail!(
                    "{key_label} uses `{}` but keyframes[0] uses `{}`; all keys in a track must use the same unit",
                    key.at.unit_label(),
                    first_unit.unit_label()
                );
            }
            validate_number(
                &format!("{key_label}.{}", key.at.unit_label()),
                key.at.raw(),
            )?;
            validate_value(&format!("{key_label}.value"), &key.value)?;
            key.easing.validate(&format!("{key_label}.easing"))?;
        }
        for (index, pair) in self.keys.windows(2).enumerate() {
            let (previous, next) = (pair[0].at.raw(), pair[1].at.raw());
            if next <= previous {
                bail!(
                    "{label}.keyframes[{}].{unit} ({next}) must be greater than keyframes[{index}].{unit} ({previous}); keys must be strictly increasing",
                    index + 1,
                    unit = pair[1].at.unit_label()
                );
            }
        }
        Ok(())
    }
}

impl<T: Clone + Interpolate> KeyframeTrack<T> {
    pub fn sample(&self, context: &ExpressionContext<'_>) -> T {
        let frame = context.frame;
        let first = &self.keys[0];
        if frame <= first.at.to_frame(context.time_base) {
            return first.value.clone();
        }

        for pair in self.keys.windows(2) {
            let (from, to) = (&pair[0], &pair[1]);
            let end_frame = to.at.to_frame(context.time_base);
            if frame < end_frame {
                if from.easing == EasingCurve::Hold {
                    return from.value.clone();
                }
                let start_frame = from.at.to_frame(context.time_base);
                let span = end_frame - start_frame;
                let progress = (frame - start_frame) / span;
                let eased = from.easing.apply(progress.clamp(0.0, 1.0));
                return T::interpolate(&from.value, &to.value, eased);
            }
        }

        self.keys[self.keys.len() - 1].value.clone()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyframeTrackWire<T> {
    keyframes: Vec<KeyframeWire<T>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyframeWire<T> {
    #[serde(default)]
    time: Option<f32>,
    #[serde(default)]
    frame: Option<f32>,
    value: T,
    #[serde(default)]
    easing: EasingCurve,
}

impl<T> KeyframeTrackWire<T> {
    fn into_track(self) -> Result<KeyframeTrack<T>> {
        let keys = self
            .keyframes
            .into_iter()
            .enumerate()
            .map(|(index, key)| {
                let at = match (key.time, key.frame) {
                    (Some(seconds), None) => KeyTime::Seconds(seconds),
                    (None, Some(frame)) => KeyTime::Frame(frame),
                    _ => bail!(
                        "keyframes[{index}] must set exactly one of `time` (seconds) or `frame`"
                    ),
                };
                Ok(Keyframe {
                    at,
                    value: key.value,
                    easing: key.easing,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        KeyframeTrack::new(keys)
    }
}

/// Legacy single-segment mapping; desugars to a two-key track. Endpoints are given either in
/// frames (`start_frame`/`end_frame`) or in seconds (`start_time`/`end_time`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentMappingWire<T> {
    #[serde(default)]
    start_frame: Option<u32>,
    #[serde(default)]
    end_frame: Option<u32>,
    #[serde(default)]
    start_time: Option<f32>,
    #[serde(default)]
    end_time: Option<f32>,
    from: T,
    to: T,
    #[serde(default)]
    easing: EasingCurve,
}

impl<T> SegmentMappingWire<T> {
    fn into_track(self) -> Result<KeyframeTrack<T>> {
        let (start, end) = match (
            self.start_frame,
            self.end_frame,
            self.start_time,
            self.end_time,
        ) {
            (Some(start), Some(end), None, None) => {
                if end <= start {
                    bail!("mapping requires end_frame ({end}) > start_frame ({start})");
                }
                (KeyTime::Frame(start as f32), KeyTime::Frame(end as f32))
            }
            (None, None, Some(start), Some(end)) => {
                if end <= start {
                    bail!("mapping requires end_time ({end}) > start_time ({start})");
                }
                (KeyTime::Seconds(start), KeyTime::Seconds(end))
            }
            _ => bail!(
                "mapping must set either start_frame and end_frame (frames) or start_time and end_time (seconds), not a mix"
            ),
        };
        KeyframeTrack::new(vec![
            Keyframe {
                at: start,
                value: self.from,
                easing: self.easing,
            },
            Keyframe {
                at: end,
                value: self.to,
                easing: EasingCurve::Linear,
            },
        ])
    }
}

/// Animatable value decoded from YAML: a static value, a `keyframes:` track, or the legacy
/// `{from, to, ...}` mapping (desugared to a track).
enum AnimatedWire<T> {
    Static(T),
    Track(KeyframeTrack<T>),
}

fn decode_animated<T: DeserializeOwned, E: DeError>(
    value: serde_yaml::Value,
) -> std::result::Result<AnimatedWire<T>, E> {
    if let serde_yaml::Value::Mapping(map) = &value {
        if map.contains_key("keyframes") {
            let wire: KeyframeTrackWire<T> = serde_yaml::from_value(value).map_err(E::custom)?;
            return wire
                .into_track()
                .map(AnimatedWire::Track)
                .map_err(E::custom);
        }
        if map.contains_key("from") || map.contains_key("to") {
            let wire: SegmentMappingWire<T> = serde_yaml::from_value(value).map_err(E::custom)?;
            return wire
                .into_track()
                .map(AnimatedWire::Track)
                .map_err(E::custom);
        }
    }
    serde_yaml::from_value(value)
        .map(AnimatedWire::Static)
        .map_err(E::custom)
}

/// Easing for a keyframe segment.
///
/// YAML: `linear`, `ease_in`, `ease_out`, `ease_in_out`, `hold` (alias `step`), or a CSS-style
/// cubic bezier as `[x1, y1, x2, y2]` / `{ cubic_bezier: [x1, y1, x2, y2] }`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum EasingCurve {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Keep the segment's start value until the next key (step / hold interpolation).
    Hold,
    CubicBezier {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
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
            Self::Hold => 0.0,
            Self::CubicBezier { x1, y1, x2, y2 } => cubic_bezier_ease(x1, y1, x2, y2, t),
        }
    }

    fn validate(self, label: &str) -> Result<()> {
        if let Self::CubicBezier { x1, y1, x2, y2 } = self {
            for (name, value) in [("x1", x1), ("y1", y1), ("x2", x2), ("y2", y2)] {
                validate_number(&format!("{label}.{name}"), value)?;
            }
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                bail!("{label} cubic bezier x1 and x2 must be within [0, 1], got x1={x1}, x2={x2}");
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for EasingCurve {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        const EXPECTED: &str =
            "linear, ease_in, ease_out, ease_in_out, hold, or a cubic bezier [x1, y1, x2, y2]";

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct BezierObject {
            cubic_bezier: [f32; 4],
        }

        let bezier = |[x1, y1, x2, y2]: [f32; 4]| Self::CubicBezier { x1, y1, x2, y2 };
        let value = serde_yaml::Value::deserialize(deserializer)?;
        match value {
            serde_yaml::Value::String(name) => match name.as_str() {
                "linear" => Ok(Self::Linear),
                "ease_in" => Ok(Self::EaseIn),
                "ease_out" => Ok(Self::EaseOut),
                "ease_in_out" => Ok(Self::EaseInOut),
                "hold" | "step" => Ok(Self::Hold),
                other => Err(D::Error::custom(format!(
                    "unknown easing '{other}'; expected {EXPECTED}"
                ))),
            },
            serde_yaml::Value::Sequence(_) => serde_yaml::from_value::<[f32; 4]>(value)
                .map(bezier)
                .map_err(|error| {
                    D::Error::custom(format!(
                        "invalid cubic bezier easing ({error}); expected {EXPECTED}"
                    ))
                }),
            serde_yaml::Value::Mapping(_) => serde_yaml::from_value::<BezierObject>(value)
                .map(|object| bezier(object.cubic_bezier))
                .map_err(|error| {
                    D::Error::custom(format!("invalid easing ({error}); expected {EXPECTED}"))
                }),
            _ => Err(D::Error::custom(format!(
                "invalid easing; expected {EXPECTED}"
            ))),
        }
    }
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` timing function: solve x(s) = t, return y(s).
/// Pure f32 arithmetic with a fixed iteration budget, so results are deterministic.
fn cubic_bezier_ease(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }

    fn curve(s: f32, p1: f32, p2: f32) -> f32 {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        let a = 1.0 - c - b;
        ((a * s + b) * s + c) * s
    }
    fn slope(s: f32, p1: f32, p2: f32) -> f32 {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        let a = 1.0 - c - b;
        (3.0 * a * s + 2.0 * b) * s + c
    }

    let mut s = t;
    for _ in 0..8 {
        let error = curve(s, x1, x2) - t;
        if error.abs() < 1e-6 {
            return curve(s, y1, y2);
        }
        let derivative = slope(s, x1, x2);
        if derivative.abs() < 1e-6 {
            break;
        }
        s -= error / derivative;
    }

    // Newton stalled on a flat section; bisect (x(s) is monotonic for x1, x2 in [0, 1]).
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    s = t;
    for _ in 0..32 {
        let x = curve(s, x1, x2);
        if (x - t).abs() < 1e-7 {
            break;
        }
        if x < t {
            low = s;
        } else {
            high = s;
        }
        s = 0.5 * (low + high);
    }
    curve(s, y1, y2)
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

/// Colors interpolate per channel on the authored (sRGB-encoded) values, matching how
/// per-channel expressions and keyframes behave.
impl Interpolate for ColorRgba {
    fn interpolate(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            r: <f32 as Interpolate>::interpolate(&from.r, &to.r, t),
            g: <f32 as Interpolate>::interpolate(&from.g, &to.g, t),
            b: <f32 as Interpolate>::interpolate(&from.b, &to.b, t),
            a: <f32 as Interpolate>::interpolate(&from.a, &to.a, t),
        }
    }
}

pub fn validate_manifest_manifest_level(manifest: &Manifest) -> Result<()> {
    if !(DEFAULT_MANIFEST_VERSION..=LATEST_MANIFEST_VERSION).contains(&manifest.version) {
        bail!(
            "unsupported manifest version {} (supported: {}..={}). Use version: {} for seconds-based time or version: {} for legacy frame-based time",
            manifest.version,
            DEFAULT_MANIFEST_VERSION,
            LATEST_MANIFEST_VERSION,
            LATEST_MANIFEST_VERSION,
            DEFAULT_MANIFEST_VERSION
        );
    }
    let seconds_time = manifest.time_base().unit == TimeUnit::Seconds;

    for (name, value) in &manifest.params {
        if !valid_identifier(name) {
            bail!("invalid param name '{name}'. Use identifiers like energy, phase, tension_2");
        }
        if name == "t" {
            bail!("param name 't' is reserved for time in expressions");
        }
        if seconds_time && TIME_BUILTINS.contains(&name.as_str()) {
            bail!(
                "param name '{name}' is reserved in version {} manifests (expressions expose t, frame, fps)",
                manifest.version
            );
        }
        validate_number(&format!("param '{name}'"), *value)?;
    }

    let probe = manifest.probe_context();
    for (name, modulator) in &manifest.modulators {
        if !valid_identifier(name) {
            bail!("invalid modulator name '{name}'. Use identifiers like wobble or pulse_1");
        }
        modulator.validate(name, &probe)?;
    }

    let mut seen_group_ids = HashSet::with_capacity(manifest.groups.len());
    for group in &manifest.groups {
        group.validate(&probe, &manifest.modulators)?;
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
            let (default_attack, default_decay) = match context.time_base.unit {
                TimeUnit::Frames => (DEFAULT_ENV_ATTACK_FRAMES, DEFAULT_ENV_DECAY_FRAMES),
                TimeUnit::Seconds => (DEFAULT_ENV_ATTACK_SECONDS, DEFAULT_ENV_DECAY_SECONDS),
            };
            let attack = evaluated.get(1).copied().unwrap_or(default_attack);
            let decay = evaluated.get(2).copied().unwrap_or(default_decay);
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

#[cfg(test)]
mod tests {
    use super::{
        default_manifest_version, validate_manifest_manifest_level, ExpressionContext, Layer,
        Manifest, ScalarExpression, ScalarProperty, TimeBase,
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

        let context = ExpressionContext::new(4.0, TimeBase::legacy_frames(24), &params, 7);
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
            .evaluate_with_context(&ExpressionContext::new(
                2.0,
                TimeBase::legacy_frames(24),
                &params,
                0,
            ))
            .expect_err("inverted clamp bounds should fail");
        assert!(error.to_string().contains("requires min <= max"));
    }

    #[test]
    fn expression_noise_is_deterministic() {
        let expression = parse_expression("noise1d(t * 0.1)");
        let params = std::collections::BTreeMap::new();

        let a = expression
            .evaluate_with_context(&ExpressionContext::new(
                12.0,
                TimeBase::legacy_frames(24),
                &params,
                99,
            ))
            .expect("noise should evaluate");
        let b = expression
            .evaluate_with_context(&ExpressionContext::new(
                12.0,
                TimeBase::legacy_frames(24),
                &params,
                99,
            ))
            .expect("noise should evaluate");
        let c = expression
            .evaluate_with_context(&ExpressionContext::new(
                12.0,
                TimeBase::legacy_frames(24),
                &params,
                100,
            ))
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
            .evaluate_with_context(&ExpressionContext::new(
                0.0,
                TimeBase::legacy_frames(24),
                &params,
                0,
            ))
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
    fn seconds_mapping_samples_identically_across_fps() {
        let property: ScalarProperty =
            serde_yaml::from_str("{ start_time: 0.5, end_time: 1.5, from: 0, to: 10 }")
                .expect("mapping should parse");
        property
            .validate_with_context(
                "opacity",
                &ExpressionContext::new(0.0, TimeBase::seconds(24), &Default::default(), 0),
            )
            .expect("seconds mapping should validate");
        let params = std::collections::BTreeMap::new();
        for (fps, frame) in [(24_u32, 24.0_f32), (60, 60.0), (25, 25.0)] {
            let context = ExpressionContext::new(frame, TimeBase::seconds(fps), &params, 0);
            let value = property.evaluate_with_context(&context).expect("sample");
            assert!((value - 5.0).abs() < 1e-5, "fps {fps}: got {value}");
        }
    }

    #[test]
    fn mapping_rejects_mixed_frame_and_seconds_endpoints() {
        let error = serde_yaml::from_str::<ScalarProperty>(
            "{ start_frame: 0, end_time: 1.5, from: 0, to: 10 }",
        )
        .expect_err("mixed units must be rejected");
        assert!(error.to_string().contains("not a mix"), "{error}");
    }

    fn sample_scalar(property: &ScalarProperty, frame: f32, time_base: TimeBase) -> f32 {
        let params = std::collections::BTreeMap::new();
        property
            .evaluate_with_context(&ExpressionContext::new(frame, time_base, &params, 0))
            .expect("sample")
    }

    fn probe_scalar(property: &ScalarProperty) -> anyhow::Result<()> {
        let params = std::collections::BTreeMap::new();
        property.validate_with_context(
            "opacity",
            &ExpressionContext::new(0.0, TimeBase::seconds(24), &params, 0),
        )
    }

    #[test]
    fn keyframe_track_samples_segments_with_per_key_easing() {
        let property: ScalarProperty = serde_yaml::from_str(
            r#"
keyframes:
  - { time: 1.0, value: 10 }
  - { time: 2.0, value: 20, easing: hold }
  - { time: 3.0, value: 0, easing: ease_in }
  - { time: 4.0, value: 100 }
"#,
        )
        .expect("track should parse");
        probe_scalar(&property).expect("track should validate");
        let tb = TimeBase::seconds(10);
        let at = |seconds: f32| sample_scalar(&property, seconds * 10.0, tb);

        assert_eq!(at(0.0), 10.0, "before first key holds first value");
        assert!((at(1.5) - 15.0).abs() < 1e-5, "linear segment");
        assert_eq!(at(2.0), 20.0);
        assert_eq!(at(2.9), 20.0, "hold keeps value until next key");
        assert_eq!(at(3.0), 0.0, "hold switches exactly at the next key");
        assert!((at(3.5) - 25.0).abs() < 1e-4, "ease_in: 0.5^2 * 100");
        assert_eq!(at(9.0), 100.0, "after last key holds last value");
    }

    #[test]
    fn frame_keyed_track_is_fps_dependent_and_seconds_track_is_not() {
        let frames: ScalarProperty = serde_yaml::from_str(
            "{ keyframes: [ { frame: 0, value: 0 }, { frame: 24, value: 1 } ] }",
        )
        .expect("frame track");
        let seconds: ScalarProperty =
            serde_yaml::from_str("{ keyframes: [ { time: 0, value: 0 }, { time: 1, value: 1 } ] }")
                .expect("seconds track");
        // Half a second in at 60fps is frame 30.
        assert_eq!(sample_scalar(&frames, 30.0, TimeBase::seconds(60)), 1.0);
        assert!((sample_scalar(&seconds, 30.0, TimeBase::seconds(60)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn legacy_mapping_is_sugar_for_two_key_track() {
        let legacy: ScalarProperty = serde_yaml::from_str(
            "{ start_frame: 6, end_frame: 30, from: -3, to: 17.5, easing: ease_in_out }",
        )
        .expect("legacy mapping");
        let track: ScalarProperty = serde_yaml::from_str(
            r#"
keyframes:
  - { frame: 6, value: -3, easing: ease_in_out }
  - { frame: 30, value: 17.5 }
"#,
        )
        .expect("track");
        let tb = TimeBase::legacy_frames(24);
        for step in 0..=80 {
            let frame = step as f32 * 0.5;
            assert_eq!(
                sample_scalar(&legacy, frame, tb).to_bits(),
                sample_scalar(&track, frame, tb).to_bits(),
                "frame {frame}"
            );
        }
    }

    #[test]
    fn cubic_bezier_easing_matches_reference_points() {
        use super::EasingCurve;
        let linear = EasingCurve::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        let ease_in_out = EasingCurve::CubicBezier {
            x1: 0.42,
            y1: 0.0,
            x2: 0.58,
            y2: 1.0,
        };
        for t in [0.0, 0.1, 0.25, 0.5, 0.8, 1.0] {
            assert!((linear.apply(t) - t).abs() < 1e-4, "linear bezier at {t}");
        }
        assert!((ease_in_out.apply(0.5) - 0.5).abs() < 1e-4);
        assert!(ease_in_out.apply(0.25) < 0.25, "slow start");
        assert!(ease_in_out.apply(0.75) > 0.75, "slow end");
        // Symmetric curve: f(t) + f(1 - t) == 1.
        for t in [0.1_f32, 0.3, 0.45] {
            assert!((ease_in_out.apply(t) + ease_in_out.apply(1.0 - t) - 1.0).abs() < 1e-4);
        }

        let parsed: EasingCurve = serde_yaml::from_str("[0.42, 0, 0.58, 1]").expect("array");
        assert_eq!(parsed, ease_in_out);
        let parsed: EasingCurve =
            serde_yaml::from_str("{ cubic_bezier: [0.42, 0, 0.58, 1] }").expect("object");
        assert_eq!(parsed, ease_in_out);
        assert_eq!(
            serde_yaml::from_str::<EasingCurve>("step").expect("alias"),
            EasingCurve::Hold
        );
    }

    #[test]
    fn keyframe_track_validation_errors_are_explicit() {
        let cases = [
            (
                "{ keyframes: [ { time: 1, value: 0 }, { time: 1, value: 1 } ] }",
                "strictly increasing",
            ),
            (
                "{ keyframes: [ { time: 0, value: 0 }, { frame: 30, value: 1 } ] }",
                "same unit",
            ),
            (
                "{ keyframes: [ { time: 0, value: 0, easing: [1.5, 0, 0.5, 1] }, { time: 1, value: 1 } ] }",
                "x1 and x2 must be within [0, 1]",
            ),
        ];
        for (yaml, expected) in cases {
            let property: ScalarProperty = serde_yaml::from_str(yaml).expect("shape parses");
            let error = probe_scalar(&property).expect_err(yaml);
            assert!(error.to_string().contains(expected), "{yaml}: {error}");
        }

        let parse_cases = [
            (
                "{ keyframes: [ { time: 0, frame: 0, value: 0 } ] }",
                "exactly one of `time` (seconds) or `frame`",
            ),
            ("{ keyframes: [] }", "at least one key"),
            (
                "{ keyframes: [ { time: 0, value: 0, easing: bounce } ] }",
                "unknown easing 'bounce'",
            ),
            ("\"sin(t\"", "unterminated function call for 'sin'"),
        ];
        for (yaml, expected) in parse_cases {
            let error = serde_yaml::from_str::<ScalarProperty>(yaml).expect_err(yaml);
            assert!(error.to_string().contains(expected), "{yaml}: {error}");
        }
    }

    #[test]
    fn color_keyframes_interpolate_whole_colors() {
        use super::{AnimatableColor, ColorRgba};
        let color: AnimatableColor = serde_yaml::from_str(
            r#"
keyframes:
  - { time: 0, value: { r: 1, g: 0, b: 0 } }
  - { time: 1, value: { r: 0, g: 0, b: 1, a: 0.5 } }
"#,
        )
        .expect("color track");
        assert!(!color.is_static());
        let params = std::collections::BTreeMap::new();
        let mid = color
            .evaluate(&ExpressionContext::new(
                12.0,
                TimeBase::seconds(24),
                &params,
                0,
            ))
            .expect("evaluate");
        assert_eq!(
            mid,
            ColorRgba {
                r: 0.5,
                g: 0.0,
                b: 0.5,
                a: 0.75
            }
        );
    }

    #[test]
    fn legacy_params_named_like_time_builtins_keep_their_value() {
        let expression = parse_expression("fps + frame");
        let mut params = std::collections::BTreeMap::new();
        params.insert("fps".to_owned(), 1.0);
        let value = expression
            .evaluate_with_context(&ExpressionContext::new(
                10.0,
                TimeBase::legacy_frames(24),
                &params,
                0,
            ))
            .expect("expression should evaluate");
        // param fps (1) wins over builtin fps (24); frame falls back to the builtin (10).
        assert!((value - 11.0).abs() < 1e-6);
    }

    #[test]
    fn env_default_attack_decay_follow_time_unit() {
        let expression = parse_expression("env(t)");
        let params = std::collections::BTreeMap::new();
        // Legacy: attack 12 frames -> half way at frame 6.
        let legacy = expression
            .evaluate_with_context(&ExpressionContext::new(
                6.0,
                TimeBase::legacy_frames(24),
                &params,
                0,
            ))
            .expect("legacy env");
        // Seconds: attack 0.5s -> half way at 0.25s, i.e. frame 15 at 60fps.
        let seconds = expression
            .evaluate_with_context(&ExpressionContext::new(
                15.0,
                TimeBase::seconds(60),
                &params,
                0,
            ))
            .expect("seconds env");
        assert!((legacy - 0.5).abs() < 1e-6);
        assert!((seconds - 0.5).abs() < 1e-6);
    }

    #[test]
    fn scalar_property_expression_uses_context() {
        let property = ScalarProperty::Expression(parse_expression("energy * cos(t)"));
        let mut params = std::collections::BTreeMap::new();
        params.insert("energy".to_owned(), 2.0);

        let value = property
            .evaluate_with_context(&ExpressionContext::new(
                0.0,
                TimeBase::legacy_frames(24),
                &params,
                0,
            ))
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
            .validate(&manifest.probe_context(), &manifest.modulators)
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
}
