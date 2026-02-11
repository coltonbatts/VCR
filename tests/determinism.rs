use std::path::Path;
use vcr::manifest::load_and_validate_manifest;
use vcr::renderer::Renderer;
use vcr::timeline::RenderSceneData;

#[test]
fn test_determinism_white_test_frame_60() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/white_test.vcr");
    let manifest = load_and_validate_manifest(&manifest_path).expect("failed to load manifest");
    
    let mut renderer = Renderer::new_software(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(&manifest),
    ).expect("failed to create renderer");

    assert!(!renderer.is_gpu_backend(), "Determinism tests must run on CPU backend");

    let frame = renderer.render_frame_rgba(60).expect("failed to render frame 60");
    let hash = fnv1a64(&frame);
    
    // Expected hash for white_test.vcr frame 60 on CPU
    let expected_hash = 11496645850692453479; 
    assert_eq!(hash, expected_hash, "Hash mismatch for white_test.vcr frame 60. If this change is intentional, update the golden hash. Actual: {}", hash);
}

#[test]
fn test_determinism_sanity_check_static_frame_0() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/sanity_check_static.vcr");
    let manifest = load_and_validate_manifest(&manifest_path).expect("failed to load manifest");
    
    let mut renderer = Renderer::new_software(
        &manifest.environment,
        &manifest.layers,
        RenderSceneData::from_manifest(&manifest),
    ).expect("failed to create renderer");

    assert!(!renderer.is_gpu_backend());

    let frame = renderer.render_frame_rgba(0).expect("failed to render frame 0");
    let hash = fnv1a64(&frame);
    
    // Expected hash for sanity_check_static.vcr frame 0 on CPU
    let expected_hash = 18115719896304974138; 
    assert_eq!(hash, expected_hash, "Hash mismatch for sanity_check_static.vcr frame 0. If this change is intentional, update the golden hash. Actual: {}", hash);
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
