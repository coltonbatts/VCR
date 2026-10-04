//! Backend/runtime preflight: "can this machine render this scene with this backend?"
//!
//! This is a distinct stage from manifest validation (`check`), lint, and artifact verification.
//! It answers only: given the requested backend and the tools installed *here*, will the render
//! honor the manifest, or would it silently drop something? `vcr explain --json` surfaces it.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::agent_contract::{Diagnostic, Location, Severity};
use crate::font_assets::{resolve_font_family, verify_geist_pixel_bundle, FONT_FAMILY_ALIASES};
use crate::renderer::{software_unsupported_layers, Renderer, SOFTWARE_SUPPORTED_LAYER_TYPES};
use crate::schema::{Environment, Layer, Manifest, Resolution};
use crate::timeline::RenderSceneData;

#[derive(Debug, Clone, Serialize)]
pub struct ToolProbe {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuProbe {
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FontProbe {
    pub bundle_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbe {
    pub ffmpeg: ToolProbe,
    pub ffprobe: ToolProbe,
    pub fonts: FontProbe,
    pub gpu: GpuProbe,
    /// The CPU (software) backend is always compiled in; it is usable whenever the scene only
    /// contains software-supported layers and no GPU-only features.
    pub software_backend_available: bool,
}

pub fn probe_tool(name: &str) -> ToolProbe {
    match Command::new(name).arg("-version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(|line| line.trim().to_owned());
            ToolProbe {
                available: true,
                version,
                path: find_on_path(name),
                error: None,
            }
        }
        Ok(output) => ToolProbe {
            available: false,
            version: None,
            path: find_on_path(name),
            error: Some(format!("`{name} -version` exited with {}", output.status)),
        },
        Err(error) => ToolProbe {
            available: false,
            version: None,
            path: None,
            error: Some(error.to_string()),
        },
    }
}

fn find_on_path(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

pub fn probe_gpu() -> GpuProbe {
    let env = Environment {
        resolution: Resolution {
            width: 16,
            height: 16,
        },
        fps: 24,
        duration: crate::schema::Duration::Frames { frames: 1 },
        color_space: Default::default(),
        encoding: Default::default(),
    };
    match pollster::block_on(Renderer::new_with_scene(
        &env,
        &[],
        RenderSceneData::default(),
    )) {
        Ok(renderer) => GpuProbe {
            available: renderer.is_gpu_backend(),
            detail: renderer.backend_reason().to_owned(),
        },
        Err(error) => GpuProbe {
            available: false,
            detail: error.to_string(),
        },
    }
}

pub fn probe_runtime() -> RuntimeProbe {
    let fonts = match verify_geist_pixel_bundle(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(()) => FontProbe {
            bundle_ok: true,
            error: None,
        },
        Err(error) => FontProbe {
            bundle_ok: false,
            error: Some(error.to_string()),
        },
    };
    RuntimeProbe {
        ffmpeg: probe_tool("ffmpeg"),
        ffprobe: probe_tool("ffprobe"),
        fonts,
        gpu: probe_gpu(),
        software_backend_available: true,
    }
}

/// Something in the manifest the software backend cannot honor.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SoftwareIncompatibility {
    /// `layer` | `feature`
    pub scope: &'static str,
    /// Layer id for layers, feature name (`post`, `ascii_post`) for features.
    pub id: String,
    pub kind: String,
}

pub fn software_incompatibilities(manifest: &Manifest) -> Vec<SoftwareIncompatibility> {
    let mut found = software_unsupported_layers(&manifest.layers)
        .into_iter()
        .map(|layer| SoftwareIncompatibility {
            scope: "layer",
            id: layer.id.to_owned(),
            kind: layer.kind.to_owned(),
        })
        .collect::<Vec<_>>();
    if !manifest.post.is_empty() {
        found.push(SoftwareIncompatibility {
            scope: "feature",
            id: "post".to_owned(),
            kind: "post_effects".to_owned(),
        });
    }
    if manifest
        .ascii_post
        .as_ref()
        .is_some_and(|post| post.enabled)
    {
        found.push(SoftwareIncompatibility {
            scope: "feature",
            id: "ascii_post".to_owned(),
            kind: "ascii_post".to_owned(),
        });
    }
    found
}

/// Features (not layers) that only the GPU pipeline implements. The software renderer ignores
/// them silently, so callers must treat a software render of such a scene as unsupported.
pub fn software_ignored_features(manifest: &Manifest) -> Vec<SoftwareIncompatibility> {
    software_incompatibilities(manifest)
        .into_iter()
        .filter(|item| item.scope == "feature")
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct BackendPlan {
    pub requested: &'static str,
    /// What `requested` resolves to on this machine; `None` when it cannot run here.
    pub resolved: Option<&'static str>,
    pub gpu_available: bool,
}

pub fn plan_backend(requested: &'static str, gpu_available: bool) -> BackendPlan {
    let resolved = match requested {
        "software" => Some("software"),
        "gpu" => gpu_available.then_some("gpu"),
        _ => Some(if gpu_available { "gpu" } else { "software" }),
    };
    BackendPlan {
        requested,
        resolved,
        gpu_available,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    pub plan: BackendPlan,
    pub runtime: RuntimeProbe,
    pub software_supported_layer_types: &'static [&'static str],
    pub incompatibilities: Vec<SoftwareIncompatibility>,
    pub checks: Vec<Diagnostic>,
    /// True when no blocker prevents rendering this manifest with the requested backend here.
    pub ready: bool,
}

fn blocker(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(Severity::Blocker, "preflight", code, message)
}

/// Evaluate whether `manifest` can be rendered on this machine with `requested_backend`
/// (`auto` | `software` | `gpu`). Pure of side effects except tool/GPU probing.
pub fn run_preflight(
    manifest: &Manifest,
    requested_backend: &'static str,
    output_path: Option<&Path>,
    runtime: RuntimeProbe,
) -> PreflightReport {
    let plan = plan_backend(requested_backend, runtime.gpu.available);
    let incompatibilities = software_incompatibilities(manifest);
    let mut checks = Vec::new();

    match plan.resolved {
        None => checks.push(
            blocker(
                "backend.gpu_unavailable",
                "GPU backend requested but no usable GPU adapter was found on this machine",
            )
            .observed(runtime.gpu.detail.clone())
            .recover("Use --backend software if the scene is software-compatible.")
            .recover("Otherwise render on a machine with a GPU adapter."),
        ),
        Some("software") => {
            for item in &incompatibilities {
                let (code, what) = if item.scope == "layer" {
                    (
                        "backend.software_unsupported_layer",
                        format!("layer '{}' ({})", item.id, item.kind),
                    )
                } else {
                    (
                        "backend.software_ignores_feature",
                        format!("manifest feature `{}`", item.id),
                    )
                };
                let mut diagnostic = blocker(
                    code,
                    format!("software backend cannot honor {what}; it would be dropped or rendered empty"),
                )
                .expected(SOFTWARE_SUPPORTED_LAYER_TYPES.to_vec())
                .observed(item.kind.clone())
                .recover("Render with --backend gpu on a machine with a GPU adapter.")
                .recover("Or remove/replace it; confirm with the requester before changing the design.");
                if item.scope == "layer" {
                    diagnostic = diagnostic.at(Location::layer(item.id.clone()));
                }
                checks.push(diagnostic);
            }
        }
        _ => {}
    }

    if !runtime.ffmpeg.available {
        checks.push(
            blocker(
                "runtime.ffmpeg_missing",
                "ffmpeg is not available; video export is impossible",
            )
            .observed(runtime.ffmpeg.error.clone().unwrap_or_default())
            .recover("Install ffmpeg on PATH, then re-run `vcr doctor --json`."),
        );
    }
    if !runtime.ffprobe.available {
        checks.push(
            blocker(
                "runtime.ffprobe_missing",
                "ffprobe is not available; encoded output cannot be verified",
            )
            .recover("Install ffmpeg (ships ffprobe) on PATH, then re-run `vcr doctor --json`."),
        );
    }

    let has_text = manifest
        .layers
        .iter()
        .any(|layer| matches!(layer, Layer::Text(_)));
    if !runtime.fonts.bundle_ok && has_text {
        checks.push(
            blocker(
                "runtime.font_bundle_invalid",
                "bundled fonts are missing or modified",
            )
            .observed(runtime.fonts.error.clone().unwrap_or_default())
            .recover("Restore assets/fonts/geist_pixel from the repository."),
        );
    }

    for layer in &manifest.layers {
        if let Layer::Text(text) = layer {
            if resolve_font_family(&text.text.font_family).is_none() {
                checks.push(
                    Diagnostic::new(
                        Severity::Warning,
                        "preflight",
                        "text.font_family_fallback",
                        format!(
                            "font_family '{}' is not bundled; the renderer substitutes GeistPixel-Line",
                            text.text.font_family
                        ),
                    )
                    .at(Location {
                        layer: Some(text.common.id.clone()),
                        field: Some("text.font_family".to_owned()),
                        ..Location::default()
                    })
                    .expected(
                        FONT_FAMILY_ALIASES
                            .iter()
                            .map(|(alias, _)| *alias)
                            .collect::<Vec<_>>(),
                    )
                    .observed(text.text.font_family.clone())
                    .recover("Choose a bundled family, or ask the requester to supply the font; do not assume the substitute is acceptable."),
                );
            }
        }
    }

    if let Some(output) = output_path {
        let parent = output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if parent.exists()
            && std::fs::metadata(parent)
                .map(|m| m.permissions().readonly())
                .unwrap_or(false)
        {
            checks.push(
                blocker(
                    "output.directory_not_writable",
                    "output directory is read-only",
                )
                .observed(parent.display().to_string()),
            );
        }
    }

    let ready = !checks.iter().any(Diagnostic::is_blocking);
    PreflightReport {
        plan,
        runtime,
        software_supported_layer_types: &SOFTWARE_SUPPORTED_LAYER_TYPES,
        incompatibilities,
        checks,
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_plan_resolves_against_gpu_availability() {
        assert_eq!(plan_backend("software", false).resolved, Some("software"));
        assert_eq!(plan_backend("auto", false).resolved, Some("software"));
        assert_eq!(plan_backend("auto", true).resolved, Some("gpu"));
        assert_eq!(plan_backend("gpu", true).resolved, Some("gpu"));
        assert_eq!(
            plan_backend("gpu", false).resolved,
            None,
            "an unavailable GPU must not silently become software"
        );
    }

    #[test]
    fn font_family_resolution_flags_unknown_names() {
        assert_eq!(
            resolve_font_family("GeistPixel-Square"),
            Some("GeistPixel-Square.ttf")
        );
        assert_eq!(resolve_font_family("LINE"), Some("GeistPixel-Line.ttf"));
        assert_eq!(resolve_font_family("Inter"), None);
    }
}
