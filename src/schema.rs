use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use serde::{de::Error as DeError, Deserialize, Deserializer};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub environment: Environment,
    pub layers: Vec<Layer>,
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
            }
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerCommon {
    pub id: String,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default)]
    pub position: PropertyValue<Vec2>,
    #[serde(default = "default_scale")]
    pub scale: PropertyValue<Vec2>,
    #[serde(default)]
    pub rotation_degrees: ScalarProperty,
    #[serde(default = "default_opacity_property")]
    pub opacity: ScalarProperty,
}

impl LayerCommon {
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("layer id cannot be empty");
        }

        self.position
            .validate("position")
            .map_err(|error| anyhow::anyhow!("layer '{}': {error}", self.id))?;
        self.scale
            .validate("scale")
            .map_err(|error| anyhow::anyhow!("layer '{}': {error}", self.id))?;
        self.rotation_degrees
            .validate("rotation_degrees")
            .map_err(|error| anyhow::anyhow!("layer '{}': {error}", self.id))?;
        self.opacity
            .validate("opacity")
            .map_err(|error| anyhow::anyhow!("layer '{}': {error}", self.id))?;

        Ok(())
    }

    pub fn has_static_properties(&self) -> bool {
        self.position.is_static()
            && self.scale.is_static()
            && self.rotation_degrees.is_static()
            && self.opacity.is_static()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Layer {
    Asset(AssetLayer),
    Procedural(ProceduralLayer),
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
            Self::Procedural(layer) => &layer.common,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.common().validate()?;
        match self {
            Self::Asset(layer) => layer.validate(),
            Self::Procedural(layer) => layer.validate(),
        }
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
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralLayer {
    #[serde(flatten)]
    pub common: LayerCommon,
    pub procedural: ProceduralSource,
}

impl ProceduralLayer {
    fn validate(&self) -> Result<()> {
        self.procedural
            .validate()
            .map_err(|error| anyhow::anyhow!("layer '{}': {error}", self.common.id))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProceduralSource {
    SolidColor {
        color: ColorRgba,
    },
    Gradient {
        start_color: ColorRgba,
        end_color: ColorRgba,
        #[serde(default)]
        direction: GradientDirection,
    },
}

impl ProceduralSource {
    fn validate(&self) -> Result<()> {
        match self {
            Self::SolidColor { color } => color.validate("color"),
            Self::Gradient {
                start_color,
                end_color,
                ..
            } => {
                start_color.validate("start_color")?;
                end_color.validate("end_color")
            }
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

#[derive(Debug, Clone, Copy, Deserialize)]
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

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue<T> {
    Static(T),
    Mapping(KeyValue<T>),
}

impl<T: Clone + Interpolate> PropertyValue<T> {
    pub fn sample(&self, frame: u32) -> T {
        match self {
            Self::Static(value) => value.clone(),
            Self::Mapping(mapping) => mapping.sample(frame),
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
    pub fn evaluate(&self, t: u32) -> Result<f32> {
        match self {
            Self::Static(value) => Ok(*value),
            Self::Mapping(mapping) => Ok(mapping.sample(t)),
            Self::Expression(expression) => expression.evaluate(t),
        }
    }

    pub fn validate(&self, label: &str) -> Result<()> {
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
                let probe = expression.evaluate(0)?;
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
    pub fn evaluate(&self, t: u32) -> Result<f32> {
        let value = self
            .ast
            .evaluate(t as f32)
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
    Frame,
    UnaryNeg(Box<ExpressionNode>),
    Add(Box<ExpressionNode>, Box<ExpressionNode>),
    Sub(Box<ExpressionNode>, Box<ExpressionNode>),
    Mul(Box<ExpressionNode>, Box<ExpressionNode>),
    Div(Box<ExpressionNode>, Box<ExpressionNode>),
    Mod(Box<ExpressionNode>, Box<ExpressionNode>),
    Pow(Box<ExpressionNode>, Box<ExpressionNode>),
}

impl ExpressionNode {
    fn evaluate(&self, t: f32) -> Result<f32> {
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Frame => Ok(t),
            Self::UnaryNeg(value) => Ok(-value.evaluate(t)?),
            Self::Add(left, right) => Ok(left.evaluate(t)? + right.evaluate(t)?),
            Self::Sub(left, right) => Ok(left.evaluate(t)? - right.evaluate(t)?),
            Self::Mul(left, right) => Ok(left.evaluate(t)? * right.evaluate(t)?),
            Self::Div(left, right) => {
                let divisor = right.evaluate(t)?;
                if divisor.abs() <= f32::EPSILON {
                    bail!("expression attempted division by zero");
                }
                Ok(left.evaluate(t)? / divisor)
            }
            Self::Mod(left, right) => {
                let divisor = right.evaluate(t)?;
                if divisor.abs() <= f32::EPSILON {
                    bail!("expression attempted modulo by zero");
                }
                Ok(left.evaluate(t)? % divisor)
            }
            Self::Pow(left, right) => {
                let value = left.evaluate(t)?.powf(right.evaluate(t)?);
                validate_number("pow result", value)?;
                Ok(value)
            }
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
            Some('a'..='z') | Some('A'..='Z') | Some('_') => self.parse_identifier(),
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

    fn parse_identifier(&mut self) -> Result<ExpressionNode> {
        let start = self.index;
        while matches!(
            self.peek_char(),
            Some('a'..='z') | Some('A'..='Z') | Some('_') | Some('0'..='9')
        ) {
            self.index += 1;
        }
        let identifier = &self.source[start..self.index];
        match identifier {
            "t" => Ok(ExpressionNode::Frame),
            _ => bail!("unsupported variable '{identifier}', only 't' is allowed"),
        }
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
    pub fn sample(&self, frame: u32) -> T {
        if frame <= self.start_frame {
            return self.from.clone();
        }
        if frame >= self.end_frame {
            return self.to.clone();
        }

        let span = (self.end_frame - self.start_frame) as f32;
        let progress = (frame - self.start_frame) as f32 / span;
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
    fn apply(self, t: f32) -> f32 {
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

fn default_scale() -> PropertyValue<Vec2> {
    PropertyValue::Static(Vec2 { x: 1.0, y: 1.0 })
}

fn default_opacity_property() -> ScalarProperty {
    ScalarProperty::Static(1.0)
}

fn validate_number(label: &str, value: f32) -> Result<()> {
    if !value.is_finite() {
        bail!("{label} must be finite");
    }
    Ok(())
}
