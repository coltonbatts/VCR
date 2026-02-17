use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_yaml::{Mapping, Value};

use crate::asset_catalog::{parse_asset_reference, resolve_manifest_asset_reference};
use crate::library::ManifestSourceUsage;
use crate::schema::{
    validate_manifest_manifest_level, ColorRgba, Layer, Manifest, ParamDefinition, ParamType,
    ParamValue, Parameters, Vec2,
};

#[derive(Debug, Clone, Default)]
pub struct ManifestLoadOptions {
    pub overrides: Vec<ParamOverride>,
    pub allow_raw_paths: bool,
}

#[derive(Debug, Clone)]
pub struct ParamOverride {
    pub name: String,
    pub value: String,
}

impl ParamOverride {
    pub fn parse(raw: &str) -> Result<Self> {
        let (name, value) = raw
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid --set '{raw}': expected NAME=VALUE"))?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            bail!("invalid --set '{raw}': parameter name cannot be empty");
        }
        if !valid_identifier(name) {
            bail!(
                "invalid --set '{raw}': parameter name must be an identifier like speed or glow_2"
            );
        }
        if value.is_empty() {
            bail!("invalid --set '{raw}': parameter value cannot be empty");
        }
        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }
}

pub fn load_and_validate_manifest(path: &Path) -> Result<Manifest> {
    load_and_validate_manifest_with_options(path, &ManifestLoadOptions::default())
}

pub fn load_and_validate_manifest_with_options(
    path: &Path,
    options: &ManifestLoadOptions,
) -> Result<Manifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let mut manifest_value = parse_yaml_value(path, &contents)?;
    normalize_layer_source_shorthand(&mut manifest_value)?;
    apply_trailer_encoding_defaults(path, &mut manifest_value)?;
    let param_definitions = parse_param_definitions(&manifest_value)?;
    validate_param_definitions(&param_definitions)?;

    let (resolved_params, applied_overrides) =
        resolve_params_with_overrides(&param_definitions, &options.overrides)?;
    substitute_param_references(&mut manifest_value, &resolved_params)?;
    inject_numeric_params_for_expressions(&mut manifest_value, &resolved_params)?;

    let mut manifest: Manifest = serde_yaml::from_value(manifest_value)
        .with_context(|| format!("failed to decode manifest {}", path.display()))?;
    manifest.param_definitions = param_definitions;
    manifest.resolved_params = resolved_params;
    manifest.applied_param_overrides = applied_overrides;
    manifest.manifest_hash = compute_resolved_manifest_hash(
        &contents,
        &manifest.resolved_params,
        &manifest.applied_param_overrides,
    )?;

    validate_manifest(&mut manifest, path, options)?;
    Ok(manifest)
}

fn parse_yaml_value(path: &Path, contents: &str) -> Result<Value> {
    serde_yaml::from_str(contents).map_err(|error| {
        let location = error
            .location()
            .map(|location| format!("line {}, column {}", location.line(), location.column()))
            .unwrap_or_else(|| "unknown location".to_owned());
        anyhow!(
            "failed to parse yaml in {} at {}: {}",
            path.display(),
            location,
            error
        )
    })
}

fn key(name: &str) -> Value {
    Value::String(name.to_owned())
}

