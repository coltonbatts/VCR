//! ASCII render exploration harness.
//!
//! Generates a matrix of ASCII configuration variations for visual comparison.
//! Does NOT modify the ASCII pipeline or shader logic. Uses existing manifest
//! loading and render-frame plumbing.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::RgbaImage;
use vcr::manifest::{load_and_validate_manifest_with_options, ManifestLoadOptions};
use vcr::renderer::Renderer;
use vcr::timeline::{ascii_overrides_from_flags, AsciiRuntimeOverrides, RenderSceneData};

const DEFAULT_MANIFEST: &str = "manifests/ascii_post_debug.vcr";
const OUTPUT_DIR: &str = "renders/ascii_explore";
const FRAME_INDEX: u32 = 0;

/// Default ramp from schema (10 chars).
const RAMP_DEFAULT: &str = " .:-=+*#%@";
/// Dense ramp with full block for finer gradation (11 chars).
const RAMP_DENSE: &str = " .:-=+*#%@█";

/// Cell resolution (cols, rows).
const RES_80X24: (u32, u32) = (80, 24);
const RES_120X45: (u32, u32) = (120, 45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoolVariant {
    On,
    Off,
}

impl BoolVariant {
    fn as_str(&self) -> &'static str {
        match self {
            BoolVariant::On => "on",
            BoolVariant::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RampVariant {
    Default,
    Dense,
}

impl RampVariant {
    fn ramp(&self) -> &'static str {
        match self {
            RampVariant::Default => RAMP_DEFAULT,
            RampVariant::Dense => RAMP_DENSE,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            RampVariant::Default => "default",
            RampVariant::Dense => "dense",
        }
    }
}

/// One combination in the variation matrix.
#[derive(Debug, Clone)]
struct Variation {
    edge_boost: BoolVariant,
    bayer_dither: BoolVariant,
    stabilize: BoolVariant, // Scaffolded: included in matrix/filenames but not yet wired to pipeline
    cols: u32,
    rows: u32,
    ramp: RampVariant,
}

impl Variation {
    fn filename(&self) -> String {
        format!(
            "edge_{}_bayer_{}_stab_{}_{}x{}_ramp_{}.png",
            self.edge_boost.as_str(),
            self.bayer_dither.as_str(),
            self.stabilize.as_str(),
            self.cols,
            self.rows,
            self.ramp.as_str(),
        )
    }

    fn progress_label(&self, index: usize, total: usize) -> String {
        format!(
            "Rendering {}/{} → edge={} bayer={} stabilize={} res={}x{} ramp={}",
            index,
            total,
            self.edge_boost.as_str(),
            self.bayer_dither.as_str(),
            self.stabilize.as_str(),
            self.cols,
            self.rows,
            self.ramp.as_str(),
        )
    }
}

/// Build all variations. Stabilize is scaffolded (included but not applied).
fn build_variations() -> Vec<Variation> {
    let mut out = Vec::with_capacity(32);
    for edge in [BoolVariant::On, BoolVariant::Off] {
        for bayer in [BoolVariant::On, BoolVariant::Off] {
            for stab in [BoolVariant::On, BoolVariant::Off] {
                for (cols, rows) in [RES_80X24, RES_120X45] {
                    for ramp in [RampVariant::Default, RampVariant::Dense] {
                        out.push(Variation {
                            edge_boost: edge,
                            bayer_dither: bayer,
                            stabilize: stab,
                            cols,
                            rows,
                            ramp,
                        });
                    }
                }
            }
        }
    }
    out
}

fn main() -> Result<()> {
    let manifest_path = std::env::current_dir()
        .context("failed to get cwd")?
        .join(DEFAULT_MANIFEST);

    run_explore(&manifest_path)
}

fn run_explore(manifest_path: &Path) -> Result<()> {
    let base_manifest =
        load_and_validate_manifest_with_options(manifest_path, &ManifestLoadOptions::default())
            .with_context(|| format!("failed to load manifest {}", manifest_path.display()))?;

    if base_manifest
        .ascii_post
        .as_ref()
        .map_or(true, |c| !c.enabled)
    {
        bail!(
            "manifest {} must have ascii_post.enabled: true for this tool",
            manifest_path.display()
        );
    }

    let variations = build_variations();
    let total = variations.len();

    std::fs::create_dir_all(OUTPUT_DIR)
        .with_context(|| format!("failed to create output directory {}", OUTPUT_DIR))?;

    let output_dir = PathBuf::from(OUTPUT_DIR);

    println!("[ascii_explore] Base manifest: {}", manifest_path.display());
    println!("[ascii_explore] Output directory: {}", output_dir.display());
    println!("[ascii_explore] Variations: {}", total);
    println!();

    for (i, variation) in variations.iter().enumerate() {
        println!("{}", variation.progress_label(i + 1, total));

        let mut manifest = base_manifest.clone();
        if let Some(ref mut ap) = manifest.ascii_post {
            ap.cols = variation.cols;
            ap.rows = variation.rows;
            ap.ramp = variation.ramp.ramp().to_owned();
        }

        let ascii_overrides = ascii_overrides_from_flags(
            Some(matches!(variation.edge_boost, BoolVariant::On)),
            Some(matches!(variation.bayer_dither, BoolVariant::On)),
        )
        .unwrap_or_else(|| AsciiRuntimeOverrides::default());

        let mut scene = RenderSceneData::from_manifest(&manifest);
        scene = scene.with_ascii_overrides(ascii_overrides);

        let mut renderer = pollster::block_on(Renderer::new_with_scene(
            &manifest.environment,
            &manifest.layers,
            scene,
        ))
        .context("failed to create renderer")?;

        let rgba = renderer
            .render_frame_rgba(FRAME_INDEX)
            .context("failed to render frame")?;

        let output_path = output_dir.join(variation.filename());
        save_rgba_png(
            &output_path,
            manifest.environment.resolution.width,
            manifest.environment.resolution.height,
            rgba,
        )
        .with_context(|| format!("failed to save {}", output_path.display()))?;
    }

    println!();
    println!(
        "[ascii_explore] Done. Wrote {} images to {}",
        total,
        output_dir.display()
    );
    Ok(())
}

fn save_rgba_png(path: &Path, width: u32, height: u32, rgba: Vec<u8>) -> Result<()> {
    let image = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| {
        anyhow::anyhow!(
            "failed to construct image buffer for {}x{} RGBA frame",
            width,
            height
        )
    })?;
    image
        .save(path)
        .with_context(|| format!("failed to write png {}", path.display()))
}
