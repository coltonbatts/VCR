//! The VCR agent contract: one response envelope, one error taxonomy, one engine identity.
//!
//! Every machine-facing surface (CLI `--json`, the MCP server, tests) goes through this module so
//! that an agent never has to scrape human text. See `docs/AGENT_CONTRACT.md`.
//!
//! Contract rules:
//! - stdout carries exactly one JSON document (a single line) when `--json` is requested.
//! - stderr carries progress/logging and may be ignored by machine consumers.
//! - The process exit code mirrors `error.exit_code` / `status` (see [`Status::exit_code`]).
//! - Fields are only ever added within a contract major version.

use std::path::Path;

use anyhow::Error;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error_codes::find_coded_error;

/// Identifies the shape of every machine document. Bumped on breaking changes only.
pub const CONTRACT_VERSION: &str = "vcr.agent/1";
pub const CONTRACT_MAJOR: u32 = 1;

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_MANIFEST: u8 = 3;
pub const EXIT_MISSING_DEPENDENCY: u8 = 4;
pub const EXIT_IO: u8 = 5;
pub const EXIT_BLOCKED: u8 = 6;

/// Who produced a document. Agents must compare this against the producer of any artifact they
/// intend to trust: the verifier's identity is not evidence of the producer's identity.
#[derive(Debug, Clone, Serialize)]
pub struct EngineIdentity {
    pub name: &'static str,
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_hash: Option<&'static str>,
    pub build_profile: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub contract: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
}

impl EngineIdentity {
    pub fn current() -> Self {
        let git_hash = option_env!("VCR_GIT_HASH").filter(|hash| !hash.is_empty());
        Self {
            name: "vcr",
            version: env!("CARGO_PKG_VERSION"),
            git_hash,
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            contract: CONTRACT_VERSION,
            executable: std::env::current_exe()
                .ok()
                .map(|path| path.display().to_string()),
        }
    }

    /// `0.1.2 (85e6e26)` — matches `vcr --version`.
    pub fn display_version(&self) -> String {
        match self.git_hash {
            Some(hash) => format!("{} ({hash})", self.version),
            None => self.version.to_owned(),
        }
    }
}

/// Outcome of an operation.
///
/// - `ok`: the operation ran and its result is acceptable.
/// - `blocked`: the operation ran, but unresolved requirements must be settled before authoring or
///   rendering may continue (e.g. prompt normalization with missing duration). Never silently
///   treat as ok.
/// - `failed`: the operation ran and produced a negative finding (lint issues, verification
///   mismatch, failed preflight). `result` carries the details.
/// - `error`: the operation could not complete. `error` carries the cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Blocked,
    Failed,
    Error,
}

