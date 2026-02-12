//! Frame render benchmarks: CPU (software) vs GPU.
//! Run: cargo bench
//!
//! GPU benchmark may be skipped if no adapter is available.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;
use vcr::manifest::load_and_validate_manifest;
use vcr::renderer::Renderer;
use vcr::timeline::RenderSceneData;

fn bench_software_render(c: &mut Criterion) {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/white_on_alpha.vcr");
    let manifest = load_and_validate_manifest(&manifest_path).expect("load manifest");
    let scene = RenderSceneData::from_manifest(&manifest);

    let mut group = c.benchmark_group("render_frame");
    group.sample_size(50);

    group.bench_function("software_720p_frame0", |b| {
        b.iter(|| {
            let mut renderer =
                Renderer::new_software(&manifest.environment, &manifest.layers, scene.clone())
                    .expect("create renderer");
            black_box(renderer.render_frame_rgba(0).expect("render"))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_software_render);
criterion_main!(benches);
