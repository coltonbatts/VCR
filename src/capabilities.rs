//! Installed-engine discovery: what this binary can do, and what is usable on this machine.
//!
//! Everything here is derived from the engine's own definitions (the serde types via
//! `schemars`, the renderer's support tables, the expression table, the font table). Nothing is a
//! hand-maintained prose list, so it cannot drift from behavior without a failing test.

use std::path::Path;

use serde_json::{json, Value};

use crate::agent_contract::{
    EngineIdentity, CONTRACT_VERSION, EXIT_BLOCKED, EXIT_IO, EXIT_MANIFEST,
    EXIT_MISSING_DEPENDENCY, EXIT_USAGE,
};
use crate::font_assets::{FONT_FAMILY_ALIASES, GEIST_PIXEL_FILES};
use crate::preflight::RuntimeProbe;
use crate::renderer::SOFTWARE_SUPPORTED_LAYER_TYPES;
use crate::schema::{Manifest, DEFAULT_MANIFEST_VERSION, EXPRESSION_FUNCTIONS};

pub fn manifest_json_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(Manifest)).unwrap_or(Value::Null)
}

fn defs<'a>(schema: &'a Value, name: &str) -> Option<&'a Value> {
    schema.get("$defs")?.get(name)
}

/// Variant tags of an internally tagged enum, read from its generated `oneOf`.
fn tagged_variants(schema: &Value, def: &str, tag: &str) -> Vec<String> {
    defs(schema, def)
        .and_then(|d| d.get("oneOf"))
        .and_then(Value::as_array)
        .map(|variants| {
            variants
                .iter()
                .filter_map(|variant| {
                    variant
                        .get("properties")?
                        .get(tag)?
                        .get("const")?
                        .as_str()
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn enum_values(schema: &Value, def: &str) -> Vec<String> {
    defs(schema, def)
        .and_then(|d| d.get("enum"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// YAML source keys of a layer (`image`, `text`, `source_path`, ...), derived from the schema:
/// the optional `*Source` blocks plus the legacy `source_path`.
pub fn layer_source_keys(schema: &Value) -> Vec<String> {
    let Some(props) = defs(schema, "Layer")
        .and_then(|layer| layer.get("properties"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut keys = props
        .iter()
        .filter(|(key, value)| {
            key.as_str() == "source_path"
                || value
                    .get("anyOf")
                    .and_then(Value::as_array)
                    .is_some_and(|options| {
                        options.iter().any(|option| {
                            option
                                .get("$ref")
                                .and_then(Value::as_str)
                                .is_some_and(|r| r.ends_with("Source"))
                        })
                    })
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn layer_kind_for_key(key: &str) -> &str {
    if key == "source_path" {
        "asset"
    } else {
        key
    }
}

pub fn discover(runtime: &RuntimeProbe, cli_commands: Vec<Value>, include_schema: bool) -> Value {
    let schema = manifest_json_schema();
    let identity = EngineIdentity::current();
    let gpu = runtime.gpu.available;

    let layers = layer_source_keys(&schema)
        .into_iter()
        .map(|key| {
            let kind = layer_kind_for_key(&key).to_owned();
            let software = SOFTWARE_SUPPORTED_LAYER_TYPES.contains(&kind.as_str());
            json!({
                "kind": kind,
                "yaml_key": key,
                "compiled_backends": if software { json!(["software", "gpu"]) } else { json!(["gpu"]) },
                "usable_here": software || gpu,
                "requires_gpu": !software,
            })
        })
        .collect::<Vec<_>>();

    let resolved_fonts_ok = runtime.fonts.bundle_ok;
    let fonts = json!({
        "bundle_ok": resolved_fonts_ok,
        "files": GEIST_PIXEL_FILES,
        "families": FONT_FAMILY_ALIASES.iter().map(|(alias, file)| json!({"name": alias, "file": file})).collect::<Vec<_>>(),
        "unknown_family_behavior": "substitutes GeistPixel-Line (flagged by preflight as text.font_family_fallback)",
    });

    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let example_files = std::fs::read_dir(&examples)
        .map(|entries| {
            let mut names = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".vcr"))
                .collect::<Vec<_>>();
            names.sort();
            names
        })
        .unwrap_or_default();

    json!({
        "engine": identity,
        "contract": {
            "version": CONTRACT_VERSION,
            "stdout": "exactly one JSON document on a single line when --json is passed",
            "stderr": "human progress/logging only; never required to interpret a result",
            "status_values": ["ok", "blocked", "failed", "error"],
            "exit_codes": {
                "0": "ok",
                EXIT_USAGE.to_string(): "usage/argument error",
                EXIT_MANIFEST.to_string(): "manifest/validation/lint/verification failure",
                EXIT_MISSING_DEPENDENCY.to_string(): "missing runtime dependency",
                EXIT_IO.to_string(): "I/O or encoder failure",
                EXIT_BLOCKED.to_string(): "blocked: unresolved requirements (only with --strict)",
            },
        },
        "manifest": {
            "supported_versions": [DEFAULT_MANIFEST_VERSION],
            "schema_included": include_schema,
            "schema": if include_schema { schema.clone() } else { Value::Null },
        },
        "time": {
            "duration": {
                "forms": ["seconds as a number", "{frames: N}"],
                "frame_count_rule": "seconds form: ceil(seconds * fps) evaluated in f32, minimum 1; frames form: exact",
                "fps": "positive integer; frame rate is exactly fps/1",
            },
            "expression_t": "frame index as float, 0-based (not seconds)",
            "keyframes": "start_frame/end_frame are frame indices",
            "layer_start_time_end_time": "seconds (global timeline)",
            "layer_time_offset": "seconds; local frame = (global_frame + time_offset*fps) * time_scale",
            "note": "keyframes and expressions inside a layer use the local (remapped) frame; a layer with start_time does NOT shift its keyframes unless time_offset is set to -start_time",
        },
        "layers": layers,
        "procedural_kinds": tagged_variants(&schema, "ProceduralSource", "kind"),
        "procedural_geometry": "center/size/radius/p0..p2/start/end/thickness are NORMALIZED fractions of canvas width and height (corner_radius and radii scale with width); the procedural layer itself is canvas-sized and `position`/`scale` transform the whole canvas-sized layer",
        "post_effects": {
            "shaders": tagged_variants(&schema, "PostEffect", "shader"),
            "backends": ["gpu"],
            "software_behavior": "unsupported: preflight reports a blocker and render refuses",
        },
        "expression": {
            "functions": EXPRESSION_FUNCTIONS.iter().map(|(name, args, summary)| json!({"name": name, "args": args, "summary": summary})).collect::<Vec<_>>(),
            "variables": ["t (frame index)", "any declared param name"],
            "operators": ["+", "-", "*", "/", "%", "^", "unary -", "parentheses"],
        },
        "encoding": {
            "containers": ["mov"],
            "codec": "prores",
            "profiles": enum_values(&schema, "ProResProfile"),
            "alpha_profiles": ["prores4444", "prores4444_xq"],
            "encoders": enum_values(&schema, "ProResEncoder"),
            "color_spaces": enum_values(&schema, "ColorSpace"),
            "default_profile": "hq",
            "golden_path_render": "`vcr render` forces an alpha-capable profile (prores4444) when the manifest profile has no alpha",
        },
        "backends": {
            "software": {"compiled": true, "usable": runtime.software_backend_available, "deterministic": "frame bytes identical for the same engine build, manifest, params and seed; scope in docs/AGENT_CONTRACT.md"},
            "gpu": {"compiled": true, "usable": gpu, "detail": runtime.gpu.detail, "deterministic": "not guaranteed across hardware/drivers"},
            "software_supported_layer_types": SOFTWARE_SUPPORTED_LAYER_TYPES,
        },
        "features_compiled": {
            "sidecar_ffmpeg": cfg!(feature = "sidecar_ffmpeg"),
            "play": cfg!(feature = "play"),
            "workflow": cfg!(feature = "workflow"),
            "wgpu_layers": cfg!(feature = "wgpu_layers"),
        },
        "runtime": runtime,
        "fonts": fonts,
        "authoring": {
            "examples_dir": examples.display().to_string(),
            "examples": example_files,
            "text": "text layers render GeistPixel fonts only; no wrapping (wrap_style Letter, no max_width)",
        },
        "cli": { "commands": cli_commands },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_source_keys_are_the_ten_known_sources() {
        let schema = manifest_json_schema();
        assert_eq!(
            layer_source_keys(&schema),
            vec![
                "ascii",
                "image",
                "lottie",
                "procedural",
                "sequence",
                "shader",
                "source_path",
                "text",
                "video",
                "wgpu_shader"
            ]
        );
    }

    #[test]
    fn procedural_and_post_variants_are_derived() {
        let schema = manifest_json_schema();
        let procedural = tagged_variants(&schema, "ProceduralSource", "kind");
        assert!(procedural.contains(&"solid_color".to_owned()));
        assert!(procedural.contains(&"rounded_rect".to_owned()));
        let post = tagged_variants(&schema, "PostEffect", "shader");
        assert!(post.contains(&"levels".to_owned()) && post.contains(&"sobel".to_owned()));
    }
}