fn normalize_layer_source_shorthand(root: &mut Value) -> Result<()> {
    let Some(root_map) = root.as_mapping_mut() else {
        return Ok(());
    };
    let Some(layers_value) = root_map.get_mut(key("layers")) else {
        return Ok(());
    };
    let Some(layers) = layers_value.as_sequence_mut() else {
        return Ok(());
    };

    for layer_value in layers {
        let Some(layer) = layer_value.as_mapping_mut() else {
            continue;
        };
        let Some(source_value) = layer.remove(key("source")) else {
            continue;
        };
        let source = parse_source_reference_value(&source_value)?;
        let layer_id = layer
            .get(key("id"))
            .and_then(Value::as_str)
            .unwrap_or("<unknown>")
            .to_owned();

        if layer.contains_key(key("source_path")) {
            let source_path_key = key("source_path");
            if layer
                .get(&source_path_key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .is_some()
            {
                bail!(
                    "layer '{}' defines both source and source_path; keep only one",
                    layer_id
                );
            }
            layer.insert(source_path_key, Value::String(source));
            continue;
        }

        if layer.contains_key(key("image")) {
            set_nested_path(layer, "image", "path", &source, &layer_id)?;
            continue;
        }
        if layer.contains_key(key("ascii")) {
            set_nested_path(layer, "ascii", "path", &source, &layer_id)?;
            continue;
        }
        if layer.contains_key(key("sequence")) {
            set_nested_path(layer, "sequence", "path", &source, &layer_id)?;
            continue;
        }

        for incompatible in ["procedural", "shader", "wgpu_shader", "text"] {
            if layer.contains_key(key(incompatible)) {
                bail!(
                    "layer '{}' uses source shorthand but source block '{}' does not support external assets",
                    layer_id,
                    incompatible
                );
            }
        }

        let mut image = Mapping::new();
        image.insert(key("path"), Value::String(source));
        layer.insert(key("image"), Value::Mapping(image));
    }

    Ok(())
}

fn set_nested_path(
    layer: &mut Mapping,
    block_name: &str,
    field_name: &str,
    source: &str,
    layer_id: &str,
) -> Result<()> {
    let block_key = key(block_name);
    let Some(block_value) = layer.get_mut(&block_key) else {
        bail!("layer '{}' missing '{}' block", layer_id, block_name);
    };
    let Some(block) = block_value.as_mapping_mut() else {
        bail!(
            "layer '{}' {} block must be a mapping",
            layer_id,
            block_name
        );
    };
    let field_key = key(field_name);
    if block
        .get(&field_key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        bail!(
            "layer '{}' defines both source and {}.{}; keep only one",
            layer_id,
            block_name,
            field_name
        );
    }
    block.insert(field_key, Value::String(source.to_owned()));
    Ok(())
}

fn parse_source_reference_value(source: &Value) -> Result<String> {
    match source {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                bail!("source cannot be empty");
            }
            Ok(trimmed.to_owned())
        }
        Value::Mapping(map) => {
            let kind = map
                .get(key("kind"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("source.kind must be a string"))?;
            match kind {
                "library" => {
                    let id = map
                        .get(key("id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow!("source.kind=library requires source.id"))?;
                    let id = id.trim();
                    if id.is_empty() {
                        bail!("source.id cannot be empty");
                    }
                    Ok(format!("library:{id}"))
                }
                "pack" => {
                    let pack_id = map
                        .get(key("pack"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow!("source.kind=pack requires source.pack"))?;
                    let asset_id = map
                        .get(key("id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow!("source.kind=pack requires source.id"))?;
                    let pack_id = pack_id.trim();
                    let asset_id = asset_id.trim();
                    if pack_id.is_empty() {
                        bail!("source.pack cannot be empty");
                    }
                    if asset_id.is_empty() {
                        bail!("source.id cannot be empty");
                    }
                    Ok(format!("pack:{pack_id}/{asset_id}"))
                }
                _ => bail!(
                    "unsupported source.kind '{}': expected 'library' or 'pack'",
                    kind
                ),
            }
        }
        _ => bail!(
            "source must be a string like 'library:<id>' or 'pack:<pack-id>/<asset-id>' or mapping"
        ),
    }
}

fn apply_trailer_encoding_defaults(manifest_path: &Path, root: &mut Value) -> Result<()> {
    if !is_trailer_manifest(manifest_path) {
        return Ok(());
    }
    let Some(root_map) = root.as_mapping_mut() else {
        return Ok(());
    };
    let Some(environment_value) = root_map.get_mut(key("environment")) else {
        return Ok(());
    };
    let Some(environment_map) = environment_value.as_mapping_mut() else {
        return Ok(());
    };

    let encoding_key = key("encoding");
    let mut encoding_map = if let Some(existing) = environment_map.remove(&encoding_key) {
        existing
            .as_mapping()
            .cloned()
            .ok_or_else(|| anyhow!("environment.encoding must be a mapping"))?
    } else {
        Mapping::new()
    };

    encoding_map
        .entry(key("prores_profile"))
        .or_insert_with(|| Value::String("prores4444".to_owned()));
    encoding_map
        .entry(key("vendor"))
        .or_insert_with(|| Value::String("apl0".to_owned()));
    environment_map.insert(encoding_key, Value::Mapping(encoding_map));
    Ok(())
}

fn parse_param_definitions(root: &Value) -> Result<BTreeMap<String, ParamDefinition>> {
    let Some(root_map) = root.as_mapping() else {
        bail!("manifest root must be a mapping");
    };
    let Some(params_value) = root_map.get(Value::String("params".to_owned())) else {
        return Ok(BTreeMap::new());
    };
    let Some(params_map) = params_value.as_mapping() else {
        bail!("top-level params must be a mapping");
    };

    let mut definitions = BTreeMap::new();
    for (key, value) in params_map {
        let name = key
            .as_str()
            .ok_or_else(|| anyhow!("parameter names must be strings"))?
            .trim()
            .to_owned();
        if name.is_empty() {
            bail!("parameter name cannot be empty");
        }

        let definition = if let Some(number) = value.as_f64() {
            let number = finite_f64_to_f32(number, &format!("param '{name}'"))?;
            ParamDefinition {
                param_type: ParamType::Float,
                default: ParamValue::Float(number),
                min: None,
                max: None,
                description: None,
            }
        } else if value.is_i64() || value.is_u64() {
            let integer = parse_yaml_i64(value, &format!("param '{name}'"))?;
            ParamDefinition {
                param_type: ParamType::Float,
                default: ParamValue::Float(integer as f32),
                min: None,
                max: None,
                description: None,
            }
        } else {
            parse_param_definition_object(&name, value)?
        };

        definitions.insert(name, definition);
    }

    Ok(definitions)
}

fn parse_param_definition_object(name: &str, value: &Value) -> Result<ParamDefinition> {
    let Some(map) = value.as_mapping() else {
        bail!(
            "param '{}' must be either a number (legacy) or a mapping with type/default",
            name
        );
    };
    for key in map.keys() {
        let key = key
            .as_str()
            .ok_or_else(|| anyhow!("param '{}' field names must be strings", name))?;
        if !matches!(key, "type" | "default" | "min" | "max" | "description") {
            bail!("param '{}' has unknown field '{}'", name, key);
        }
    }
    let param_type = parse_param_type(
        get_required_field(map, "type", name)?
            .as_str()
            .ok_or_else(|| anyhow!("param '{}.type' must be a string", name))?,
    )?;
    if let Some(default_value) = map.get(Value::String("default".to_owned())) {
        if contains_substitution_syntax(default_value) {
            if let Some(reference) = find_param_reference(default_value) {
                bail!(
                    "param '{}' default cannot reference '${{{}}}'. Param defaults are non-recursive (max substitution depth is 1 in manifest fields only).",
                    name,
                    reference
                );
            }
            bail!(
                "param '{}' default cannot contain substitution syntax ('${{...}}'). Param defaults are non-recursive.",
                name
            );
        }
    }
    let default_value = parse_value_for_param_type(
        &param_type,
        get_required_field(map, "default", name)?,
        &format!("param '{name}'.default"),
    )?;

    let min = get_optional_f32_field(map, "min", name)?;
    let max = get_optional_f32_field(map, "max", name)?;
    let description = get_optional_string_field(map, "description", name)?;

    if !matches!(param_type, ParamType::Float | ParamType::Int) && (min.is_some() || max.is_some())
    {
        bail!(
            "param '{}' of type '{}' cannot set min/max bounds",
            name,
            param_type_label(param_type)
        );
    }
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            bail!("param '{}' has min ({min}) greater than max ({max})", name);
        }
    }

    let definition = ParamDefinition {
        param_type,
        default: default_value,
        min,
        max,
        description,
    };
    validate_param_value_in_bounds(name, &definition, &definition.default)?;
    Ok(definition)
}

fn get_required_field<'a>(map: &'a Mapping, key: &str, param_name: &str) -> Result<&'a Value> {
    map.get(Value::String(key.to_owned()))
        .ok_or_else(|| anyhow!("param '{}' must define '{}'", param_name, key))
}

fn get_optional_f32_field(map: &Mapping, key: &str, param_name: &str) -> Result<Option<f32>> {
    let Some(value) = map.get(Value::String(key.to_owned())) else {
        return Ok(None);
    };
    let parsed = parse_yaml_f32(value, &format!("param '{}.{}'", param_name, key))?;
    Ok(Some(parsed))
}

fn get_optional_string_field(map: &Mapping, key: &str, param_name: &str) -> Result<Option<String>> {
    let Some(value) = map.get(Value::String(key.to_owned())) else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| anyhow!("param '{}.{}' must be a string", param_name, key))?
        .trim()
        .to_owned();
    if text.is_empty() {
        bail!("param '{}.{}' cannot be empty", param_name, key);
    }
    Ok(Some(text))
}

