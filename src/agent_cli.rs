//! Contract-shaped (`--json`) implementations of the agent workflow operations.
//!
//! Every function here prints exactly one JSON document (an [`Envelope`]) to stdout and returns
//! `Err(EarlyExit(code))` when the envelope's exit code is non-zero, so `main` neither prints a
//! second error nor loses the code. Human progress goes to stderr only.

use super::*;

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};
use vcr::agent_contract::{
    ArtifactRef, Basis, Diagnostic, Envelope, Location, Severity, Status, EXIT_BLOCKED,
    EXIT_MISSING_DEPENDENCY,
};
use vcr::inspect::{self, DiagnosticConfig};
use vcr::media_verify::{self, CheckStatus, Expectations, TransparencyExpectation};
use vcr::preflight::{self, PreflightReport};
use vcr::provenance;

/// Set when a `--json` operation is running: incidental human lines move to stderr so stdout
/// holds exactly one document.
static JSON_MODE: AtomicBool = AtomicBool::new(false);

pub(super) fn set_json_mode(on: bool) {
    JSON_MODE.store(on, Ordering::SeqCst);
}

/// Print a human status line: stdout normally, stderr in `--json` mode.
pub(super) fn human_line(args: std::fmt::Arguments<'_>) {
    if JSON_MODE.load(Ordering::SeqCst) {
        eprintln!("{args}");
    } else {
        println!("{args}");
    }
}

#[derive(Debug)]
pub(super) struct EarlyExit(pub u8);

impl std::fmt::Display for EarlyExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exit {}", self.0)
    }
}
impl std::error::Error for EarlyExit {}

pub(super) fn finish(envelope: Envelope) -> Result<()> {
    println!("{}", envelope.to_json_line());
    match envelope.exit_code() {
        0 => Ok(()),
        code => Err(anyhow::Error::new(EarlyExit(code))),
    }
}

fn finish_with_legacy(envelope: Envelope, legacy: &impl Serialize) -> Result<()> {
    println!("{}", envelope.to_json_line_with_legacy(legacy));
    match envelope.exit_code() {
        0 => Ok(()),
        code => Err(anyhow::Error::new(EarlyExit(code))),
    }
}

fn duration_seconds(manifest: &Manifest) -> f64 {
    f64::from(manifest.environment.total_frames()) / f64::from(manifest.environment.fps)
}

fn manifest_summary(manifest: &Manifest, path: &Path) -> Value {
    let env = &manifest.environment;
    let profile = env.encoding.prores_profile;
    json!({
        "path": path.display().to_string(),
        "manifest_hash": manifest.manifest_hash,
        "version": manifest.version,
        "environment": {
            "width": env.resolution.width,
            "height": env.resolution.height,
            "fps": env.fps,
            "frames": env.total_frames(),
            "duration_seconds": duration_seconds(manifest),
            "duration_rational": format!("{}/{}", env.total_frames(), env.fps),
            "color_space": format!("{:?}", env.color_space),
            "prores_profile": profile.to_ffmpeg_profile(),
            "alpha_capable_profile": profile.supports_alpha(),
        },
        "layers": manifest.layers.iter().map(|l| json!({"id": l.id(), "kind": l.kind(), "z_index": l.z_index()})).collect::<Vec<_>>(),
        "params": manifest.resolved_params,
        "overrides": manifest.applied_param_overrides,
    })
}

// ───────────────────────────── check / lint ─────────────────────────────

pub(super) fn check(manifest_path: &Path, set: &[String]) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set)?;
    let mut result = manifest_summary(&manifest, manifest_path);
    result["next"] = json!([
        "vcr lint --json",
        "vcr explain --json (backend preflight)",
        "vcr inspect --json"
    ]);
    result["scope"] = json!("structural and semantic validity only; does not prove assets render, a backend can run the scene, or the design is good");
    finish(Envelope::ok("check", result))
}

pub(super) struct LintIssue {
    pub layer: String,
    pub code: &'static str,
    pub message: String,
}

pub(super) fn lint_diagnostics(manifest_path: &Path, issues: &[LintIssue]) -> Vec<Diagnostic> {
    issues
        .iter()
        .map(|issue| {
            Diagnostic::new(Severity::Warning, "lint", issue.code, issue.message.clone())
                .at(Location {
                    file: Some(manifest_path.display().to_string()),
                    layer: Some(issue.layer.clone()),
                    ..Location::default()
                })
                .basis(Basis::Exact)
                .recover(match issue.code {
                    "lint.alpha_blocked" => "Reduce or remove the opaque full-frame bottom layer if transparency is required.",
                    _ => "Check start_time/end_time, opacity and keyframe ranges; layer never becomes visible on sampled frames.",
                })
        })
        .collect()
}