impl Status {
    pub fn exit_code(self) -> u8 {
        match self {
            Status::Ok => EXIT_SUCCESS,
            Status::Blocked => EXIT_BLOCKED,
            Status::Failed => EXIT_MANIFEST,
            Status::Error => EXIT_MANIFEST,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocker,
    Error,
    Warning,
    Info,
}

/// Whether a diagnostic is computed exactly by the engine or is a labelled estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    Exact,
    Approximate,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Location {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

impl Location {
    pub fn is_empty(&self) -> bool {
        self.file.is_none()
            && self.layer.is_none()
            && self.field.is_none()
            && self.line.is_none()
            && self.column.is_none()
    }

    pub fn layer(layer: impl Into<String>) -> Self {
        Self {
            layer: Some(layer.into()),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    /// Pipeline stage that produced it: prompt | check | lint | preflight | inspect | render | verify.
    pub stage: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
    /// Guidance only. Never permission to change the requester's intent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recovery: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<Basis>,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        stage: &str,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            stage: stage.to_owned(),
            message: message.into(),
            location: None,
            expected: None,
            observed: None,
            recovery: Vec::new(),
            basis: None,
        }
    }

    pub fn at(mut self, location: Location) -> Self {
        if !location.is_empty() {
            self.location = Some(location);
        }
        self
    }

    pub fn expected(mut self, value: impl Into<Value>) -> Self {
        self.expected = Some(value.into());
        self
    }

    pub fn observed(mut self, value: impl Into<Value>) -> Self {
        self.observed = Some(value.into());
        self
    }

    pub fn recover(mut self, step: impl Into<String>) -> Self {
        self.recovery.push(step.into());
        self
    }

    pub fn basis(mut self, basis: Basis) -> Self {
        self.basis = Some(basis);
        self
    }

    pub fn is_blocking(&self) -> bool {
        matches!(self.severity, Severity::Blocker | Severity::Error)
    }
}

/// Reference to a file the operation produced or consumed.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRef {
    /// output | preview_frame | contact_sheet | metadata | provenance | manifest | input
    pub role: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_seconds: Option<f64>,
}

impl ArtifactRef {
    pub fn new(role: &str, path: &Path) -> Self {
        Self {
            role: role.to_owned(),
            path: path.display().to_string(),
            sha256: None,
            bytes: None,
            frame: None,
            time_seconds: None,
        }
    }

    /// Records size and SHA-256 of an existing file. Missing files simply carry no digest, which
    /// is itself visible to the consumer.
    pub fn with_digest(mut self, path: &Path) -> Self {
        if let Ok(hash) = sha256_file(path) {
            self.sha256 = Some(hash);
        }
        self.bytes = std::fs::metadata(path).ok().map(|meta| meta.len());
        self
    }

    pub fn at_frame(mut self, frame: u32, time_seconds: f64) -> Self {
        self.frame = Some(frame);
        self.time_seconds = Some(time_seconds);
        self
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Failure description. `code`, `message`, `details` are the pre-contract envelope fields and are
/// preserved verbatim; everything else is additive.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    /// Stable, machine-matchable identifier (`manifest.schema_invalid`, `dependency.ffmpeg_missing`, ...).
    pub code: String,
    /// usage | manifest | dependency | backend | asset | io | encoder | artifact | timeout | internal
    pub category: &'static str,
    pub message: String,
    pub summary: String,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
    pub recovery: Vec<String>,
    /// True only when re-running the *unchanged* inputs plausibly succeeds (transient failures).
    pub retryable: bool,
    pub exit_code: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// The single response document shape.
#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub contract: &'static str,
    pub operation: String,
    pub ok: bool,
    pub status: Status,
    pub engine: EngineIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    /// Process exit code override (not serialized); defaults derive from `status`/`error`.
    #[serde(skip)]
    pub exit_override: Option<u8>,
}

impl Envelope {
    pub fn new(operation: &str, status: Status) -> Self {
        Self {
            contract: CONTRACT_VERSION,
            operation: operation.to_owned(),
            ok: status == Status::Ok,
            status,
            engine: EngineIdentity::current(),
            result: None,
            diagnostics: Vec::new(),
            artifacts: Vec::new(),
            error: None,
            exit_override: None,
        }
    }

    pub fn with_exit(mut self, code: u8) -> Self {
        self.exit_override = Some(code);
        self
    }

    pub fn ok(operation: &str, result: impl Serialize) -> Self {
        Self::new(operation, Status::Ok).with_result(result)
    }

    pub fn with_status(mut self, status: Status) -> Self {
        self.status = status;
        self.ok = status == Status::Ok;
        self
    }

    pub fn with_result(mut self, result: impl Serialize) -> Self {
        self.result = Some(serde_json::to_value(result).unwrap_or(Value::Null));
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    pub fn with_artifacts(mut self, artifacts: Vec<ArtifactRef>) -> Self {
        self.artifacts = artifacts;
        self
    }

    pub fn from_error(operation: &str, error: &Error) -> Self {
        let body = classify_error(operation, error);
        let mut envelope = Self::new(operation, Status::Error);
        envelope.error = Some(body);
        envelope
    }

    pub fn exit_code(&self) -> u8 {
        if let Some(code) = self.exit_override {
            return code;
        }
        match (&self.error, self.status) {
            (Some(error), _) => error.exit_code,
            (None, status) => status.exit_code(),
        }
    }

    /// Single-line JSON, suitable for `tail -1` and line-oriented transports.
    pub fn to_json_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|error| {
            format!(
                "{{\"contract\":\"{CONTRACT_VERSION}\",\"operation\":\"{}\",\"ok\":false,\"status\":\"error\",\"error\":{{\"code\":\"internal.serialization\",\"message\":\"{error}\"}}}}",
                self.operation
            )
        })
    }

    /// Merge pre-contract top-level keys into the document so existing consumers keep working.
    /// New code should read `result`. Existing keys are never overwritten.
    pub fn to_json_line_with_legacy(&self, legacy: &impl Serialize) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let (Some(target), Ok(Value::Object(extra))) =
            (value.as_object_mut(), serde_json::to_value(legacy))
        {
            for (key, entry) in extra {
                target.entry(key).or_insert(entry);
            }
        }
        value.to_string()
    }
}

// ───────────────────────────── error taxonomy ─────────────────────────────

struct Class {
    code: &'static str,
    category: &'static str,
    exit: u8,
    retryable: bool,
}

const fn class(code: &'static str, category: &'static str, exit: u8, retryable: bool) -> Class {
    Class {
        code,
        category,
        exit,
        retryable,
    }
}

fn chain_text(error: &Error) -> String {
    error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(" :: ")
        .to_ascii_lowercase()
}

fn any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn classify(error: &Error, text: &str) -> Class {
    if let Some(coded) = find_coded_error(error) {
        // Pre-contract coded errors keep their code and exit mapping.
        let category = match coded.code {
            "UNSUPPORTED_SOFTWARE_LAYER_TYPES" => "backend",
            _ => "usage",
        };
        return Class {
            code: coded.code,
            category,
            exit: EXIT_USAGE,
            retryable: false,
        };
    }
    if any(text, &["ffmpeg was not found on path"]) {
        return class(
            "dependency.ffmpeg_missing",
            "dependency",
            EXIT_MISSING_DEPENDENCY,
            false,
        );
    }
    if any(text, &["ffprobe was not found on path"]) {
        return class(
            "dependency.ffprobe_missing",
            "dependency",
            EXIT_MISSING_DEPENDENCY,
            false,
        );
    }
    if any(
        text,
        &[
            "curl was not found on path",
            "chafa was not found on path",
            "missing dependency",
        ],
    ) {
        return class(
            "dependency.missing",
            "dependency",
            EXIT_MISSING_DEPENDENCY,
            false,
        );
    }
    if any(
        text,
        &[
            "geist pixel font",
            "invalid geist pixel bundle",
            "font_asset_hash_mismatch",
        ],
    ) {
        return class(
            "dependency.font_missing",
            "dependency",
            EXIT_MISSING_DEPENDENCY,
            false,
        );
    }
    if any(
        text,
        &["no suitable gpu adapter", "gpu backend is unavailable"],
    ) {
        return class(
            "backend.unavailable",
            "backend",
            EXIT_MISSING_DEPENDENCY,
            false,
        );
    }
    if any(
        text,
        &[
            "invalid --set",
            "expected name=value",
            "use either --frame or --time",
            "use either --end-frame or --frames",
            "--time must be",
            "--frames must be > 0",
            "--interval-ms must be > 0",
            "preview --scale must be in",
            "invalid_aspect_preset",
            "invalid .vcrchat format",
            "invalid .vcrtxt format",
            "empty input script",
            "empty input transcript",
            "unknown --theme",
            "invalid --size",
            "invalid --source",
            "unsupported ascii-live stream",
            "invalid --export-dir",
            "--fps must be > 0",
            "--duration must be > 0",
            "--font-size must be > 0",
            "--speed must be > 0",
            "start frame",
            "out of bounds",
            "provide only one of",
            "missing input:",
        ],
    ) {
        return class("usage.invalid_argument", "usage", EXIT_USAGE, false);
    }
    if any(
        text,
        &[
            "failed to decode manifest",
            "unknown field",
            "missing field",
            "invalid type",
        ],
    ) {
        return class("manifest.schema_invalid", "manifest", EXIT_MANIFEST, false);
    }
    if any(text, &["does not exist", "no such file or directory"])
        && any(
            text,
            &[
                "image.path",
                "video.path",
                "source_path",
                "lottie",
                "sequence",
                "shader",
                "font",
            ],
        )
    {
        return class("asset.missing", "asset", EXIT_MANIFEST, false);
    }
    if any(
        text,
        &[
            "could not read",
            "could not decode",
            "did not report a video stream",
        ],
    ) {
        return class("artifact.unreadable", "artifact", EXIT_MANIFEST, false);
    }
    if any(text, &["output conformance check failed"]) {
        return class("artifact.nonconformant", "artifact", EXIT_MANIFEST, false);
    }
    if any(
        text,
        &[
            "ffmpeg exited",
            "ffmpeg failed",
            "ffmpeg encoder",
            "failed to spawn ffmpeg",
            "broken pipe",
        ],
    ) {
        return class("encoder.failed", "encoder", EXIT_IO, false);
    }
    if any(text, &["timed out", "timeout"]) {
        return class("timeout.exceeded", "timeout", EXIT_IO, true);
    }
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<std::io::Error>().is_some())
        || any(
            text,
            &[
                "failed to read",
                "failed to write",
                "failed waiting",
                "failed to create",
                "file not found",
            ],
        )
    {
        let code = if any(text, &["failed to write", "failed to create"]) {
            "io.write_failed"
        } else {
            "io.read_failed"
        };
        return class(code, "io", EXIT_IO, code == "io.write_failed");
    }
    class(
        "manifest.validation_failed",
        "manifest",
        EXIT_MANIFEST,
        false,
    )
}

