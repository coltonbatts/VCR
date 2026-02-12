# Reproducible Build

## 1. Toolchain

Rust toolchain pinned via `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

## 2. Build

```bash
# From repo root
cargo build --release --bin vcr --bin figma-vcr-workflow
```

## 3. Dependencies

- Rust (stable)
- FFmpeg (for `build`, `preview` video output)
- Fonts: `assets/fonts/geist_pixel/GeistPixel-Line.ttf` (bundled)

## 4. Lockfile

`Cargo.lock` is committed. Do not run `cargo update` without explicit intent.

## 5. Scripted Build

```bash
#!/bin/bash
set -e
rustup show
cargo build --release
cargo test --all-targets
```

## 6. Release Binaries

Build artifacts: `target/release/vcr`, `target/release/figma-vcr-workflow`.

Hash verification:
```bash
sha256sum target/release/vcr
```

## 7. CI

`.github/workflows/ci.yml` runs:
- `cargo fmt --check`
- `cargo clippy`
- `cargo test --all-targets` (includes determinism, cli_contract)