pub(super) fn lint_json(manifest_path: &Path, set: &[String]) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set)?;
    let issues = collect_lint_issues(&manifest)?;
    let diagnostics = lint_diagnostics(manifest_path, &issues);
    let status = if issues.is_empty() {
        Status::Ok
    } else {
        Status::Failed
    };
    finish(
        Envelope::new("lint", status)
            .with_result(json!({
                "manifest": manifest_path.display().to_string(),
                "manifest_hash": manifest.manifest_hash,
                "issue_count": issues.len(),
                "scope": "heuristic timing/visibility checks sampled across the timeline; use `vcr inspect` for rendered evidence",
            }))
            .with_diagnostics(diagnostics),
    )
}

// ───────────────────────────── doctor / capabilities ─────────────────────────────

pub(super) fn doctor_json() -> Result<()> {
    let runtime = preflight::probe_runtime();
    let mut diagnostics = Vec::new();
    let mut missing = false;
    if !runtime.ffmpeg.available {
        missing = true;
        diagnostics.push(
            Diagnostic::new(
                Severity::Blocker,
                "doctor",
                "dependency.ffmpeg_missing",
                "ffmpeg not available",
            )
            .recover("Install ffmpeg and ensure it is on PATH."),
        );
    }
    if !runtime.ffprobe.available {
        missing = true;
        diagnostics.push(
            Diagnostic::new(
                Severity::Blocker,
                "doctor",
                "dependency.ffprobe_missing",
                "ffprobe not available; builds and verification need it",
            )
            .recover("Install ffmpeg (ships ffprobe) and ensure it is on PATH."),
        );
    }
    if !runtime.fonts.bundle_ok {
        missing = true;
        diagnostics.push(
            Diagnostic::new(
                Severity::Blocker,
                "doctor",
                "dependency.font_missing",
                "bundled Geist Pixel fonts missing or modified",
            )
            .observed(runtime.fonts.error.clone().unwrap_or_default())
            .recover("Restore assets/fonts/geist_pixel from the repository."),
        );
    }
    if !runtime.gpu.available {
        diagnostics.push(
            Diagnostic::new(
                Severity::Info,
                "doctor",
                "backend.gpu_unavailable",
                "no usable GPU adapter; only the software backend can run here",
            )
            .observed(runtime.gpu.detail.clone()),
        );
    }
    let ready = !missing;
    let mut envelope = Envelope::new("doctor", if ready { Status::Ok } else { Status::Failed })
        .with_result(json!({ "ready": ready, "runtime": runtime }))
        .with_diagnostics(diagnostics);
    if missing {
        envelope = envelope.with_exit(EXIT_MISSING_DEPENDENCY);
    }
    finish(envelope)
}

pub(super) fn capabilities(include_schema: bool, json_out: bool) -> Result<()> {
    let runtime = preflight::probe_runtime();
    let commands = Cli::command()
        .get_subcommands()
        .map(|sub| {
            json!({
                "name": sub.get_name(),
                "about": sub.get_about().map(|a| a.to_string()),
                "json": sub.get_arguments().any(|a| a.get_id() == "json"),
            })
        })
        .collect::<Vec<_>>();
    let result = vcr::capabilities::discover(&runtime, commands, include_schema);
    if json_out {
        return finish(Envelope::ok("capabilities", result));
    }
    let engine = vcr::agent_contract::EngineIdentity::current();
    println!(
        "vcr {} contract {}",
        engine.display_version(),
        vcr::agent_contract::CONTRACT_VERSION
    );
    println!(
        "software backend: usable | gpu: {}",
        if runtime.gpu.available {
            "usable"
        } else {
            "unavailable"
        }
    );
    println!(
        "ffmpeg: {} | ffprobe: {} | fonts: {}",
        if runtime.ffmpeg.available {
            "ok"
        } else {
            "MISSING"
        },
        if runtime.ffprobe.available {
            "ok"
        } else {
            "MISSING"
        },
        if runtime.fonts.bundle_ok {
            "ok"
        } else {
            "INVALID"
        }
    );
    println!("Run `vcr capabilities --json [--schema]` for the full machine-readable contract.");
    Ok(())
}

// ───────────────────────────── prompt ─────────────────────────────

