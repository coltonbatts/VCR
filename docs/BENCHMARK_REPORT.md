# Benchmark Report

## Running Benchmarks

```bash
cargo bench
```

## Metrics

| Benchmark | Description |
|-----------|-------------|
| `software_720p_frame0` | Single frame render, 1280x720, software (tiny-skia) backend, white_on_alpha.vcr |

## When to Use GPU vs Software

- **Software**: Deterministic, works in CI, no GPU required. Use for verification, golden tests, headless CI.
- **GPU**: Faster on supported hardware. Use for interactive preview, batch renders. Not guaranteed bit-exact across drivers.

## Reality Check

- CPU software render: ~10–50 ms/frame for 720p (hardware-dependent)
- GPU: typically 2–10x faster when adapter is available
- Memory: Frame buffer scales with resolution (e.g. 1280×720×4 ≈ 3.7 MB per frame)