/// Exit code for an error. Single source of truth for the CLI's `classify_exit_code`.
pub fn exit_code_for_error(error: &Error) -> u8 {
    let text = chain_text(error);
    classify(error, &text).exit
}

/// Build the structured description of a failure, extracting location/expected/observed from the
/// engine's own messages where they are reliably present.
pub fn classify_error(operation: &str, error: &Error) -> ErrorBody {
    let text = chain_text(error);
    let class = classify(error, &text);
    let coded = find_coded_error(error);
    let head = coded
        .map(|c| c.message.clone())
        .unwrap_or_else(|| error.to_string());
    let message = match coded {
        Some(c) => c.message.clone(),
        None => error
            .chain()
            .map(|cause| cause.to_string())
            .collect::<Vec<_>>()
            .join(": "),
    };
    let summary = head
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.')
        .to_owned();
    let raw_chain = error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    let mut location = Location::default();
    let mut expected: Option<Value> = None;
    let mut observed: Option<Value> = None;
    let mut recovery: Vec<String> = Vec::new();
    let mut details: Option<Value> = None;

    if let Some(coded) = find_coded_error(error) {
        details = coded.details.clone();
        if let Some(next) = coded
            .details
            .as_ref()
            .and_then(|d| d.get("next_steps"))
            .and_then(Value::as_array)
        {
            recovery.extend(next.iter().filter_map(|s| s.as_str().map(str::to_owned)));
        }
        if let Some(layers) = coded
            .details
            .as_ref()
            .and_then(|d| d.get("unsupported_layers"))
        {
            observed = Some(layers.clone());
        }
        if let Some(supported) = coded
            .details
            .as_ref()
            .and_then(|d| d.get("supported_layer_types"))
        {
            expected = Some(supported.clone());
        }
    }

    if let Some(path) = capture(
        &raw_chain,
        r"failed to (?:decode|read|parse) manifest (\S+?)(?:\.\s|:\s|\s|$)",
    ) {
        location.file = Some(path);
    }
    if let (Some(line), Some(col)) = (
        capture(&raw_chain, r"at line (\d+)"),
        capture(&raw_chain, r"column (\d+)"),
    ) {
        location.line = line.parse().ok();
        location.column = col.parse().ok();
    }
    if let Some(layer) = capture(&raw_chain, r"layer '([^']+)'") {
        location.layer = Some(layer);
    }
    if let Some(field) = capture(&raw_chain, r"unknown field `([^`]+)`") {
        location.field = Some(field.clone());
        observed = Some(json!(field));
        if let Some(list) = capture(
            &raw_chain,
            r"expected (?:one of )?([^\n]+?)(?: at line|\n|$)",
        ) {
            let names = list
                .split(',')
                .map(|part| part.trim().trim_matches('`').to_owned())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            expected = Some(json!(names));
        }
        recovery.push(format!(
            "Remove or rename the unknown field `{field}`; fields are strictly checked."
        ));
    }
    if let Some(field) = capture(&raw_chain, r"missing field `([^`]+)`") {
        location.field = Some(field.clone());
        recovery.push(format!("Add the required field `{field}`."));
    }
    if class.code == "asset.missing" {
        if let Some(field) = capture(
            &raw_chain,
            r"layer '[^']+' ([a-z_]+\.[a-z_]+) does not exist",
        ) {
            location.field = Some(field);
        }
        if let Some(path) = capture(&raw_chain, r"does not exist: (.+)") {
            observed = Some(json!(path.trim()));
        }
        recovery.push("Supply the asset at the referenced path (relative to the manifest) or correct the path; do not substitute different artwork without approval.".to_owned());
    }
    if class.code == "usage.invalid_argument" {
        recovery.push(
            "Run `vcr <command> --help` or `vcr capabilities --json` for valid arguments."
                .to_owned(),
        );
    }
    match class.code {
        "dependency.ffmpeg_missing" | "dependency.ffprobe_missing" => recovery.push(
            "Install ffmpeg/ffprobe on PATH (or use --ffmpeg sidecar if compiled in) and re-run `vcr doctor --json`.".to_owned(),
        ),
        "dependency.font_missing" => recovery
            .push("Restore assets/fonts/geist_pixel from the repository and re-run `vcr doctor --json`.".to_owned()),
        "backend.unavailable" => recovery.push(
            "Use --backend software for software-compatible scenes, or run on a machine with a GPU adapter.".to_owned(),
        ),
        "encoder.failed" => recovery.push(
            "Inspect stderr from the render, confirm free disk space and a writable output path, then re-run `vcr doctor --json`.".to_owned(),
        ),
        "io.write_failed" => recovery.push("Check the output path is writable and the disk is not full.".to_owned()),
        "io.read_failed" => recovery.push("Check the path exists and is readable.".to_owned()),
        "artifact.nonconformant" => recovery.push(
            "Do not deliver this file. Re-render, then run `vcr verify` against the manifest.".to_owned(),
        ),
        "manifest.validation_failed" | "manifest.schema_invalid" => recovery.push(
            "Correct the manifest at the reported location and re-run `vcr check --json`.".to_owned(),
        ),
        _ => {}
    }

    // Legacy suggestions (agent_errors) are folded in as recovery guidance.
    if matches!(class.category, "manifest") {
        if let Some(fix) = crate::agent_errors::suggest_fix_for_validation_error(&message) {
            recovery.push(fix.description);
        }
    }

    ErrorBody {
        code: class.code.to_owned(),
        category: class.category,
        message,
        summary,
        operation: operation.to_owned(),
        location: if location.is_empty() {
            None
        } else {
            Some(location)
        },
        expected,
        observed,
        recovery,
        retryable: class.retryable,
        exit_code: class.exit,
        details,
    }
}