fn creative_inputs(raw: &str, spec: &vcr::prompt_gate::NormalizedSpec) -> Value {
    let quoted = regex::Regex::new(r#"["“][^"”]{2,}["”]"#)
        .map(|r| r.is_match(raw))
        .unwrap_or(false);
    let hex = regex::Regex::new(r"#[0-9a-fA-F]{6}\b")
        .map(|r| r.is_match(raw))
        .unwrap_or(false);
    let lower = raw.to_ascii_lowercase();
    let font = lower.contains("geistpixel") || lower.contains("font");
    let assets = regex::Regex::new(r"[\w./-]+\.(png|jpe?g|webp|svg|json)")
        .map(|r| r.is_match(raw))
        .unwrap_or(false)
        || !spec.input.is_empty();
    let state = |specified: bool| {
        if specified {
            "specified"
        } else {
            "unspecified"
        }
    };
    json!({
        "detected_by": "heuristic over the brief; normalization never invents creative content",
        "text_content": state(quoted),
        "palette_or_brand_colors": state(hex),
        "typeface": state(font),
        "supplied_assets": state(assets),
        "rule": "unspecified items are not defaults: take them from the requester or get approval before authoring"
    })
}

pub(super) fn prompt_json(
    text: Option<&str>,
    input_file: Option<&Path>,
    output_file: Option<&Path>,
    strict: bool,
) -> Result<()> {
    let raw = match (text, input_file) {
        (Some(inline), None) => inline.to_owned(),
        (None, Some(path)) => fs::read_to_string(path)
            .with_context(|| format!("failed to read input file {}", path.display()))?,
        (Some(_), Some(_)) => bail!("provide only one of --text or --in"),
        (None, None) => bail!("missing input: provide --text or --in"),
    };
    let translated = translate_to_standard_prompt(&raw)?;
    let blockers = translated
        .unknowns_and_fixes
        .iter()
        .map(|unknown| {
            Diagnostic::new(
                Severity::Blocker,
                "prompt",
                "prompt.unresolved",
                unknown.issue.clone(),
            )
            .observed(unknown.why_it_matters.clone())
            .recover(unknown.proposed_fix.clone())
        })
        .collect::<Vec<_>>();
    let status = if blockers.is_empty() {
        Status::Ok
    } else {
        Status::Blocked
    };
    let creative = creative_inputs(&raw, &translated.normalized_spec);
    let mut artifacts = Vec::new();
    if let Some(path) = output_file {
        let yaml = serde_yaml::to_string(&translated)
            .context("failed to serialize translated prompt YAML")?;
        provenance::atomic_write(path, yaml.as_bytes())?;
        artifacts.push(ArtifactRef::new("normalized_spec", path).with_digest(path));
    }
    let mut envelope = Envelope::new("prompt", status)
        .with_result(json!({
            "standardized_vcr_prompt": translated.standardized_vcr_prompt,
            "normalized_spec": translated.normalized_spec,
            "defaults_applied": translated.defaults_applied,
            "assumptions_applied": translated.assumptions_applied,
            "acceptance_checks": translated.acceptance_checks,
            "unknowns_and_fixes": translated.unknowns_and_fixes,
            "creative_inputs": creative,
            "scope": "normalization states what the brief specifies and what is unresolved; it does not prove a scene exists, a backend can run it, or assets exist",
        }))
        .with_diagnostics(blockers)
        .with_artifacts(artifacts);
    if strict && status == Status::Blocked {
        envelope = envelope.with_exit(EXIT_BLOCKED);
    } else if status == Status::Blocked {
        envelope = envelope.with_exit(0);
    }
    finish(envelope)
}

// ───────────────────────────── explain / preflight ─────────────────────────────

pub(super) fn preflight_report(
    manifest: &Manifest,
    requested: BackendArg,
    output: Option<&Path>,
) -> PreflightReport {
    preflight::run_preflight(
        manifest,
        backend_label(requested),
        output,
        preflight::probe_runtime(),
    )
}

// ───────────────────────────── dump ─────────────────────────────

pub(super) fn dump_json(
    manifest_path: &Path,
    frame: Option<u32>,
    time: Option<f32>,
    set: &[String],
) -> Result<()> {
    if frame.is_some() && time.is_some() {
        bail!("use either --frame or --time, not both");
    }
    let manifest = load_manifest_with_overrides(manifest_path, set)?;
    let total = manifest.environment.total_frames();
    let fps = manifest.environment.fps;
    let selected = match (frame, time) {
        (Some(f), _) => f,
        (None, Some(t)) => {
            if !t.is_finite() || t < 0.0 {
                bail!("--time must be a finite non-negative value");
            }
            (t * fps as f32).round() as u32
        }
        _ => 0,
    };
    if selected >= total {
        bail!("frame {selected} is out of range for {total} total frames");
    }
    let states = evaluate_manifest_layers_at_frame(&manifest, selected)?;
    finish(Envelope::ok(
        "dump",
        json!({
            "frame": selected,
            "time_seconds": f64::from(selected) / f64::from(fps),
            "time_rational": format!("{selected}/{fps}"),
            "layers": states.iter().map(layer_state_json).collect::<Vec<_>>(),
        }),
    ))
}

fn layer_state_json(s: &vcr::timeline::LayerDebugState) -> Value {
    json!({
        "id": s.id, "z_index": s.z_index, "visible": s.visible && s.opacity > 0.0,
        "position": {"x": s.position.x, "y": s.position.y},
        "scale": {"x": s.scale.x, "y": s.scale.y},
        "rotation_degrees": s.rotation_degrees, "opacity": s.opacity,
    })
}

// ───────────────────────────── verify ─────────────────────────────

#[derive(Debug, Default)]
pub(super) struct VerifyArgs {
    pub manifest: Option<PathBuf>,
    pub set: Vec<String>,
    pub expect_width: Option<u32>,
    pub expect_height: Option<u32>,
    pub expect_fps: Option<u32>,
    pub expect_frames: Option<u32>,
    pub expect_container: Option<String>,
    pub expect_codec: Option<String>,
    pub expect_profile: Option<String>,
    pub expect_alpha_capable: Option<bool>,
    pub expect_transparency: Option<String>,
    pub decode_frames: Option<String>,
    pub require_provenance: bool,
}

#[derive(Serialize)]
struct VerifyLegacy {
    file_path: PathBuf,
    hash: String,
    tool_version: String,
    backend: Option<String>,
}

fn read_provenance(output: &Path) -> Option<Value> {
    let raw = fs::read(provenance::provenance_path_for(output)).ok()?;
    serde_json::from_slice(&raw).ok()
}

pub(super) fn verify(output_file: &Path, args: &VerifyArgs, json_out: bool) -> Result<()> {
    if !output_file.exists() {
        bail!("file not found: {}", output_file.display());
    }
    let sha = vcr::agent_contract::sha256_file(output_file)
        .with_context(|| format!("failed to read {}", output_file.display()))?;
    let bytes = fs::metadata(output_file).map(|m| m.len()).unwrap_or(0);
    let provenance_doc = read_provenance(output_file);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut expected = Expectations::default();
    let mut sources: Vec<&str> = Vec::new();

    // 1. Recorded at production time (lowest precedence).
    if let Some(p) = &provenance_doc {
        if let Some(rec) = p.get("verification").and_then(|v| v.get("expectations")) {
            expected.width = rec.get("width").and_then(Value::as_u64).map(|v| v as u32);
            expected.height = rec.get("height").and_then(Value::as_u64).map(|v| v as u32);
            expected.fps = rec.get("fps").and_then(Value::as_u64).map(|v| v as u32);
            expected.frame_count = rec
                .get("frame_count")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            expected.container = rec
                .get("container")
                .and_then(Value::as_str)
                .map(str::to_owned);
            expected.codec = rec.get("codec").and_then(Value::as_str).map(str::to_owned);
            expected.profile = rec
                .get("profile")
                .and_then(Value::as_str)
                .map(str::to_owned);
            expected.alpha_capable = rec.get("alpha_capable").and_then(Value::as_bool);
            sources.push("provenance");
        }
    }
    // 2. Current manifest (stale detection + expectations).
    let mut freshness = json!({"checked": false});
    if let Some(manifest_path) = &args.manifest {
        let manifest = load_manifest_with_overrides(manifest_path, &args.set)?;
        let env = &manifest.environment;
        expected.width = Some(env.resolution.width);
        expected.height = Some(env.resolution.height);
        expected.fps = Some(env.fps);
        expected.frame_count = Some(env.total_frames());
        expected.container.get_or_insert_with(|| "mov".to_owned());
        expected.codec.get_or_insert_with(|| "prores".to_owned());
        if provenance_doc.is_none() {
            expected.profile = Some(env.encoding.prores_profile.to_ffmpeg_profile().to_owned());
            expected.alpha_capable = Some(env.encoding.prores_profile.supports_alpha());
        }
        sources.push("manifest");
        let recorded = provenance_doc.as_ref().and_then(|p| {
            p.get("manifest")?
                .get("resolved_manifest_hash")?
                .as_str()
                .map(str::to_owned)
        });
        let current = manifest.manifest_hash.clone();
        let fresh = recorded.as_ref().map(|r| *r == current);
        freshness = json!({"checked": true, "recorded_manifest_hash": recorded, "current_manifest_hash": current, "fresh": fresh});
        if fresh == Some(false) {
            diagnostics.push(
                Diagnostic::new(
                    Severity::Error,
                    "verify",
                    "verify.stale_artifact",
                    "output was rendered from a different manifest/params than the current ones",
                )
                .expected(json!(current))
                .observed(json!(recorded))
                .recover("Re-render, then verify again. Do not deliver this file."),
            );
        }
    }
    // 3. Explicit flags (highest precedence).
    macro_rules! flag {
        ($field:ident, $arg:ident) => {
            if let Some(v) = &args.$arg {
                expected.$field = Some(v.clone());
                if !sources.contains(&"flags") {
                    sources.push("flags");
                }
            }
        };
    }
    flag!(width, expect_width);
    flag!(height, expect_height);
    flag!(fps, expect_fps);
    flag!(frame_count, expect_frames);
    flag!(container, expect_container);
    flag!(codec, expect_codec);
    flag!(profile, expect_profile);
    flag!(alpha_capable, expect_alpha_capable);
    if let Some(t) = &args.expect_transparency {
        expected.transparency = t.parse::<TransparencyExpectation>().map_err(|e| {
            anyhow::Error::new(vcr::error_codes::CodedError::usage(
                "invalid_expectation",
                e.to_string(),
            ))
        })?;
        sources.push("flags");
    }

    // Provenance integrity.
    let mut producer = Value::Null;
    match &provenance_doc {
        Some(p) => {
            producer = p.get("engine").cloned().unwrap_or(Value::Null);
            let recorded_sha = p.pointer("/output/sha256").and_then(Value::as_str);
            if recorded_sha != Some(sha.as_str()) {
                diagnostics.push(
                    Diagnostic::new(Severity::Error, "verify", "verify.modified_since_render", "file bytes differ from the output recorded in provenance (modified, replaced, or incomplete)")
                        .expected(json!(recorded_sha)).observed(json!(sha))
                        .recover("Re-render; do not deliver this file."),
                );
            }
            if p.get("status").and_then(Value::as_str) != Some("complete") {
                diagnostics.push(Diagnostic::new(
                    Severity::Error,
                    "verify",
                    "verify.incomplete_export",
                    "provenance does not mark this export complete",
                ));
            }
        }
        None => {
            let sev = if args.require_provenance {
                Severity::Error
            } else {
                Severity::Warning
            };
            diagnostics.push(
                Diagnostic::new(sev, "verify", "verify.no_provenance", "no provenance record next to the file; its producer and freshness cannot be established")
                    .recover("Re-render with this engine to produce <file>.provenance.json."),
            );
        }
    }

    let decode_budget = match args.decode_frames.as_deref() {
        None => None, // all frames
        Some("all") => None,
        Some(n) => Some(n.parse::<u64>().map_err(|_| {
            anyhow::Error::new(vcr::error_codes::CodedError::usage(
                "invalid_expectation",
                format!("--decode-frames must be a number or 'all', got '{n}'"),
            ))
        })?),
    };
    let report = match media_verify::verify_media(output_file, &expected, decode_budget) {
        Ok(report) => report,
        Err(error) if !error.to_string().contains("was not found on PATH") => {
            // A file ffprobe cannot read is a verification *finding* (truncated, corrupt, not media).
            diagnostics.push(
                Diagnostic::new(Severity::Error, "verify", "verify.unreadable_media", "the file could not be read as media; it is incomplete, corrupt, or not a video")
                    .observed(error.to_string().lines().next().unwrap_or("").to_owned())
                    .recover("Re-render; do not deliver this file."),
            );
            let envelope = Envelope::new("verify", Status::Failed)
                .with_result(json!({
                    "file": output_file.display().to_string(), "sha256": sha, "bytes": bytes,
                    "verifier": vcr::agent_contract::EngineIdentity::current(),
                    "producer": producer, "expectation_sources": sources, "freshness": freshness,
                    "media": Value::Null,
                }))
                .with_diagnostics(diagnostics)
                .with_artifacts(vec![
                    ArtifactRef::new("output", output_file).with_digest(output_file)
                ]);
            if json_out {
                let legacy = VerifyLegacy {
                    file_path: output_file.to_path_buf(),
                    hash: sha,
                    tool_version: version_string(),
                    backend: None,
                };
                return finish_with_legacy(envelope, &legacy);
            }
            println!("FAIL: {} could not be read as media", output_file.display());
            return Err(anyhow::Error::new(EarlyExit(3)));
        }
        Err(error) => return Err(error),
    };
    for c in report
        .checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
    {
        let mut d = Diagnostic::new(
            Severity::Error,
            "verify",
            format!("verify.{}_mismatch", c.id),
            format!("{} does not match the expectation", c.id),
        )
        .basis(Basis::Exact);
        if let Some(e) = &c.expected {
            d = d.expected(e.clone());
        }
        if let Some(o) = &c.observed {
            d = d.observed(o.clone());
        }
        diagnostics.push(d);
    }
    let failed = diagnostics.iter().any(Diagnostic::is_blocking);
    let version = version_string();
    let backend = provenance_doc
        .as_ref()
        .and_then(|p| p.pointer("/backend/name")?.as_str().map(str::to_owned));
    let artifacts = vec![ArtifactRef::new("output", output_file).with_digest(output_file)];
    let checked_any_expectation = !sources.is_empty();
    let result = json!({
        "file": output_file.display().to_string(),
        "sha256": sha, "bytes": bytes,
        "verifier": vcr::agent_contract::EngineIdentity::current(),
        "producer": producer,
        "producer_note": "`engine` (top level) is the VERIFIER; `producer` is the engine recorded in provenance. They are not the same evidence.",
        "expectation_sources": sources,
        "freshness": freshness,
        "media": report,
        "limits": if checked_any_expectation { json!([]) } else { json!(["no expectations or provenance supplied: only file integrity and probe facts are reported"]) },
    });
    let status = if failed { Status::Failed } else { Status::Ok };
    let envelope = Envelope::new("verify", status)
        .with_result(result)
        .with_diagnostics(diagnostics)
        .with_artifacts(artifacts);
    if json_out {
        let legacy = VerifyLegacy {
            file_path: output_file.to_path_buf(),
            hash: sha,
            tool_version: version,
            backend,
        };
        return finish_with_legacy(envelope, &legacy);
    }
    println!("Path: {}", output_file.display());
    println!("Hash: {sha}");
    println!("Version: {version}");
    for c in &report_checks_for_humans(&envelope) {
        println!("{c}");
    }
    if failed {
        Err(anyhow::Error::new(EarlyExit(3)))
    } else {
        Ok(())
    }
}

fn report_checks_for_humans(envelope: &Envelope) -> Vec<String> {
    let mut lines = Vec::new();
    for d in &envelope.diagnostics {
        let detail = match (&d.expected, &d.observed) {
            (Some(e), Some(o)) => format!(" [expected {e}, observed {o}]"),
            (None, Some(o)) => format!(" [observed {o}]"),
            _ => String::new(),
        };
        lines.push(format!(
            "{}: {} ({}){detail}",
            match d.severity {
                Severity::Warning => "warn",
                _ => "FAIL",
            },
            d.message,
            d.code
        ));
    }
    if lines.is_empty() {
        lines.push("Verification: all checks passed".to_owned());
    }
    lines
}

// ───────────────────────────── build / render envelope ─────────────────────────────

pub(super) fn build_envelope(
    operation: &str,
    manifest_path: &Path,
    output_path: &Path,
    build: &BuildResult,
    elapsed_ms: u64,
) -> Envelope {
    let prov_path = provenance::provenance_path_for(output_path);
    let meta_path = metadata_sidecar_for_file(output_path);
    let output_hash = vcr::agent_contract::sha256_file(output_path).unwrap_or_default();
    let result = json!({
        "manifest": manifest_path.display().to_string(),
        "backend": build.backend_name,
        "frame_count": build.frame_count,
        "frame_hash": build.frame_hash,
        "output_hash": output_hash,
        "duration_ms": elapsed_ms,
        "output": output_path.display().to_string(),
        "published": "atomic",
        "verification": build.verification,
        "next": [format!("vcr verify {} --manifest {} --json", output_path.display(), manifest_path.display())],
    });
    Envelope::ok(operation, result).with_artifacts(vec![
        ArtifactRef::new("output", output_path).with_digest(output_path),
        ArtifactRef::new("metadata", &meta_path).with_digest(&meta_path),
        ArtifactRef::new("provenance", &prov_path).with_digest(&prov_path),
    ])
}

#[derive(Serialize)]
pub(super) struct RenderLegacy {
    pub manifest: PathBuf,
    pub backend: String,
    pub frame_count: u32,
    pub frame_hash: String,
    pub output_hash: String,
    pub duration_ms: u64,
}

pub(super) fn finish_render(envelope: Envelope, legacy: RenderLegacy) -> Result<()> {
    finish_with_legacy(envelope, &legacy)
}

// ───────────────────────────── inspect ─────────────────────────────

pub(super) struct InspectArgs {
    pub samples: usize,
    pub output_dir: PathBuf,
    pub preview_width: u32,
    pub safe_margin: f64,
}

pub(super) fn inspect(
    manifest_path: &Path,
    set: &[String],
    args: &InspectArgs,
    backend: BackendArg,
    ascii_overrides: Option<&AsciiRuntimeOverrides>,
    json_out: bool,
) -> Result<()> {
    if !(0.0..0.5).contains(&args.safe_margin) {
        bail!(
            "--safe-margin must be in [0, 0.5), got {}",
            args.safe_margin
        );
    }
    if args.samples == 0 {
        bail!("--samples must be > 0");
    }
    let manifest = load_manifest_with_overrides(manifest_path, set)?;
    let env = manifest.environment.clone();
    let (width, height) = (env.resolution.width, env.resolution.height);
    let timeline = inspect::build_timeline(&manifest)?;
    let points = inspect::plan_samples(&timeline, args.samples);
    let config = DiagnosticConfig {
        safe_margin: args.safe_margin,
        fps: env.fps,
    };

    fs::create_dir_all(&args.output_dir).with_context(|| {
        format!(
            "failed to create inspect directory {}",
            args.output_dir.display()
        )
    })?;

    let mut scene = RenderSceneData::from_manifest(&manifest);
    if let Some(o) = ascii_overrides {
        scene = scene.with_ascii_overrides(o.clone());
    }
    let mut renderer = create_renderer(&env, &manifest.layers, scene, backend)?;
    let backend_name = renderer.backend_name().to_owned();

    // One isolated renderer per layer: exact per-layer pixel bounds for any layer type.
    const MAX_SOLO_LAYERS: usize = 32;
    let mut limits = vec![
        "layer bounds are measured from each layer rendered alone, so they are exact for the pixels drawn (not an estimate of layout intent)".to_owned(),
        "layer-state motion detection does not see changes inside a layer (procedural colour, text, sequence frames); evenly spaced samples cover those".to_owned(),
        "this reports mechanical facts; whether the design is readable, balanced or on-brief is a judgment for review of the contact sheet".to_owned(),
    ];
    let mut solo: Vec<(String, Option<Renderer>)> = Vec::new();
    for layer in manifest.layers.iter().take(MAX_SOLO_LAYERS) {
        let mut single = manifest.clone();
        single.layers = vec![layer.clone()];
        let single_scene = RenderSceneData::from_manifest(&single);
        let made = create_renderer(&env, &single.layers, single_scene, backend).ok();
        if made.is_none() {
            limits.push(format!(
                "layer '{}' could not be rendered in isolation; no bounds reported",
                layer.id()
            ));
        }
        solo.push((layer.id().to_owned(), made));
    }
    if manifest.layers.len() > MAX_SOLO_LAYERS {
        limits.push(format!(
            "only the first {MAX_SOLO_LAYERS} layers were measured in isolation"
        ));
    }

    let preview_w = args.preview_width.min(width).max(1);
    let preview_h = ((u64::from(height) * u64::from(preview_w)) / u64::from(width)).max(1) as u32;
    let precision = f64::from(width) / f64::from(preview_w);

    let mut diagnostics = inspect::timeline_diagnostics(&timeline, &config);
    let mut samples_json = Vec::new();
    let mut artifacts = Vec::new();
    let mut tiles = Vec::new();
    for point in &points {
        let rgba = renderer.render_frame_rgba(point.frame)?;
        let stats = inspect::pixel_stats(&rgba, width, height);
        let small = inspect::downscale_rgba(rgba, width, height, preview_w, preview_h);
        let png = args
            .output_dir
            .join(format!("sample_{:06}.png", point.frame));
        save_rgba_png(&png, preview_w, preview_h, small.clone())?;
        artifacts.push(
            ArtifactRef::new("preview_frame", &png).at_frame(point.frame, point.time_seconds),
        );
        if let Some(img) = RgbaImage::from_raw(preview_w, preview_h, small) {
            tiles.push((img, point.frame, point.time_seconds));
        }

        let states = evaluate_manifest_layers_at_frame(&manifest, point.frame)?;
        let mut layer_bounds = Vec::new();
        for state in states.iter().filter(|s| s.visible && s.opacity > 0.0) {
            let Some((_, Some(r))) = solo.iter_mut().find(|(id, _)| *id == state.id) else {
                continue;
            };
            let solo_rgba = r.render_frame_rgba(point.frame)?;
            let lstats = inspect::pixel_stats(&solo_rgba, width, height);
            diagnostics.extend(inspect::bounds_diagnostics(
                &state.id,
                point.frame,
                point.phase,
                &lstats,
                &config,
                1.0,
            ));
            layer_bounds.push(json!({"id": state.id, "bbox": lstats.alpha_bbox, "edge_touch": lstats.edge_touch, "covered_fraction": lstats.covered_fraction}));
        }
        samples_json.push(json!({
            "frame": point.frame, "time_seconds": point.time_seconds, "time_rational": point.time_rational,
            "phase": point.phase, "roles": point.roles,
            "preview": png.display().to_string(),
            "layers": states.iter().map(layer_state_json).collect::<Vec<_>>(),
            "layer_bounds": layer_bounds,
            "pixels": stats,
        }));
    }

    // Small-text heuristic (approximate): font_size × evaluated scale versus canvas height.
    for layer in &manifest.layers {
        if let Layer::Text(t) = layer {
            let min_px = 0.022 * f32::from(height as u16);
            let mut seen_scale = None;
            for point in &points {
                if let Some(s) = evaluate_manifest_layers_at_frame(&manifest, point.frame)?
                    .into_iter()
                    .find(|s| s.id == t.common.id && s.visible && s.opacity > 0.0)
                {
                    seen_scale = Some(s.scale.y.abs().max(s.scale.x.abs()));
                    if t.text.font_size * s.scale.y.abs() >= min_px {
                        seen_scale = None;
                        break;
                    }
                }
            }
            if let Some(scale) = seen_scale {
                diagnostics.push(
                    Diagnostic::new(Severity::Warning, "inspect", "text.small", format!("text layer '{}' renders at about {:.0}px tall at its largest sampled scale ({:.2}); below ~2.2% of canvas height", t.common.id, t.text.font_size * scale, scale))
                        .at(Location::layer(t.common.id.clone())).basis(Basis::Approximate)
                        .recover("Judgment call: confirm legibility against the brief's delivery size."),
                );
            }
        }
    }

    let sheet = inspect::contact_sheet(&tiles, 3, 320.min(preview_w));
    let sheet_path = args.output_dir.join("contact_sheet.png");
    sheet
        .save(&sheet_path)
        .with_context(|| format!("failed to write {}", sheet_path.display()))?;
    artifacts.insert(0, ArtifactRef::new("contact_sheet", &sheet_path));

    let blocking = diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error || d.severity == Severity::Blocker);
    let warned = diagnostics.iter().any(|d| d.severity == Severity::Warning);
    let result = json!({
        "manifest": manifest_summary(&manifest, manifest_path),
        "backend": {"name": backend_name, "reason": renderer.backend_reason()},
        "timeline": timeline,
        "samples": samples_json,
        "contact_sheet": {"path": sheet_path.display().to_string(), "columns": 3, "order": points.iter().map(|p| p.frame).collect::<Vec<_>>()},
        "preview": {"width": preview_w, "height": preview_h, "downscale_from": [width, height], "bounds_precision_px": 1.0, "preview_precision_px": precision},
        "summary": {"errors": blocking, "warnings": warned, "diagnostic_count": diagnostics.len()},
        "revise_with": "edit the layer by its stable `id` in the manifest, or change declared params with --set name=value; then run `vcr inspect` again",
        "limits": limits,
    });
    let mut envelope = Envelope::new(
        "inspect",
        if blocking { Status::Failed } else { Status::Ok },
    )
    .with_result(result)
    .with_diagnostics(diagnostics)
    .with_artifacts(artifacts);
    // Keep a copy of the document next to the images.
    let doc_path = args.output_dir.join("inspection.json");
    provenance::atomic_write(&doc_path, envelope.to_json_line().as_bytes())?;
    envelope
        .artifacts
        .push(ArtifactRef::new("inspection", &doc_path));
    if json_out {
        return finish(envelope);
    }
    println!(
        "Inspected {} samples -> {}",
        points.len(),
        args.output_dir.display()
    );
    println!("Contact sheet: {}", sheet_path.display());
    for d in &envelope.diagnostics {
        println!("[{:?}] {}: {}", d.severity, d.code, d.message);
    }
    if blocking {
        Err(anyhow::Error::new(EarlyExit(3)))
    } else {
        Ok(())
    }
}
