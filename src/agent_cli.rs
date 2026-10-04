//! Contract-shaped (`--json`) implementations of the agent workflow operations.
//!
//! Every function here prints exactly one JSON document (an [`Envelope`]) to stdout and returns
//! `Err(EarlyExit(code))` when the envelope's exit code is non-zero, so `main` neither prints a
//! second error nor loses the code. Human progress goes to stderr only.

use super::*;

use serde_json::{json, Value};
use vcr::agent_contract::{
    ArtifactRef, Basis, Diagnostic, Envelope, Location, Severity, Status, EXIT_BLOCKED,
    EXIT_MISSING_DEPENDENCY,
};

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
        "layers": manifest.layers.iter().map(|l| json!({"id": l.id(), "z_index": l.z_index()})).collect::<Vec<_>>(),
        "params": manifest.resolved_params,
        "overrides": manifest.applied_param_overrides,
    })
}

// ───────────────────────────── check / lint ─────────────────────────────

pub(super) fn check(manifest_path: &Path, set: &[String]) -> Result<()> {
    let manifest = load_manifest_with_overrides(manifest_path, set)?;
    let mut result = manifest_summary(&manifest, manifest_path);
    result["scope"] = json!("structural and semantic validity only; does not prove assets render, a backend can run the scene, or the design is good");
    finish(Envelope::ok("check", result))
}

pub(super) struct LintIssue {
    pub layer: String,
    pub code: &'static str,
    pub message: String,
}

fn lint_diagnostics(manifest_path: &Path, issues: &[LintIssue]) -> Vec<Diagnostic> {
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
                    _ => "Check start_time/end_time, opacity and keyframe ranges; the layer never becomes visible on sampled frames.",
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
                "scope": "heuristic timing/visibility checks sampled across the timeline",
            }))
            .with_diagnostics(diagnostics),
    )
}

// ───────────────────────────── doctor ─────────────────────────────

fn probe_tool(name: &str) -> Value {
    match std::process::Command::new(name).arg("-version").output() {
        Ok(out) if out.status.success() => json!({
            "available": true,
            "version": String::from_utf8_lossy(&out.stdout).lines().next().map(str::trim),
        }),
        Ok(out) => {
            json!({"available": false, "error": format!("`{name} -version` exited with {}", out.status)})
        }
        Err(error) => json!({"available": false, "error": error.to_string()}),
    }
}

pub(super) fn doctor_json() -> Result<()> {
    let ffmpeg = probe_tool("ffmpeg");
    let ffprobe = probe_tool("ffprobe");
    let fonts = match verify_geist_pixel_bundle(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(()) => json!({"bundle_ok": true}),
        Err(error) => json!({"bundle_ok": false, "error": error.to_string()}),
    };
    let probe_env = Environment {
        resolution: Resolution {
            width: 16,
            height: 16,
        },
        fps: 24,
        duration: ManifestDuration::Frames { frames: 1 },
        color_space: Default::default(),
        encoding: Default::default(),
    };
    let gpu = match pollster::block_on(Renderer::new_with_scene(
        &probe_env,
        &[],
        RenderSceneData::default(),
    )) {
        Ok(renderer) => {
            json!({"available": renderer.is_gpu_backend(), "detail": renderer.backend_reason()})
        }
        Err(error) => json!({"available": false, "detail": error.to_string()}),
    };

    let mut diagnostics = Vec::new();
    let mut missing = false;
    if ffmpeg["available"] != true {
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
    if ffprobe["available"] != true {
        missing = true;
        diagnostics.push(
            Diagnostic::new(
                Severity::Blocker,
                "doctor",
                "dependency.ffprobe_missing",
                "ffprobe not available; builds run it to check the encoded output",
            )
            .recover("Install ffmpeg (ships ffprobe) and ensure it is on PATH."),
        );
    }
    if fonts["bundle_ok"] != true {
        missing = true;
        diagnostics.push(
            Diagnostic::new(
                Severity::Blocker,
                "doctor",
                "dependency.font_missing",
                "bundled Geist Pixel fonts missing or modified",
            )
            .observed(fonts["error"].clone())
            .recover("Restore assets/fonts/geist_pixel from the repository."),
        );
    }
    if gpu["available"] != true {
        diagnostics.push(
            Diagnostic::new(
                Severity::Info,
                "doctor",
                "backend.gpu_unavailable",
                "no usable GPU adapter; only the software backend can run here",
            )
            .observed(gpu["detail"].clone()),
        );
    }
    let ready = !missing;
    let mut envelope = Envelope::new("doctor", if ready { Status::Ok } else { Status::Failed })
        .with_result(json!({
            "ready": ready,
            "runtime": {
                "ffmpeg": ffmpeg, "ffprobe": ffprobe, "fonts": fonts, "gpu": gpu,
                "software_backend_available": true,
            },
        }))
        .with_diagnostics(diagnostics);
    if missing {
        envelope = envelope.with_exit(EXIT_MISSING_DEPENDENCY);
    }
    finish(envelope)
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
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create output directory {}", parent.display())
            })?;
        }
        fs::write(path, yaml)
            .with_context(|| format!("failed to write translated output {}", path.display()))?;
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
    // `blocked` is a result, not a crash: exit 0 unless the caller asked for --strict.
    envelope = envelope.with_exit(if strict && status == Status::Blocked {
        EXIT_BLOCKED
    } else {
        0
    });
    finish(envelope)
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
            "layers": states.iter().map(|s| json!({
                "id": s.id, "z_index": s.z_index, "visible": s.visible && s.opacity > 0.0,
                "position": {"x": s.position.x, "y": s.position.y},
                "scale": {"x": s.scale.x, "y": s.scale.y},
                "rotation_degrees": s.rotation_degrees, "opacity": s.opacity,
            })).collect::<Vec<_>>(),
        }),
    ))
}
