use std::path::Path;

use vcr::manifest::{
    load_and_validate_manifest, load_and_validate_manifest_with_options, ManifestLoadOptions,
    ParamOverride,
};
use vcr::renderer::Renderer;
use vcr::timeline::RenderSceneData;

#[test]
fn determinism_legacy_manifest_is_stable() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/white_on_alpha.vcr");

    let first = render_hash(&manifest_path, 0, &[]);
    let second = render_hash(&manifest_path, 0, &[]);
    assert_eq!(
        first, second,
        "legacy manifest render should be deterministic"
    );
}

#[test]
fn determinism_with_same_overrides_is_stable() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/steerable_motion.vcr");
    let overrides = [
        "speed=1.9",
        "glow_strength=1.2",
        "drift=220,90",
        "accent_color=#4FE1B8",
    ];

    let first = render_hash(&manifest_path, 48, &overrides);
    let second = render_hash(&manifest_path, 48, &overrides);
    assert_eq!(
        first, second,
        "render with identical --set overrides should be deterministic"
    );
}

#[test]
fn determinism_overrides_change_output_when_values_change() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/steerable_motion.vcr");

    let slow = render_hash(&manifest_path, 48, &["speed=0.6", "glow_strength=0.4"]);
    let fast = render_hash(&manifest_path, 48, &["speed=2.4", "glow_strength=1.3"]);
    assert_ne!(
        slow, fast,
        "different overrides should produce different visual output for steerable manifests"
    );
}

fn render_hash(manifest_path: &Path, frame: u32, overrides: &[&str]) -> u64 {
    let manifest = if overrides.is_empty() {
        load_and_validate_manifest(manifest_path).expect("failed to load manifest")
    } else {
        let parsed_overrides = overrides
            .iter()
            .map(|raw| ParamOverride::parse(raw).expect("override should parse"))
            .collect::<Vec<_>>();
        load_and_validate_manifest_with_options(
            manifest_path,
            &ManifestLoadOptions {
                overrides: parsed_overrides,
                allow_raw_paths: false,
            },
        )
        .expect("failed to load manifest with overrides")
    };

    let mut renderer = Renderer::new_software(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(&manifest),
    )
    .expect("failed to create renderer");
    assert!(
        !renderer.is_gpu_backend(),
        "Determinism tests must run on CPU backend"
    );

    let rgba = renderer
        .render_frame_rgba(frame)
        .expect("failed to render frame for hash");
    fnv1a64(&rgba)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

#[derive(serde::Deserialize)]
struct RenderJsonOutput {
    frame_hash: String,
    output_hash: String,
}

fn render_cli_json(manifest: &Path, output_name: &str, backend: &str, cwd: &Path) -> RenderJsonOutput {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(cwd)
        .arg("render")
        .arg(manifest)
        .arg("-o")
        .arg(output_name)
        .arg("--backend")
        .arg(backend)
        .arg("--json")
        .output()
        .expect("Failed to execute VCR process");

    assert!(result.status.success(), "VCR render failed: {}", String::from_utf8_lossy(&result.stderr));

    let stdout = String::from_utf8_lossy(&result.stdout);
    let json_str = stdout.lines().last().expect("No output from VCR");
    serde_json::from_str(json_str).expect("Failed to parse JSON")
}

#[test]
fn determinism_repeat_render() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/minimal_manifest.yaml");
    let test_dir = tempfile::tempdir().unwrap();

    let first = render_cli_json(&manifest_path, "out.mov", "software", test_dir.path());

    for i in 0..4 {
        let output_name = format!("out_{}.mov", i);
        let next = render_cli_json(&manifest_path, &output_name, "software", test_dir.path());

        assert_eq!(
            first.frame_hash, next.frame_hash,
            "Frame hash mismatch on repeat render #{}", i + 2
        );
        assert_eq!(
            first.output_hash, next.output_hash,
            "Output file hash mismatch on repeat render #{}", i + 2
        );
    }
}

#[test]
fn backend_parity_contract() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/minimal_manifest.yaml");
    let test_dir = tempfile::tempdir().unwrap();

    // Software is our canonical deterministic backend
    let sw_run1 = render_cli_json(&manifest_path, "sw_1.mov", "software", test_dir.path());
    let sw_run2 = render_cli_json(&manifest_path, "sw_2.mov", "software", test_dir.path());
    
    assert_eq!(
        sw_run1.frame_hash, sw_run2.frame_hash,
        "Software backend must be perfectly stable across runs"
    );

    // We record the GPU hash, but we explicitly do NOT fail if it mismatches software.
    // GPU rendering is not guaranteed to be bit-identical to software rasterization.
    let gpu_run = render_cli_json(&manifest_path, "gpu.mov", "gpu", test_dir.path());
    
    println!("Backend Hash Contract:");
    println!("  -> Canonical SWE (CPU): {}", sw_run1.frame_hash);
    println!("  -> Hardware ACC (GPU): {}", gpu_run.frame_hash);
}
