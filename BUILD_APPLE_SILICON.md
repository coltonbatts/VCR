# VCR on Apple Silicon (M1/M2/M3/M4)

This guide builds VCR for native macOS ARM64 and verifies Metal acceleration is active.

## Prerequisites

```bash
xcode-select --install
brew install ffmpeg
rustup target add aarch64-apple-darwin
```

## Build Native ARM64

```bash
rustc --version --verbose | grep host
# expected: host: aarch64-apple-darwin

cargo build --release --target aarch64-apple-darwin
```

## Verify Metal Backend Is Compiled

```bash
cargo tree -e features -i wgpu
```

Expected `wgpu` features include `metal` (and `wgsl`) for this project.

## Verify Runtime Uses GPU

```bash
cargo run --release --target aarch64-apple-darwin -- render-frame examples/demo_scene.vcr --frame 0 -o /tmp/vcr_frame0.png
```

Expected backend line:

```text
[VCR] Backend: GPU (adapter 'Apple <chip>' (Metal))
```

If you see CPU fallback:

```text
[VCR] Backend: CPU (no suitable GPU adapter found...)
```

check troubleshooting below.

## Benchmark Command

```bash
/usr/bin/time -l cargo run --release --target aarch64-apple-darwin -- build examples/demo_scene.vcr -o /tmp/vcr_metal_test.mov
```

The command prints:
- wall time (`real`)
- render/encode breakdown (`[VCR] timing ...`)
- peak memory footprint (`peak memory footprint`)

## Expected Performance (Reference)

On an M1 Max, demo scene results from this repo:
- GPU (Metal): total render pipeline about 1.39s, wall time about 2.51s
- CPU fallback: total render pipeline about 12.57s, wall time about 13.06s

The exact numbers vary by scene complexity, resolution, and FFmpeg settings. GPU should be multiple times faster than CPU fallback.

## Test Suite (ARM64)

```bash
cargo test --release --target aarch64-apple-darwin
```

## Troubleshooting

1. No GPU adapter found:
   - Confirm native target: `host: aarch64-apple-darwin`.
   - Confirm Metal is enabled: `cargo tree -e features -i wgpu`.
   - Avoid restricted/sandboxed environments that can block Metal device access.
2. Build or linker issues:
   - Reinstall Xcode CLT: `xcode-select --install`.
3. Slow total time even with GPU:
   - FFmpeg encode can dominate some scenes; compare render vs encode in `[VCR] timing`.

