use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use anyhow::{bail, Result};
use regex::Regex;
use serde::Serialize;
use serde_yaml::{Mapping, Value};

type FreeformMap = BTreeMap<String, Value>;

const ALLOWED_TOP_LEVEL_GROUPS: [&str; 9] = [
    "project",
    "intent",
    "input",
    "scene",
    "ascii",
    "render",
    "output",
    "determinism",
    "notes",
];

const STYLE_TERMS: [&str; 7] = [
    "dreamcore",
    "vhs",
    "glitchy",
    "cinematic",
    "retro",
    "analog",
    "surreal",
];

const COLOR_TERMS: [&str; 8] = [
    "gamma",
    "linear",
    "srgb",
    "vhs",
    "rec709",
    "rec 709",
    "display p3",
    "rec2020",
];

#[derive(Debug, Clone, Serialize)]
pub struct PromptTranslation {
    pub standardized_vcr_prompt: String,
    pub normalized_spec: NormalizedSpec,
    pub unknowns_and_fixes: Vec<UnknownFix>,
    pub assumptions_applied: Vec<String>,
    pub acceptance_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownFix {
    pub issue: String,
    pub why_it_matters: String,
    pub proposed_fix: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizedSpec {
    pub project: FreeformMap,
    pub intent: FreeformMap,
    pub input: FreeformMap,
    pub scene: FreeformMap,
    pub ascii: FreeformMap,
    pub render: RenderSpec,
    pub output: OutputSpec,
    pub determinism: DeterminismSpec,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderSpec {
    pub resolution: ResolutionSpec,
    pub fps: u32,
    pub duration_seconds: Option<f64>,
    pub frames: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolutionSpec {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputSpec {
    pub r#type: String,
    pub path: String,
    pub container: String,
    pub codec: String,
    pub alpha: bool,
    pub fps: u32,
    pub frame_rate_conversion: FrameRateConversionSpec,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameRateConversionSpec {
    pub policy: String,
    pub interpolation: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeterminismSpec {
    pub enabled: bool,
    pub seed: u64,
}

#[derive(Debug, Default)]
struct WorkingSpec {
    project: FreeformMap,
    intent: FreeformMap,
    input: FreeformMap,
    scene: FreeformMap,
    ascii: FreeformMap,
    notes: Option<String>,
    render_width: Option<u32>,
    render_height: Option<u32>,
    render_fps: Option<u32>,
    duration_seconds: Option<f64>,
    frames: Option<u32>,
    output_type: Option<String>,
    output_path: Option<String>,
    output_container: Option<String>,
    output_codec: Option<String>,
    output_alpha: Option<bool>,
    output_fps: Option<u32>,
    frame_rate_conversion_policy: Option<String>,
    frame_rate_conversion_interpolation: Option<bool>,
    frame_rate_conversion_explicit: bool,
    seed: Option<u64>,
    determinism_enabled: Option<bool>,
    color_pipeline_explicit: bool,
    font_explicit: bool,
    unknown_top_level_groups: Vec<String>,
    source_kind: &'static str,
}

pub fn translate_to_standard_prompt(raw: &str) -> Result<PromptTranslation> {
    if raw.trim().is_empty() {
        bail!("input is empty");
    }

    let mut working = WorkingSpec::default();
    let mut unknowns = Vec::new();
    let mut assumptions = Vec::new();

    let parsed_yaml = parse_yaml_like(raw);
    match parsed_yaml {
        Some(ref root) => {
            working.source_kind = "yaml";
            ingest_yaml_spec(root, &mut working);
        }
        None => {
            working.source_kind = "natural_language";
            ingest_natural_language(raw, &mut working);
        }
    }

    let raw_lower = raw.to_ascii_lowercase();
    let style_terms = detect_terms(&raw_lower, &STYLE_TERMS);
    let mentions_color_terms = detect_terms(&raw_lower, &COLOR_TERMS);
    let mentions_audio = contains_any(
        &raw_lower,
        &["music", "audio", "song", "soundtrack", "voiceover", "sfx"],
    );
    let mentions_text_or_ascii = contains_any(
        &raw_lower,
        &["text", "title", "caption", "ascii", "type", "typography"],
    );
    let mentions_external_asset = contains_any(
        &raw_lower,
        &["logo", "image", "video", "clip", "asset", "font", "sprite"],
    );
    let has_asset_path = contains_asset_path(raw);

    let width = working.render_width.unwrap_or_else(|| {
        assumptions.push("Defaulted resolution width to 1920 because it was missing.".to_owned());
        1920
    });
    let height = working.render_height.unwrap_or_else(|| {
        assumptions.push("Defaulted resolution height to 1080 because it was missing.".to_owned());
        1080
    });
    let render_fps = working.render_fps.unwrap_or_else(|| {
        assumptions.push("Defaulted render fps to 60 because `render.fps` was missing.".to_owned());
        60
    });
    let output_fps = working.output_fps.unwrap_or_else(|| {
        assumptions.push(
            "Defaulted output fps to render fps because `output.fps` was missing.".to_owned(),
        );
        render_fps
    });

    let mut duration_seconds = working.duration_seconds;
    let mut frames = working.frames;
    match (duration_seconds, frames) {
        (None, Some(frame_count)) => {
            let computed = frame_count as f64 / render_fps as f64;
            duration_seconds = Some(computed);
            assumptions.push(format!(
                "Computed duration_seconds as frames/fps ({frame_count}/{render_fps})."
            ));
        }
        (Some(duration), None) => {
            let computed_frames = (duration * render_fps as f64).round();
            let computed_frames = if computed_frames.is_sign_negative() {
                0
            } else {
                computed_frames as u32
            };
            frames = Some(computed_frames);
            assumptions.push(format!(
                "Computed frames as round(duration_seconds*fps) ({duration:.3}*{render_fps})."
            ));
        }
        _ => {}
    }

    if duration_seconds.is_none() && frames.is_none() {
        unknowns.push(UnknownFix {
            issue: "Both `render.duration_seconds` and `render.frames` are missing.".to_owned(),
            why_it_matters: "Output length is undefined, so the render is not reproducible."
                .to_owned(),
            proposed_fix:
                "Set one of:\nrender:\n  duration_seconds: 5.0\nOR\nrender:\n  frames: 300"
                    .to_owned(),
        });
    }

    let mut output_type = working.output_type.unwrap_or_else(|| {
        if let Some(path) = working.output_path.as_deref() {
            if path.to_ascii_lowercase().ends_with(".png") {
                return "still".to_owned();
            }
        }
        assumptions.push("Defaulted output type to `video` because it was missing.".to_owned());
        "video".to_owned()
    });
    output_type.make_ascii_lowercase();

    let alpha = working.output_alpha.unwrap_or_else(|| {
        assumptions.push("Defaulted output alpha to false because it was missing.".to_owned());
        false
    });

    let mut output_path = working.output_path.clone().unwrap_or_else(|| {
        let default_path = if output_type == "still" {
            "./renders/out.png".to_owned()
        } else {
            "./renders/out.mov".to_owned()
        };
        assumptions.push(format!(
            "Defaulted output path to `{default_path}` because `output.path` was missing."
        ));
        default_path
    });

    if output_type == "still" && !output_path.to_ascii_lowercase().ends_with(".png") {
        unknowns.push(UnknownFix {
            issue: "Output type is `still` but output path is not a `.png` file.".to_owned(),
            why_it_matters: "Still exports require an image container; mismatched extension can break downstream tooling."
                .to_owned(),
            proposed_fix: "Set:\noutput:\n  type: \"still\"\n  path: \"./renders/out.png\"".to_owned(),
        });
    }

    if output_type == "video" && !output_path.to_ascii_lowercase().ends_with(".mov") {
        if output_path.to_ascii_lowercase().ends_with(".png") {
            output_type = "still".to_owned();
        } else {
            output_path = "./renders/out.mov".to_owned();
            assumptions.push(
                "Output type is `video` but path extension was unsupported; defaulted to `./renders/out.mov`."
                    .to_owned(),
            );
        }
    }

    let container = working.output_container.clone().unwrap_or_else(|| {
        if output_path.to_ascii_lowercase().ends_with(".png") {
            "png".to_owned()
        } else {
            "mov".to_owned()
        }
    });

    let codec = working.output_codec.clone().unwrap_or_else(|| {
        if output_type == "still" {
            assumptions.push("Defaulted still output codec to `png`.".to_owned());
            "png".to_owned()
        } else if alpha {
            assumptions.push(
                "Defaulted codec to ProRes 4444 because codec was missing and alpha is enabled."
                    .to_owned(),
            );
            "prores_4444".to_owned()
        } else {
            assumptions.push(
                "Defaulted codec to ProRes 422 HQ because codec was missing and alpha is disabled."
                    .to_owned(),
            );
            "prores_422_hq".to_owned()
        }
    });

    if alpha && !codec_supports_alpha(&codec) {
        unknowns.push(UnknownFix {
            issue: "Alpha is enabled but selected codec does not support alpha.".to_owned(),
            why_it_matters: "Transparency may be dropped, causing compositing regressions."
                .to_owned(),
            proposed_fix:
                "Set:\noutput:\n  codec: \"prores_4444\"\n  container: \"mov\"\n  alpha: true"
                    .to_owned(),
        });
    }

    if alpha && container.eq_ignore_ascii_case("mp4") {
        unknowns.push(UnknownFix {
            issue: "Alpha requested with MP4 container.".to_owned(),
            why_it_matters: "Common MP4/H.264 exports discard alpha, producing opaque output."
                .to_owned(),
            proposed_fix: "Use MOV + ProRes 4444 for alpha output.".to_owned(),
        });
    }

    let mut conversion_policy = working
        .frame_rate_conversion_policy
        .clone()
        .unwrap_or_else(|| "frame_sampling".to_owned());
    conversion_policy.make_ascii_lowercase();
    let conversion_interpolation = working.frame_rate_conversion_interpolation.unwrap_or(false);

    if render_fps != output_fps && !working.frame_rate_conversion_explicit {
        unknowns.push(UnknownFix {
            issue: format!(
                "Render fps ({render_fps}) and output fps ({output_fps}) differ without explicit conversion policy."
            ),
            why_it_matters: "Frame conversion behavior can change motion timing and smoothness."
                .to_owned(),
            proposed_fix: "Set:\noutput:\n  frame_rate_conversion:\n    policy: \"frame_sampling\"\n    interpolation: false".to_owned(),
        });
        assumptions.push(
            "Applied default frame-rate conversion policy: frame_sampling with interpolation disabled."
                .to_owned(),
        );
    }

    let seed = working.seed.unwrap_or_else(|| {
        assumptions.push("Defaulted deterministic seed to 0 because it was missing.".to_owned());
        0
    });

    if !working.unknown_top_level_groups.is_empty() {
        for group in &working.unknown_top_level_groups {
            unknowns.push(UnknownFix {
                issue: format!("Unknown top-level group `{group}`."),
                why_it_matters:
                    "Unknown groups are ignored by normalization and can hide user intent."
                        .to_owned(),
                proposed_fix: format!(
                    "Move `{group}` under one of: {}",
                    ALLOWED_TOP_LEVEL_GROUPS.join(", ")
                ),
            });
        }
    }

    if !style_terms.is_empty() {
        unknowns.push(UnknownFix {
            issue: format!(
                "Ambiguous style terms without deterministic parameter mapping: {}.",
                style_terms.join(", ")
            ),
            why_it_matters:
                "Casual style words are subjective and can produce inconsistent outputs.".to_owned(),
            proposed_fix: "Add explicit style directives in YAML, for example:\nscene:\n  style_directives:\n    bloom: 0.2\n    chromatic_aberration_px: 1.5\n    grain_strength: 0.1".to_owned(),
        });
    }

    if !mentions_color_terms.is_empty() && !working.color_pipeline_explicit {
        unknowns.push(UnknownFix {
            issue: "Color-related terms were used without an explicit color management pipeline."
                .to_owned(),
            why_it_matters:
                "Gamma/transfer ambiguity can shift look and break reproducibility.".to_owned(),
            proposed_fix:
                "Add:\nrender:\n  color_pipeline:\n    working_space: \"srgb\"\n    transfer: \"gamma2.2\""
                    .to_owned(),
        });
    }

    if mentions_audio {
        unknowns.push(UnknownFix {
            issue: "Audio/music was requested.".to_owned(),
            why_it_matters:
                "VCR render output is silent; audio is not authored in this spec pipeline."
                    .to_owned(),
            proposed_fix:
                "Remove audio requirements from render spec or mux audio in a separate post step."
                    .to_owned(),
        });
    }

    if (mentions_text_or_ascii || !working.ascii.is_empty()) && !working.font_explicit {
        unknowns.push(UnknownFix {
            issue: "Text/ASCII is requested but no explicit font is provided.".to_owned(),
            why_it_matters:
                "Font differences change metrics and layout, reducing output predictability."
                    .to_owned(),
            proposed_fix: "Add:\nascii:\n  font: \"GeistPixel-Line\"".to_owned(),
        });
    }

    if mentions_external_asset && !has_asset_path {
        unknowns.push(UnknownFix {
            issue: "Asset-like request found without explicit file paths.".to_owned(),
            why_it_matters:
                "The engine cannot resolve external assets unless paths are concrete.".to_owned(),
            proposed_fix:
                "Add explicit paths, for example:\ninput:\n  assets:\n    - path: \"./assets/logo.png\""
                    .to_owned(),
        });
    }

    let normalized_spec = NormalizedSpec {
        project: working.project,
        intent: working.intent,
        input: working.input,
        scene: working.scene,
        ascii: working.ascii,
        render: RenderSpec {
            resolution: ResolutionSpec { width, height },
            fps: render_fps,
            duration_seconds,
            frames,
        },
        output: OutputSpec {
            r#type: output_type.clone(),
            path: output_path.clone(),
            container,
            codec,
            alpha,
            fps: output_fps,
            frame_rate_conversion: FrameRateConversionSpec {
                policy: conversion_policy,
                interpolation: conversion_interpolation,
            },
        },
        determinism: DeterminismSpec {
            enabled: working.determinism_enabled.unwrap_or(true),
            seed,
        },
        notes: working.notes,
    };

    let standardized_vcr_prompt =
        build_standardized_prompt(&normalized_spec, working.source_kind, raw.trim(), &unknowns);
    let acceptance_checks = build_acceptance_checks(&normalized_spec);

    Ok(PromptTranslation {
        standardized_vcr_prompt,
        normalized_spec,
        unknowns_and_fixes: unknowns,
        assumptions_applied: assumptions,
        acceptance_checks,
    })
}

fn ingest_yaml_spec(root: &Mapping, spec: &mut WorkingSpec) {
    let mut unknown_groups = Vec::new();
    for key in root.keys() {
        if let Some(name) = key.as_str() {
            if !ALLOWED_TOP_LEVEL_GROUPS.contains(&name) {
                unknown_groups.push(name.to_owned());
            }
        }
    }
    spec.unknown_top_level_groups = unknown_groups;

    spec.project = section_to_map(root, "project");
    spec.intent = section_to_map(root, "intent");
    spec.input = section_to_map(root, "input");
    spec.scene = section_to_map(root, "scene");
    spec.ascii = section_to_map(root, "ascii");
    spec.notes = mapping_get(root, "notes").and_then(value_to_string);

    if let Some(render) = mapping_get(root, "render").and_then(Value::as_mapping) {
        if let Some(resolution) = mapping_get(render, "resolution").and_then(Value::as_mapping) {
            spec.render_width = mapping_get(resolution, "width").and_then(value_to_u32);
            spec.render_height = mapping_get(resolution, "height").and_then(value_to_u32);
        }
        spec.render_fps = mapping_get(render, "fps").and_then(value_to_u32);
        spec.duration_seconds = mapping_get(render, "duration_seconds").and_then(value_to_f64);
        spec.frames = mapping_get(render, "frames").and_then(value_to_u32);

        if spec.duration_seconds.is_none() {
            if let Some(duration_value) = mapping_get(render, "duration") {
                spec.duration_seconds = value_to_f64(duration_value);
                if let Some(duration_mapping) = duration_value.as_mapping() {
                    spec.frames = spec
                        .frames
                        .or_else(|| mapping_get(duration_mapping, "frames").and_then(value_to_u32));
                }
            }
        }

        spec.color_pipeline_explicit = mapping_get(render, "color_pipeline").is_some()
            || mapping_get(render, "color_space").is_some()
            || mapping_get(render, "gamma").is_some();
    }

    if let Some(output) = mapping_get(root, "output").and_then(Value::as_mapping) {
        spec.output_type = mapping_get(output, "type").and_then(value_to_string);
        spec.output_path = mapping_get(output, "path").and_then(value_to_string);
        spec.output_container = mapping_get(output, "container").and_then(value_to_string);
        spec.output_codec = mapping_get(output, "codec").and_then(value_to_string);
        spec.output_alpha = mapping_get(output, "alpha").and_then(value_to_bool);
        spec.output_fps = mapping_get(output, "fps")
            .and_then(value_to_u32)
            .or_else(|| mapping_get(output, "export_fps").and_then(value_to_u32));
        if let Some(conversion) =
            mapping_get(output, "frame_rate_conversion").and_then(Value::as_mapping)
        {
            spec.frame_rate_conversion_explicit = true;
            spec.frame_rate_conversion_policy =
                mapping_get(conversion, "policy").and_then(value_to_string);
            spec.frame_rate_conversion_interpolation =
                mapping_get(conversion, "interpolation").and_then(value_to_bool);
        }
    }

    if let Some(determinism) = mapping_get(root, "determinism").and_then(Value::as_mapping) {
        spec.seed = mapping_get(determinism, "seed").and_then(value_to_u64);
        spec.determinism_enabled = mapping_get(determinism, "enabled").and_then(value_to_bool);
    }

    spec.font_explicit = has_font_key(root);
}

fn ingest_natural_language(raw: &str, spec: &mut WorkingSpec) {
    spec.intent
        .insert("prompt".to_owned(), Value::String(raw.trim().to_owned()));

    if let Some((w, h)) = parse_resolution(raw) {
        spec.render_width = Some(w);
        spec.render_height = Some(h);
    }

    if let Some(fps) = parse_named_fps(raw, true) {
        spec.render_fps = Some(fps);
    }
    if let Some(fps) = parse_named_fps(raw, false) {
        spec.output_fps = Some(fps);
    }
    if spec.render_fps.is_none() {
        if let Some(fps) = parse_any_fps(raw) {
            spec.render_fps = Some(fps);
        }
    }

    spec.duration_seconds = parse_duration_seconds(raw);
    spec.frames = parse_frames(raw);
    spec.seed = parse_seed(raw);
    spec.output_path = parse_output_path(raw);
    spec.output_codec = parse_codec(raw);
    spec.output_alpha = parse_alpha(raw);

    if let Some(path) = spec.output_path.as_deref() {
        if path.to_ascii_lowercase().ends_with(".png") {
            spec.output_type = Some("still".to_owned());
        } else {
            spec.output_type = Some("video".to_owned());
        }
    } else {
        let lower = raw.to_ascii_lowercase();
        if contains_any(&lower, &["still", "image", "png", "poster"]) {
            spec.output_type = Some("still".to_owned());
        }
    }
}

fn build_standardized_prompt(
    spec: &NormalizedSpec,
    source_kind: &str,
    raw_input: &str,
    unknowns: &[UnknownFix],
) -> String {
    let duration_text = spec
        .render
        .duration_seconds
        .map(|value| format!("{value:.3}s"))
        .unwrap_or_else(|| "UNSET".to_owned());
    let frames_text = spec
        .render
        .frames
        .map(|value| value.to_string())
        .unwrap_or_else(|| "UNSET".to_owned());
    let blocking_note = if spec.render.duration_seconds.is_none() && spec.render.frames.is_none() {
        "BLOCKING: No duration or frame count was provided; fail validation before render."
    } else {
        "No blocking validation errors were found in timing fields."
    };

    format!(
        "ROLE\n\
You are the VCR render engine. Execute deterministically from explicit parameters and fail fast on invalid or missing critical inputs.\n\n\
TASK\n\
Render with these explicit parameters:\n\
- Resolution: {width}x{height}\n\
- Render FPS: {render_fps}\n\
- Output FPS: {output_fps}\n\
- Duration: {duration_text}\n\
- Frames: {frames_text}\n\
- Output Type: {output_type}\n\
- Output Path: {output_path}\n\
- Container: {container}\n\
- Codec: {codec}\n\
- Alpha: {alpha}\n\
- Determinism Seed: {seed}\n\
- {blocking_note}\n\n\
INSTRUCTIONS\n\
- Determinism is required. Use the fixed seed and avoid time-varying randomness.\n\
- If exactly one of duration or frame count is provided, derive the other using fps and integer rounding for frames.\n\
- If render fps differs from output fps, apply frame sampling with interpolation disabled unless explicitly overridden.\n\
- Validate asset paths, fonts, and output compatibility before rendering.\n\
- Do not synthesize unsupported features; surface them as validation issues.\n\n\
CONTEXT\n\
- Source mode: {source_kind}\n\
- Original input snapshot: {raw_input}\n\
- Normalization produced {unknown_count} unresolved issue(s).\n\n\
OUTPUT FORMAT\n\
- Primary artifact path: {output_path}\n\
- Emit run metadata with resolution, fps, duration_seconds, frames, codec, alpha, seed, and frame conversion policy.",
        width = spec.render.resolution.width,
        height = spec.render.resolution.height,
        render_fps = spec.render.fps,
        output_fps = spec.output.fps,
        duration_text = duration_text,
        frames_text = frames_text,
        output_type = spec.output.r#type,
        output_path = spec.output.path,
        container = spec.output.container,
        codec = spec.output.codec,
        alpha = spec.output.alpha,
        seed = spec.determinism.seed,
        blocking_note = blocking_note,
        source_kind = source_kind,
        raw_input = raw_input.replace('\n', " "),
        unknown_count = unknowns.len(),
    )
}

fn build_acceptance_checks(spec: &NormalizedSpec) -> Vec<String> {
    let mut checks = vec![
        "Spec serializes to valid YAML with expected top-level keys.".to_owned(),
        format!(
            "assert render.resolution.width == {} && render.resolution.height == {}",
            spec.render.resolution.width, spec.render.resolution.height
        ),
        format!("assert render.fps == {}", spec.render.fps),
        format!("assert output.fps == {}", spec.output.fps),
        format!("assert determinism.seed == {}", spec.determinism.seed),
        format!(
            "assert output.path == \"{}\" && output.codec == \"{}\"",
            spec.output.path, spec.output.codec
        ),
    ];

    if let (Some(duration), Some(frames)) = (spec.render.duration_seconds, spec.render.frames) {
        checks.push(format!(
            "assert frames == round(duration_seconds * fps) ({} == round({:.3} * {}))",
            frames, duration, spec.render.fps
        ));
    } else {
        checks.push(
            "assert either render.duration_seconds or render.frames is explicitly set.".to_owned(),
        );
    }

    checks.push(
        "assert all referenced asset and font paths exist before render execution.".to_owned(),
    );
    checks
}

fn parse_yaml_like(raw: &str) -> Option<Mapping> {
    let parsed = serde_yaml::from_str::<Value>(raw).ok()?;
    let root = parsed.as_mapping()?;
    let has_known_group = root
        .keys()
        .filter_map(Value::as_str)
        .any(|key| ALLOWED_TOP_LEVEL_GROUPS.contains(&key));
    if has_known_group {
        Some(root.clone())
    } else {
        None
    }
}

fn section_to_map(root: &Mapping, key: &str) -> FreeformMap {
    let Some(value) = mapping_get(root, key) else {
        return FreeformMap::new();
    };

    if let Some(mapping) = value.as_mapping() {
        return mapping_to_string_key_map(mapping);
    }

    let mut map = FreeformMap::new();
    map.insert("value".to_owned(), value.clone());
    map
}

fn mapping_to_string_key_map(mapping: &Mapping) -> FreeformMap {
    let mut out = FreeformMap::new();
    for (key, value) in mapping {
        if let Some(name) = key.as_str() {
            out.insert(name.to_owned(), value.clone());
        }
    }
    out
}

fn mapping_get<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a Value> {
    mapping.iter().find_map(|(candidate, value)| {
        candidate
            .as_str()
            .filter(|name| *name == key)
            .map(|_| value)
    })
}

fn value_to_u32(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|v| u32::try_from(v).ok()),
        Value::String(text) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn value_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn value_to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn value_to_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(boolean) => Some(*boolean),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    }
}

fn has_font_key(mapping: &Mapping) -> bool {
    for (key, value) in mapping {
        if key
            .as_str()
            .map(|name| name.to_ascii_lowercase().contains("font"))
            .unwrap_or(false)
        {
            return true;
        }

        match value {
            Value::Mapping(child) if has_font_key(child) => return true,
            Value::Sequence(sequence) => {
                for item in sequence {
                    if let Value::Mapping(child) = item {
                        if has_font_key(child) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn codec_supports_alpha(codec: &str) -> bool {
    let normalized = codec.to_ascii_lowercase();
    normalized.contains("4444")
        || normalized.contains("png")
        || normalized.contains("qtrle")
        || normalized.contains("yuva")
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn detect_terms(haystack: &str, terms: &[&str]) -> Vec<String> {
    let mut found = BTreeSet::new();
    for term in terms {
        if haystack.contains(term) {
            found.insert((*term).to_owned());
        }
    }
    found.into_iter().collect()
}

fn parse_resolution(raw: &str) -> Option<(u32, u32)> {
    static RESOLUTION_RE: OnceLock<Regex> = OnceLock::new();
    let re = RESOLUTION_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(\d{3,5})\s*[xX]\s*(\d{3,5})\b")
            .expect("resolution regex should compile")
    });
    let capture = re.captures(raw)?;
    let width = capture.get(1)?.as_str().parse::<u32>().ok()?;
    let height = capture.get(2)?.as_str().parse::<u32>().ok()?;
    Some((width, height))
}

fn parse_named_fps(raw: &str, render: bool) -> Option<u32> {
    static RENDER_FPS_RE: OnceLock<Regex> = OnceLock::new();
    static OUTPUT_FPS_RE: OnceLock<Regex> = OnceLock::new();
    let re = if render {
        RENDER_FPS_RE.get_or_init(|| {
            Regex::new(r"(?i)\b(?:render|internal)\s*fps\s*[:=]?\s*(\d{1,3})\b")
                .expect("render fps regex should compile")
        })
    } else {
        OUTPUT_FPS_RE.get_or_init(|| {
            Regex::new(r"(?i)\b(?:output|export)\s*fps\s*[:=]?\s*(\d{1,3})\b")
                .expect("output fps regex should compile")
        })
    };
    re.captures(raw)
        .and_then(|capture| capture.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
}

fn parse_any_fps(raw: &str) -> Option<u32> {
    static ANY_FPS_RE: OnceLock<Regex> = OnceLock::new();
    let re = ANY_FPS_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(\d{1,3})\s*fps\b").expect("generic fps regex should compile")
    });
    re.captures(raw)
        .and_then(|capture| capture.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
}

fn parse_duration_seconds(raw: &str) -> Option<f64> {
    static DURATION_RE: OnceLock<Regex> = OnceLock::new();
    let re = DURATION_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(\d+(?:\.\d+)?)\s*(?:s|sec|secs|second|seconds)\b")
            .expect("duration regex should compile")
    });
    re.captures(raw)
        .and_then(|capture| capture.get(1))
        .and_then(|value| value.as_str().parse::<f64>().ok())
}

fn parse_frames(raw: &str) -> Option<u32> {
    static FRAMES_RE: OnceLock<Regex> = OnceLock::new();
    let re = FRAMES_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(\d+)\s*frames?\b").expect("frames regex should compile")
    });
    re.captures(raw)
        .and_then(|capture| capture.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
}

fn parse_seed(raw: &str) -> Option<u64> {
    static SEED_RE: OnceLock<Regex> = OnceLock::new();
    let re = SEED_RE.get_or_init(|| {
        Regex::new(r"(?i)\bseed\s*[:=]?\s*(\d+)\b").expect("seed regex should compile")
    });
    re.captures(raw)
        .and_then(|capture| capture.get(1))
        .and_then(|value| value.as_str().parse::<u64>().ok())
}

fn parse_output_path(raw: &str) -> Option<String> {
    static OUTPUT_PATH_RE: OnceLock<Regex> = OnceLock::new();
    let re = OUTPUT_PATH_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|\s)([./\w-]+?\.(?:mov|mp4|png))\b")
            .expect("output path regex should compile")
    });
    re.captures(raw)
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().to_owned())
}

fn parse_codec(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("prores 4444") || lower.contains("prores4444") {
        return Some("prores_4444".to_owned());
    }
    if lower.contains("prores 422 hq") || lower.contains("prores422hq") {
        return Some("prores_422_hq".to_owned());
    }
    None
}

fn parse_alpha(raw: &str) -> Option<bool> {
    let lower = raw.to_ascii_lowercase();
    if contains_any(&lower, &["without alpha", "no alpha", "opaque"]) {
        return Some(false);
    }
    if contains_any(&lower, &["alpha", "transparent", "transparency"]) {
        return Some(true);
    }
    None
}

fn contains_asset_path(raw: &str) -> bool {
    static ASSET_PATH_RE: OnceLock<Regex> = OnceLock::new();
    let re = ASSET_PATH_RE.get_or_init(|| {
        Regex::new(r"(?i)(?:[./][^\s]+|[A-Za-z0-9_-]+[./][^\s]+)\.(png|jpg|jpeg|mov|mp4|gif|ttf|otf|wgsl)\b")
            .expect("asset path regex should compile")
    });
    re.is_match(raw)
}

#[cfg(test)]
mod tests {
    use super::translate_to_standard_prompt;

    #[test]
    fn natural_language_defaults_are_applied() {
        let output = translate_to_standard_prompt("Make a 5s intro at 30fps with alpha.")
            .expect("translation should succeed");
        assert_eq!(output.normalized_spec.render.fps, 30);
        assert_eq!(output.normalized_spec.output.fps, 30);
        assert_eq!(output.normalized_spec.render.frames, Some(150));
        assert_eq!(output.normalized_spec.output.codec, "prores_4444");
        assert_eq!(output.normalized_spec.determinism.seed, 0);
    }

    #[test]
    fn missing_duration_and_frames_are_flagged() {
        let output = translate_to_standard_prompt("Render a clean 1920x1080 title card.")
            .expect("translation should succeed");
        assert!(output
            .unknowns_and_fixes
            .iter()
            .any(|issue| issue.issue.contains("duration_seconds")));
    }

    #[test]
    fn unknown_top_level_group_is_reported() {
        let yaml = r#"
project:
  name: "demo"
render:
  fps: 24
  duration_seconds: 2.0
rogue:
  mode: "test"
"#;
        let output = translate_to_standard_prompt(yaml).expect("translation should succeed");
        assert!(output
            .unknowns_and_fixes
            .iter()
            .any(|issue| issue.issue.contains("Unknown top-level group `rogue`")));
    }
}
