use std::path::Path;

use vcr::manifest::load_and_validate_manifest;
use vcr::renderer::Renderer;
use vcr::timeline::RenderSceneData;

#[test]
fn sequence_layer_is_deterministic() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sequence_test.vcr");
    let first = render_hash(&manifest_path, 0);
    let second = render_hash(&manifest_path, 0);
    assert_eq!(
        first, second,
        "sequence layer frame 0 must produce identical hash across runs"
    );

    // Golden hash — software backend, 64×64 red gradient PNG sequence frame 0
    const GOLDEN_FRAME0: u64 = 0xa448_e4b5_24ba_ca25;
    assert_eq!(
        first, GOLDEN_FRAME0,
        "frame 0 hash {first:#018x} does not match golden {GOLDEN_FRAME0:#018x}"
    );
}

#[test]
fn sequence_layer_different_frames_produce_different_output() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sequence_test.vcr");
    let frame0 = render_hash(&manifest_path, 0);
    let frame2 = render_hash(&manifest_path, 2);
    assert_ne!(
        frame0, frame2,
        "frame 0 (red gradient) and frame 2 (blue gradient) must differ"
    );

    // Golden hash — software backend, 64×64 blue gradient PNG sequence frame 2
    const GOLDEN_FRAME2: u64 = 0xc151_ee65_465c_0225;
    assert_eq!(
        frame2, GOLDEN_FRAME2,
        "frame 2 hash {frame2:#018x} does not match golden {GOLDEN_FRAME2:#018x}"
    );
}

fn render_hash(manifest_path: &Path, frame: u32) -> u64 {
    let manifest = load_and_validate_manifest(manifest_path).expect("failed to load manifest");
    let scene = RenderSceneData::from_manifest(&manifest);
    let mut renderer = Renderer::new_software(&manifest.environment, &manifest.layers, scene)
        .expect("failed to create software renderer");
    assert!(
        !renderer.is_gpu_backend(),
        "determinism tests must use software backend"
    );

    let rgba = renderer
        .render_frame_rgba(frame)
        .expect("render_frame_rgba failed");
    fnv1a64(&rgba)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}