fn capture(haystack: &str, pattern: &str) -> Option<String> {
    regex::Regex::new(pattern)
        .ok()?
        .captures(haystack)?
        .get(1)
        .map(|m| m.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn unknown_field_error_is_structured() {
        let error = anyhow!(
            "failed to decode manifest unk.vcr. unknown field `bogus_field`, expected one of `id`, `name` at line 8 column 5"
        );
        let body = classify_error("check", &error);
        assert_eq!(body.code, "manifest.schema_invalid");
        assert_eq!(body.exit_code, EXIT_MANIFEST);
        let location = body.location.expect("location");
        assert_eq!(location.file.as_deref(), Some("unk.vcr"));
        assert_eq!(location.field.as_deref(), Some("bogus_field"));
        assert_eq!(location.line, Some(8));
        assert_eq!(location.column, Some(5));
        assert_eq!(body.expected, Some(json!(["id", "name"])));
        assert!(!body.retryable);
    }

    #[test]
    fn coded_error_message_does_not_repeat_its_code() {
        let error = anyhow::Error::new(crate::error_codes::CodedError::usage(
            "usage.invalid_argument",
            "bad flag",
        ));
        let body = classify_error("check", &error);
        assert_eq!(body.code, "usage.invalid_argument");
        assert_eq!(body.message, "bad flag");
    }

    #[test]
    fn missing_asset_is_classified_with_observed_path() {
        let error = anyhow!("layer 'a' image.path does not exist: nope/missing.png");
        let body = classify_error("build", &error);
        assert_eq!(body.code, "asset.missing");
        assert_eq!(body.location.unwrap().layer.as_deref(), Some("a"));
        assert_eq!(body.observed, Some(json!("nope/missing.png")));
    }

    #[test]
    fn io_and_dependency_errors_map_to_documented_exit_codes() {
        let missing = anyhow!("ffmpeg was not found on PATH. Install it");
        assert_eq!(exit_code_for_error(&missing), EXIT_MISSING_DEPENDENCY);
        let io = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::NotFound))
            .context("failed to read manifest x.vcr");
        assert_eq!(exit_code_for_error(&io), EXIT_IO);
        let usage = anyhow!("invalid --set value");
        assert_eq!(exit_code_for_error(&usage), EXIT_USAGE);
    }

    #[test]
    fn envelope_status_drives_ok_and_exit() {
        let blocked = Envelope::new("prompt", Status::Blocked);
        assert!(!blocked.ok);
        assert_eq!(blocked.exit_code(), EXIT_BLOCKED);
        let ok = Envelope::ok("check", json!({"layers": 1}));
        assert!(ok.ok);
        assert_eq!(ok.exit_code(), 0);
        let line = ok.to_json_line();
        assert!(!line.contains('\n'));
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["contract"], CONTRACT_VERSION);
        assert_eq!(parsed["result"]["layers"], 1);
    }
}