fn parse_param_type(raw: &str) -> Result<ParamType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "float" => Ok(ParamType::Float),
        "int" => Ok(ParamType::Int),
        "color" => Ok(ParamType::Color),
        "vec2" => Ok(ParamType::Vec2),
        "bool" => Ok(ParamType::Bool),
        _ => bail!(
            "unsupported param type '{}'. Expected one of: float, int, color, vec2, bool",
            raw
        ),
    }
}

fn parse_value_for_param_type(
    param_type: &ParamType,
    value: &Value,
    label: &str,
) -> Result<ParamValue> {
    match param_type {
        ParamType::Float => Ok(ParamValue::Float(parse_yaml_f32(value, label)?)),
        ParamType::Int => Ok(ParamValue::Int(parse_yaml_i64(value, label)?)),
        ParamType::Bool => {
            let parsed = value
                .as_bool()
                .ok_or_else(|| anyhow!("{label} must be a boolean"))?;
            Ok(ParamValue::Bool(parsed))
        }
        ParamType::Vec2 => {
            let vec = serde_yaml::from_value::<Vec2>(value.clone())
                .with_context(|| format!("{label} must be a vec2 [x, y] or {{x, y}}"))?;
            Ok(ParamValue::Vec2(vec))
        }
        ParamType::Color => {
            let color = serde_yaml::from_value::<ColorRgba>(value.clone())
                .with_context(|| format!("{label} must be a color {{r, g, b, a?}}"))?;
            color.validate(label)?;
            Ok(ParamValue::Color(color))
        }
    }
}

fn parse_yaml_f32(value: &Value, label: &str) -> Result<f32> {
    if let Some(number) = value.as_f64() {
        return finite_f64_to_f32(number, label);
    }
    if value.is_i64() || value.is_u64() {
        let integer = parse_yaml_i64(value, label)?;
        return Ok(integer as f32);
    }
    bail!("{label} must be a number");
}

fn finite_f64_to_f32(value: f64, label: &str) -> Result<f32> {
    if !value.is_finite() {
        bail!("{label} must be finite");
    }
    Ok(value as f32)
}

fn parse_yaml_i64(value: &Value, label: &str) -> Result<i64> {
    if let Some(integer) = value.as_i64() {
        return Ok(integer);
    }
    if let Some(unsigned) = value.as_u64() {
        return i64::try_from(unsigned)
            .map_err(|_| anyhow!("{label} exceeds supported integer range"));
    }
    bail!("{label} must be an integer");
}

fn validate_param_definitions(definitions: &BTreeMap<String, ParamDefinition>) -> Result<()> {
    for (name, definition) in definitions {
        if !valid_identifier(name) {
            bail!("invalid param name '{name}'. Use identifiers like energy, phase, tension_2");
        }
        if name == "t" {
            bail!("param name 't' is reserved for frame time in expressions");
        }
        validate_param_value_in_bounds(name, definition, &definition.default)?;
    }
    Ok(())
}

