use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::ascii_capture::{
    run_ascii_capture, AsciiCaptureArgs, AsciiCaptureSource, SymbolRemapMode,
};
use crate::aspect_preset::AspectPreset;
use crate::error_codes::CodedError;

const DOCU_PACK_ID: &str = "docu_pack_v1";
const DOCU_PACK_VERSION: &str = "v1";
const DOCU_ARTIFACT_ID: &str = "lower_third";
const DOCU_REQUIRED_FIELDS: [FieldLimit; 2] = [
    FieldLimit {
        name: "title",
        max_len: 32,
    },
    FieldLimit {
        name: "subtitle",
        max_len: 48,
    },
];

const PACK_CAPTURE_SOURCE: &str = "library:geist-wave";
const PACK_CAPTURE_COLS: u32 = 64;
const PACK_CAPTURE_ROWS: u32 = 24;
const PACK_CAPTURE_FRAMES: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackCompileBackend {
    Auto,
    Software,
    Gpu,
}

#[derive(Debug, Clone)]
pub struct PackCompileRequest {
    pub pack_id: String,
    pub fields_arg: String,
    pub aspect_keyword: String,
    pub fps: u32,
    pub output_root: PathBuf,
    pub backend: PackCompileBackend,
}

#[derive(Debug, Clone)]
pub struct PackCompileSummary {
    pub mov_path: PathBuf,
    pub frame_hashes_path: PathBuf,
    pub artifact_manifest_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct FieldLimit {
    name: &'static str,
    max_len: usize,
}

pub fn compile_pack(request: PackCompileRequest) -> Result<PackCompileSummary> {
    let output_root = resolve_output_root(&request.output_root)?;
    let pack = lookup_pack(&request.pack_id)?;

    if request.backend == PackCompileBackend::Gpu {
        return Err(anyhow!(CodedError::usage(
            "UNSUPPORTED_OPTION",
            "pack compile supports only software backend for v0",
        )
        .with_details(json!({
            "option": "backend",
            "provided": "gpu",
            "allowed": ["software"]
        }))));
    }

    if request.fps != 24 {
        return Err(anyhow!(CodedError::usage(
            "INVALID_FPS",
            format!("invalid fps '{}' for pack compile v0", request.fps),
        )
        .with_details(json!({
            "provided": request.fps,
            "allowed": [24]
        }))));
    }

    let fields = parse_fields(&request.fields_arg)?;
    validate_required_fields(&fields, pack.required_fields)?;

    let aspect = AspectPreset::from_keyword(&request.aspect_keyword)?;
    let output_dir = output_root
        .join(pack.pack_id)
        .join(pack.pack_version)
        .join(format!("{}_{}", aspect.keyword(), request.fps));
    let mov_path = output_dir.join(format!(
        "{}__{}__{}__{}__core-{}__pack-{}.mov",
        pack.pack_id,
        pack.artifact_id,
        aspect.keyword(),
        request.fps,
        env!("CARGO_PKG_VERSION"),
        pack.pack_version
    ));

    let source = AsciiCaptureSource::parse(PACK_CAPTURE_SOURCE)
        .context("internal pack compile source was invalid")?;
    let summary = run_ascii_capture(&AsciiCaptureArgs {
        source,
        output: mov_path,
        fps: request.fps,
        duration_seconds: 1.0,
        max_frames: Some(PACK_CAPTURE_FRAMES),
        cols: PACK_CAPTURE_COLS,
        rows: PACK_CAPTURE_ROWS,
        font_path: None,
        font_size: 16.0,
        tmp_dir: None,
        debug_txt_dir: None,
        symbol_remap: SymbolRemapMode::Equalize,
        symbol_ramp: None,
        fit_padding: 0.12,
        bg_alpha: 1.0,
        aspect,
        pack_id: pack.pack_id.to_owned(),
        pack_version: pack.pack_version.to_owned(),
        artifact_id: pack.artifact_id.to_owned(),
    })?;

    Ok(PackCompileSummary {
        mov_path: summary.output,
        frame_hashes_path: summary.frame_hashes_path,
        artifact_manifest_path: summary.artifact_manifest_path,
    })
}

struct PackContract {
    pack_id: &'static str,
    pack_version: &'static str,
    artifact_id: &'static str,
    required_fields: &'static [FieldLimit],
}

fn lookup_pack(pack_id: &str) -> Result<PackContract> {
    if pack_id == DOCU_PACK_ID {
        return Ok(PackContract {
            pack_id: DOCU_PACK_ID,
            pack_version: DOCU_PACK_VERSION,
            artifact_id: DOCU_ARTIFACT_ID,
            required_fields: &DOCU_REQUIRED_FIELDS,
        });
    }

    Err(anyhow!(CodedError::usage(
        "INVALID_PACK",
        format!("unknown pack '{pack_id}'"),
    )
    .with_details(json!({
        "provided": pack_id,
        "allowed": [DOCU_PACK_ID]
    }))))
}

fn parse_fields(value: &str) -> Result<BTreeMap<String, String>> {
    let trimmed = value.trim();
    let parsed = parse_fields_json(trimmed)?;
    let object = parsed
        .as_object()
        .ok_or_else(|| anyhow!("--fields must be a JSON object"))?;

    let mut out = BTreeMap::new();
    for (key, raw) in object {
        if let Some(text) = raw.as_str() {
            out.insert(key.clone(), text.to_owned());
        }
    }
    Ok(out)
}

fn parse_fields_json(value: &str) -> Result<Value> {
    if let Ok(parsed) = serde_json::from_str::<Value>(value) {
        return Ok(parsed);
    }

    let path = Path::new(value);
    let raw = fs::read_to_string(path).with_context(|| {
        format!(
            "--fields was neither valid JSON nor a readable file path: {}",
            value
        )
    })?;
    serde_json::from_str(&raw).with_context(|| {
        format!(
            "--fields file '{}' must contain a valid JSON object",
            path.display()
        )
    })
}

fn validate_required_fields(
    fields: &BTreeMap<String, String>,
    limits: &[FieldLimit],
) -> Result<()> {
    for limit in limits {
        let Some(value) = fields.get(limit.name) else {
            return Err(anyhow!(CodedError::usage(
                "MISSING_REQUIRED_FIELD",
                format!("missing required field '{}'", limit.name),
            )
            .with_details(json!({
                "field": limit.name,
                "required": true
            }))));
        };

        let value_len = value.chars().count();
        if value_len > limit.max_len {
            return Err(anyhow!(CodedError::usage(
                "FIELD_OVERFLOW",
                format!(
                    "field '{}' exceeds max length ({} > {})",
                    limit.name, value_len, limit.max_len
                ),
            )
            .with_details(json!({
                "field": limit.name,
                "max_len": limit.max_len,
                "actual_len": value_len,
                "overflow_policy": "compile_error"
            }))));
        }
    }
    Ok(())
}

fn resolve_output_root(output_root: &Path) -> Result<PathBuf> {
    if output_root.as_os_str().is_empty() {
        bail!("--out must not be empty");
    }
    if output_root.is_absolute() {
        bail!(
            "--out must be a relative path (deterministic output root), got {}",
            output_root.display()
        );
    }
    if output_root
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!(
            "--out must not contain parent traversal components: {}",
            output_root.display()
        );
    }
    Ok(output_root.to_path_buf())
}