fn resolve_params_with_overrides(
    definitions: &BTreeMap<String, ParamDefinition>,
    overrides: &[ParamOverride],
) -> Result<(BTreeMap<String, ParamValue>, BTreeMap<String, ParamValue>)> {
    let mut resolved = definitions
        .iter()
        .map(|(name, definition)| (name.clone(), definition.default.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut applied = BTreeMap::new();
    let mut seen_override_names = HashSet::new();

    for override_entry in overrides {
        if !seen_override_names.insert(override_entry.name.clone()) {
            bail!(
                "duplicate --set override for param '{}'. Provide each param at most once.",
                override_entry.name
            );
        }
        let definition = definitions.get(&override_entry.name).ok_or_else(|| {
            anyhow!(
                "unknown parameter '{}' in --set override",
                override_entry.name
            )
        })?;

        let parsed = parse_cli_override_value(definition.param_type, &override_entry.value)
            .map_err(|error| {
                anyhow!(
                    "invalid --set for param '{}': expected {}, got '{}'. Example: --set {}={}. {}",
                    override_entry.name,
                    param_type_label(definition.param_type),
                    override_entry.value,
                    override_entry.name,
                    override_example_for_type(definition.param_type),
                    error
                )
            })?;
        validate_param_value_in_bounds(&override_entry.name, definition, &parsed)?;
        resolved.insert(override_entry.name.clone(), parsed.clone());
        applied.insert(override_entry.name.clone(), parsed);
    }

    Ok((resolved, applied))
}

fn parse_cli_override_value(param_type: ParamType, raw: &str) -> Result<ParamValue> {
    match param_type {
        ParamType::Float => {
            let parsed = raw
                .parse::<f32>()
                .map_err(|error| anyhow!("failed to parse float literal '{}': {}", raw, error))?;
            if !parsed.is_finite() {
                bail!("float must be finite");
            }
            Ok(ParamValue::Float(parsed))
        }
        ParamType::Int => {
            let parsed = raw
                .parse::<i64>()
                .map_err(|error| anyhow!("failed to parse int literal '{}': {}", raw, error))?;
            Ok(ParamValue::Int(parsed))
        }
        ParamType::Bool => {
            let normalized = raw.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "true" | "1" => Ok(ParamValue::Bool(true)),
                "false" | "0" => Ok(ParamValue::Bool(false)),
                _ => bail!("boolean must be true/false/1/0"),
            }
        }
        ParamType::Vec2 => parse_cli_vec2(raw).map(ParamValue::Vec2),
        ParamType::Color => parse_cli_color(raw).map(ParamValue::Color),
    }
}

fn parse_cli_vec2(raw: &str) -> Result<Vec2> {
    if raw.contains(';') {
        bail!("vec2 must use ',' as the delimiter, not ';'");
    }
    if !raw.contains(',') {
        bail!("vec2 must be comma-delimited as x,y; whitespace-only separators are not supported");
    }

    let values = raw.split(',').map(str::trim).collect::<Vec<_>>();
    if values.len() != 2 {
        bail!("vec2 must be comma-delimited as x,y");
    }
    if values[0].is_empty() || values[1].is_empty() {
        bail!("vec2 must include both x and y values (example: 120,-45)");
    }
    let x = values[0]
        .parse::<f32>()
        .map_err(|error| anyhow!("invalid vec2 x '{}': {}", values[0], error))?;
    let y = values[1]
        .parse::<f32>()
        .map_err(|error| anyhow!("invalid vec2 y '{}': {}", values[1], error))?;
    if !x.is_finite() || !y.is_finite() {
        bail!("vec2 values must be finite");
    }
    Ok(Vec2 { x, y })
}

fn parse_cli_color(raw: &str) -> Result<ColorRgba> {
    if let Some(hex) = raw.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if values.len() != 3 && values.len() != 4 {
        bail!("color must be #RRGGBB, #RRGGBBAA, or r,g,b[,a]");
    }

    let mut parsed = [0.0_f32; 4];
    for (index, value) in values.iter().enumerate() {
        parsed[index] = value
            .parse::<f32>()
            .map_err(|error| anyhow!("invalid color channel '{}': {}", value, error))?;
    }
    if values.len() == 3 {
        parsed[3] = 1.0;
    }

    let color = ColorRgba {
        r: parsed[0],
        g: parsed[1],
        b: parsed[2],
        a: parsed[3],
    };
    color.validate("color override")?;
    Ok(color)
}

fn parse_hex_color(hex: &str) -> Result<ColorRgba> {
    let bytes = match hex.len() {
        6 => {
            let r = parse_hex_byte(&hex[0..2])?;
            let g = parse_hex_byte(&hex[2..4])?;
            let b = parse_hex_byte(&hex[4..6])?;
            [r, g, b, 255]
        }
        8 => {
            let r = parse_hex_byte(&hex[0..2])?;
            let g = parse_hex_byte(&hex[2..4])?;
            let b = parse_hex_byte(&hex[4..6])?;
            let a = parse_hex_byte(&hex[6..8])?;
            [r, g, b, a]
        }
        _ => bail!("hex color must be 6 or 8 digits, got '{}'", hex),
    };

    Ok(ColorRgba {
        r: bytes[0] as f32 / 255.0,
        g: bytes[1] as f32 / 255.0,
        b: bytes[2] as f32 / 255.0,
        a: bytes[3] as f32 / 255.0,
    })
}

fn parse_hex_byte(segment: &str) -> Result<u8> {
    u8::from_str_radix(segment, 16).map_err(|error| anyhow!("invalid hex '{}': {error}", segment))
}

fn validate_param_value_in_bounds(
    name: &str,
    definition: &ParamDefinition,
    value: &ParamValue,
) -> Result<()> {
    let Some(as_number) = value.as_expression_scalar() else {
        return Ok(());
    };
    if let Some(min) = definition.min {
        if as_number < min {
            bail!("param '{}' value {} is below min {}", name, as_number, min);
        }
    }
    if let Some(max) = definition.max {
        if as_number > max {
            bail!("param '{}' value {} is above max {}", name, as_number, max);
        }
    }
    Ok(())
}

fn inject_numeric_params_for_expressions(
    root: &mut Value,
    resolved_params: &BTreeMap<String, ParamValue>,
) -> Result<()> {
    let Some(root_map) = root.as_mapping_mut() else {
        bail!("manifest root must be a mapping");
    };
    let numeric_params = resolved_params
        .iter()
        .filter_map(|(name, value)| {
            value
                .as_expression_scalar()
                .map(|numeric| (name.clone(), numeric))
        })
        .collect::<Parameters>();
    root_map.insert(
        Value::String("params".to_owned()),
        serde_yaml::to_value(numeric_params).context("failed to encode numeric params")?,
    );
    Ok(())
}

fn substitute_param_references(
    root: &mut Value,
    resolved_params: &BTreeMap<String, ParamValue>,
) -> Result<()> {
    let Some(root_map) = root.as_mapping_mut() else {
        bail!("manifest root must be a mapping");
    };

    for (key, value) in root_map {
        if key.as_str() == Some("params") {
            continue;
        }
        substitute_value(value, resolved_params)?;
    }
    Ok(())
}

fn substitute_value(
    value: &mut Value,
    resolved_params: &BTreeMap<String, ParamValue>,
) -> Result<()> {
    match value {
        Value::String(text) => {
            match parse_substitution_token(text) {
                SubstitutionToken::Reference(reference_name) => {
                    let resolved = resolved_params.get(reference_name).ok_or_else(|| {
                        anyhow!(
                            "unknown parameter reference '${{{reference_name}}}'. Use '$${{{reference_name}}}' for a literal string."
                        )
                    })?;
                    *value = serde_yaml::to_value(resolved)
                        .with_context(|| format!("failed encoding parameter '{reference_name}'"))?;
                }
                SubstitutionToken::EscapedLiteral(reference_name) => {
                    *value = Value::String(format!("${{{reference_name}}}"));
                }
                SubstitutionToken::ContainsInterpolationSyntax => {
                    bail!(
                        "invalid substitution string '{}': only whole-string tokens like '${{speed}}' are supported. Use '$${{speed}}' for a literal.",
                        text
                    );
                }
                SubstitutionToken::None => {}
            }
            Ok(())
        }
        Value::Sequence(items) => {
            for item in items {
                substitute_value(item, resolved_params)?;
            }
            Ok(())
        }
        Value::Mapping(map) => {
            for (_, map_value) in map {
                substitute_value(map_value, resolved_params)?;
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Tagged(_) => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubstitutionToken<'a> {
    None,
    ContainsInterpolationSyntax,
    EscapedLiteral(&'a str),
    Reference(&'a str),
}

fn parse_substitution_token(value: &str) -> SubstitutionToken<'_> {
    let trimmed = value.trim();
    if let Some(inner) = trimmed
        .strip_prefix("$${")
        .and_then(|v| v.strip_suffix('}'))
    {
        if !inner.is_empty() {
            return SubstitutionToken::EscapedLiteral(inner);
        }
    }
    if let Some(inner) = trimmed.strip_prefix("${").and_then(|v| v.strip_suffix('}')) {
        if !inner.is_empty() {
            return SubstitutionToken::Reference(inner);
        }
    }
    if trimmed.contains("${") {
        return SubstitutionToken::ContainsInterpolationSyntax;
    }
    SubstitutionToken::None
}

fn find_param_reference(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => match parse_substitution_token(text) {
            SubstitutionToken::Reference(reference) => Some(reference.to_owned()),
            SubstitutionToken::EscapedLiteral(_)
            | SubstitutionToken::ContainsInterpolationSyntax
            | SubstitutionToken::None => None,
        },
        Value::Sequence(items) => items.iter().find_map(find_param_reference),
        Value::Mapping(map) => map.values().find_map(find_param_reference),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Tagged(_) => None,
    }
}

fn contains_substitution_syntax(value: &Value) -> bool {
    match value {
        Value::String(text) => !matches!(parse_substitution_token(text), SubstitutionToken::None),
        Value::Sequence(items) => items.iter().any(contains_substitution_syntax),
        Value::Mapping(map) => map.values().any(contains_substitution_syntax),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Tagged(_) => false,
    }
}

fn param_type_label(param_type: ParamType) -> &'static str {
    match param_type {
        ParamType::Float => "float",
        ParamType::Int => "int",
        ParamType::Color => "color",
        ParamType::Vec2 => "vec2",
        ParamType::Bool => "bool",
    }
}

fn override_example_for_type(param_type: ParamType) -> &'static str {
    match param_type {
        ParamType::Float => "1.25",
        ParamType::Int => "3",
        ParamType::Color => "#66CCFFAA",
        ParamType::Vec2 => "120,-45",
        ParamType::Bool => "true",
    }
}

fn validate_manifest(
    manifest: &mut Manifest,
    manifest_path: &Path,
    options: &ManifestLoadOptions,
) -> Result<()> {
    manifest.environment.validate()?;
    validate_manifest_manifest_level(manifest)?;

    if manifest.layers.is_empty() {
        bail!("manifest must define at least one layer");
    }

    let manifest_dir = manifest_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut seen_ids = HashSet::with_capacity(manifest.layers.len());
    let known_groups = manifest
        .groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<HashSet<_>>();

    for layer in &mut manifest.layers {
        layer
            .validate(&manifest.params, manifest.seed, &manifest.modulators)
            .with_context(|| format!("failed validating layer '{}'", layer.id()))?;

        if !seen_ids.insert(layer.id().to_owned()) {
            bail!("duplicate layer id '{}'", layer.id());
        }

        if let Some(group) = layer.common().group.as_deref() {
            if !known_groups.contains(group) {
                bail!(
                    "layer '{}' references unknown group '{}'. Define it in top-level groups",
                    layer.id(),
                    group
                );
            }
        }

        match layer {
            Layer::Asset(asset_layer) => {
                let resolved = resolve_and_validate_layer_asset_path(
                    manifest_path,
                    &manifest_dir,
                    &asset_layer.source_path,
                    &asset_layer.common.id,
                    "source_path",
                    ManifestSourceUsage::Image,
                    options.allow_raw_paths,
                )?;
                asset_layer.source_path = resolved;
            }
            Layer::Image(image_layer) => {
                let resolved = resolve_and_validate_layer_asset_path(
                    manifest_path,
                    &manifest_dir,
                    &image_layer.image.path,
                    &image_layer.common.id,
                    "image.path",
                    ManifestSourceUsage::Image,
                    options.allow_raw_paths,
                )?;
                image_layer.image.path = resolved;
            }
            Layer::Procedural(_) => {}
            Layer::Shader(shader_layer) => {
                if let Some(path) = &shader_layer.shader.path {
                    let resolved = resolve_and_validate_asset_path(
                        &manifest_dir,
                        path,
                        &shader_layer.common.id,
                        "shader.path",
                    )?;
                    shader_layer.shader.path = Some(resolved);
                }
            }
            Layer::WgpuShader(wgpu_shader_layer) => {
                let resolved = resolve_and_validate_asset_path(
                    &manifest_dir,
                    &wgpu_shader_layer.wgpu_shader.shader_path,
                    &wgpu_shader_layer.common.id,
                    "wgpu_shader.shader_path",
                )?;
                wgpu_shader_layer.wgpu_shader.shader_path = resolved;
            }
            Layer::Text(_) => {}
            Layer::Ascii(ascii_layer) => {
                if let Some(path) = &ascii_layer.ascii.path {
                    let resolved = resolve_and_validate_layer_asset_path(
                        manifest_path,
                        &manifest_dir,
                        path,
                        &ascii_layer.common.id,
                        "ascii.path",
                        ManifestSourceUsage::Ascii,
                        options.allow_raw_paths,
                    )?;
                    ascii_layer.ascii.path = Some(resolved);
                }
                ascii_layer.validate_content_source().with_context(|| {
                    format!("layer '{}': invalid ascii source", ascii_layer.common.id)
                })?;
            }
            Layer::Sequence(seq_layer) => {
                let resolved_dir = if is_asset_reference_path(&seq_layer.sequence.path) {
                    resolve_and_validate_layer_asset_path(
                        manifest_path,
                        &manifest_dir,
                        &seq_layer.sequence.path,
                        &seq_layer.common.id,
                        "sequence.path",
                        ManifestSourceUsage::Sequence,
                        options.allow_raw_paths,
                    )?
                } else if seq_layer.sequence.path.is_relative() {
                    if is_trailer_manifest(manifest_path) && !options.allow_raw_paths {
                        bail!(
                            "layer '{}' sequence.path must use library:<id> or pack:<pack-id>/<asset-id> in manifests/trailer (raw path '{}' is not allowed). Use --allow-raw-path-sources to override.",
                            seq_layer.common.id,
                            seq_layer.sequence.path.display()
                        );
                    }
                    manifest_dir.join(&seq_layer.sequence.path)
                } else {
                    if is_trailer_manifest(manifest_path) && !options.allow_raw_paths {
                        bail!(
                            "layer '{}' sequence.path must use library:<id> or pack:<pack-id>/<asset-id> in manifests/trailer (raw path '{}' is not allowed). Use --allow-raw-path-sources to override.",
                            seq_layer.common.id,
                            seq_layer.sequence.path.display()
                        );
                    }
                    seq_layer.sequence.path.clone()
                };
                if !resolved_dir.is_dir() {
                    bail!(
                        "layer '{}' sequence.path '{}' is not a directory",
                        seq_layer.common.id,
                        resolved_dir.display(),
                    );
                }
                // Validate frame 0 exists
                let frame0 = seq_layer.sequence.frame_path(0);
                let resolved_frame0 = if frame0.is_relative() {
                    manifest_dir.join(&frame0)
                } else {
                    frame0.clone()
                };
                if !resolved_frame0.is_file() {
                    bail!(
                        "layer '{}' sequence frame 0 not found at '{}'",
                        seq_layer.common.id,
                        resolved_frame0.display(),
                    );
                }
                seq_layer.sequence.path = resolved_dir;
            }
            Layer::Lottie(lottie_layer) => {
                let resolved = resolve_and_validate_layer_asset_path(
                    manifest_path,
                    &manifest_dir,
                    &lottie_layer.lottie.path,
                    &lottie_layer.common.id,
                    "lottie.path",
                    ManifestSourceUsage::Lottie,
                    options.allow_raw_paths,
                )?;
                lottie_layer.lottie.path = resolved;
            }
        }
    }

    manifest.layers.sort_by_key(Layer::z_index);
    Ok(())
}

fn resolve_and_validate_asset_path(
    manifest_dir: &Path,
    source_path: &Path,
    layer_id: &str,
    field_name: &str,
) -> Result<PathBuf> {
    if source_path.is_absolute() {
        bail!(
            "layer '{}' {}: absolute paths are not allowed for security reasons. Use relative paths within the manifest directory. Got: {}",
            layer_id,
            field_name,
            source_path.display()
        );
    }

    let resolved = manifest_dir.join(source_path);

    if !resolved.exists() {
        bail!(
            "layer '{}' {} does not exist: {}",
            layer_id,
            field_name,
            resolved.display()
        );
    }

    let canonical_manifest_dir = fs::canonicalize(manifest_dir).with_context(|| {
        format!(
            "failed to canonicalize manifest directory {}",
            manifest_dir.display()
        )
    })?;
    let canonical_asset_path = fs::canonicalize(&resolved)
        .with_context(|| format!("failed to canonicalize asset path {}", resolved.display()))?;

    if !canonical_asset_path.starts_with(&canonical_manifest_dir) {
        bail!(
            "layer '{}' {}: security violation - asset path '{}' escapes the manifest directory '{}'",
            layer_id,
            field_name,
            source_path.display(),
            manifest_dir.display()
        );
    }

    if !canonical_asset_path.is_file() {
        bail!(
            "layer '{}' {} is not a file: {}",
            layer_id,
            field_name,
            resolved.display()
        );
    }

    Ok(resolved)
}

fn resolve_and_validate_layer_asset_path(
    manifest_path: &Path,
    manifest_dir: &Path,
    source_path: &Path,
    layer_id: &str,
    field_name: &str,
    usage: ManifestSourceUsage,
    allow_raw_paths: bool,
) -> Result<PathBuf> {
    if let Some(reference) = source_path
        .to_str()
        .filter(|value| parse_asset_reference(value).is_some())
    {
        return resolve_manifest_asset_reference(manifest_path, reference, usage).with_context(
            || {
                format!(
                    "layer '{}' {} could not resolve asset source '{}'",
                    layer_id,
                    field_name,
                    source_path.display()
                )
            },
        );
    }

    if is_trailer_manifest(manifest_path) && !allow_raw_paths {
        bail!(
            "layer '{}' {} must use library:<id> or pack:<pack-id>/<asset-id> in manifests/trailer (raw path '{}' is not allowed). Use --allow-raw-path-sources to override.",
            layer_id,
            field_name,
            source_path.display()
        );
    }

    resolve_and_validate_asset_path(manifest_dir, source_path, layer_id, field_name)
}

fn is_asset_reference_path(source_path: &Path) -> bool {
    source_path
        .to_str()
        .and_then(parse_asset_reference)
        .is_some()
}

fn is_trailer_manifest(manifest_path: &Path) -> bool {
    let components = manifest_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|pair| pair[0] == "manifests" && pair[1] == "trailer")
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

#[derive(Debug, Serialize)]
struct ResolvedManifestHashMaterial<'a> {
    manifest_content: &'a str,
    resolved_params: &'a BTreeMap<String, ParamValue>,
    overrides: &'a BTreeMap<String, ParamValue>,
}

fn compute_resolved_manifest_hash(
    manifest_content: &str,
    resolved_params: &BTreeMap<String, ParamValue>,
    overrides: &BTreeMap<String, ParamValue>,
) -> Result<String> {
    let material = ResolvedManifestHashMaterial {
        manifest_content,
        resolved_params,
        overrides,
    };
    let encoded =
        serde_json::to_vec(&material).context("failed to serialize manifest hash material")?;
    Ok(format!("{:016x}", fnv1a64(&encoded)))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{load_and_validate_manifest_with_options, ManifestLoadOptions, ParamOverride};
    use crate::asset_catalog::add_asset_to_pack;
    use crate::library::{add_asset, LibraryAddRequest, LibraryItemType};
    use crate::schema::{Layer, ProResProfile};
    use tempfile::tempdir;

    #[test]
    fn typed_params_resolve_and_substitute() {
        let dir = tempdir().expect("tempdir should create");
        let manifest_path = dir.path().join("scene.vcr");
        std::fs::write(
            &manifest_path,
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 24 }
params:
  speed:
    type: float
    default: 1.2
    min: 0.5
    max: 2.0
  center:
    type: vec2
    default: [0.5, 0.5]
  tint:
    type: color
    default: { r: 0.1, g: 0.2, b: 0.3, a: 1.0 }
layers:
  - id: one
    start_time: "${speed}"
    position: "${center}"
    procedural:
      kind: solid_color
      color: "${tint}"
"#,
        )
        .expect("manifest should write");

        let manifest = load_and_validate_manifest_with_options(
            &manifest_path,
            &ManifestLoadOptions {
                overrides: vec![ParamOverride::parse("speed=1.5").expect("override parses")],
                allow_raw_paths: false,
            },
        )
        .expect("manifest should load");

        assert_eq!(manifest.params.get("speed").copied(), Some(1.5));
        assert_eq!(manifest.resolved_params.len(), 3);
    }

    #[test]
    fn override_bounds_are_enforced() {
        let dir = tempdir().expect("tempdir should create");
        let manifest_path = dir.path().join("scene.vcr");
        std::fs::write(
            &manifest_path,
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 24 }
params:
  speed:
    type: float
    default: 1.0
    min: 0.5
    max: 2.0
layers:
  - id: one
    procedural:
      kind: solid_color
      color: { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
"#,
        )
        .expect("manifest should write");

        let error = load_and_validate_manifest_with_options(
            &manifest_path,
            &ManifestLoadOptions {
                overrides: vec![ParamOverride::parse("speed=10").expect("override parses")],
                allow_raw_paths: false,
            },
        )
        .expect_err("override should fail");
        assert!(error.to_string().contains("above max"));
    }

    #[test]
    fn trailer_manifest_supports_library_source_shorthand() {
        let dir = tempdir().expect("tempdir should create");

        let source = dir.path().join("input.png");
        let mut image = image::RgbaImage::new(2, 2);
        image.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 1, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        image.save(&source).expect("image should save");

        add_asset(
            dir.path(),
            &LibraryAddRequest {
                source_path: source,
                id: "tiny-image".to_owned(),
                item_type: Some(LibraryItemType::Image),
                normalize: None,
            },
        )
        .expect("library add should succeed");

        let manifest_dir = dir.path().join("manifests/trailer");
        std::fs::create_dir_all(&manifest_dir).expect("manifest dir should create");
        let manifest_path = manifest_dir.join("scene.vcr");
        std::fs::write(
            &manifest_path,
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
layers:
  - id: logo
    source: { kind: library, id: tiny-image }
"#,
        )
        .expect("manifest should write");

        let manifest = load_and_validate_manifest_with_options(
            &manifest_path,
            &ManifestLoadOptions::default(),
        )
        .expect("manifest should load");

        assert_eq!(
            manifest.environment.encoding.prores_profile,
            ProResProfile::Prores4444
        );
        let layer = manifest.layers.first().expect("one layer expected");
        match layer {
            Layer::Image(layer) => {
                assert!(layer
                    .image
                    .path
                    .to_string_lossy()
                    .contains("library/items/tiny-image/source.png"));
            }
            other => panic!("expected image layer, got {other:?}"),
        }
    }

    #[test]
    fn manifest_supports_pack_source_shorthand() {
        let dir = tempdir().expect("tempdir should create");

        let source = dir.path().join("logo.png");
        let mut image = image::RgbaImage::new(2, 2);
        image.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 1, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        image.save(&source).expect("image should save");

        add_asset_to_pack(
            dir.path(),
            "brand-kit",
            &LibraryAddRequest {
                source_path: source,
                id: "tiny-logo".to_owned(),
                item_type: Some(LibraryItemType::Image),
                normalize: None,
            },
        )
        .expect("pack add should succeed");

        let manifest_path = dir.path().join("scene.vcr");
        std::fs::write(
            &manifest_path,
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
layers:
  - id: logo
    source: { kind: pack, pack: brand-kit, id: tiny-logo }
"#,
        )
        .expect("manifest should write");

        let manifest = load_and_validate_manifest_with_options(
            &manifest_path,
            &ManifestLoadOptions::default(),
        )
        .expect("manifest should load");

        let layer = manifest.layers.first().expect("one layer expected");
        match layer {
            Layer::Image(layer) => {
                assert!(layer
                    .image
                    .path
                    .to_string_lossy()
                    .contains("packs/brand-kit/items/tiny-logo/source.png"));
            }
            other => panic!("expected image layer, got {other:?}"),
        }
    }

    #[test]
    fn trailer_manifest_rejects_raw_paths_without_opt_in() {
        let dir = tempdir().expect("tempdir should create");
        let manifest_dir = dir.path().join("manifests/trailer");
        std::fs::create_dir_all(&manifest_dir).expect("manifest dir should create");

        let local_image = manifest_dir.join("local.png");
        let mut image = image::RgbaImage::new(1, 1);
        image.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        image.save(&local_image).expect("image should save");

        let manifest_path = manifest_dir.join("raw_path.vcr");
        std::fs::write(
            &manifest_path,
            r#"
version: 1
environment:
  resolution: { width: 64, height: 64 }
  fps: 24
  duration: { frames: 2 }
layers:
  - id: logo
    image:
      path: local.png
"#,
        )
        .expect("manifest should write");

        let error = load_and_validate_manifest_with_options(
            &manifest_path,
            &ManifestLoadOptions::default(),
        )
        .expect_err("raw paths should fail in trailer by default");
        assert!(error.to_string().contains("must use library:<id>"));

        load_and_validate_manifest_with_options(
            &manifest_path,
            &ManifestLoadOptions {
                overrides: Vec::new(),
                allow_raw_paths: true,
            },
        )
        .expect("raw paths should load when allow_raw_paths=true");
    }
}
