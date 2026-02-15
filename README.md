# VCR (Video Component Renderer)

```text
██╗   ██╗ ██████╗██████╗
██║   ██║██╔════╝██╔══██╗
██║   ██║██║     ██████╔╝
╚██╗ ██╔╝██║     ██╔══██╗
 ╚████╔╝ ╚██████╗██║  ██║
  ╚═══╝   ╚═════╝╚═╝  ╚═╝
```

A deterministic motion graphics engine in Rust. Write YAML, render video.

- **Procedural** — shapes, text, gradients, custom WGSL shaders
- **Deterministic** — same manifest always produces identical output
- **Dual backend** — GPU (wgpu) with software (tiny-skia) fallback
- **AI-native** — designed for agents to author manifests ([`SKILL.md`](SKILL.md))

## Quickstart

```bash
# 1. Clone and build
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo build --release --bin vcr

# 2. Check your environment
./target/release/vcr doctor

# 3. Render your first frame
./target/release/vcr render-frame examples/hello_world.vcr --frame 0

# 4. Render a video
./target/release/vcr build examples/hello_world.vcr -o renders/hello.mov

# 5. Live reload while editing
./target/release/vcr watch examples/hello_world.vcr -o renders/preview
```

**Requirements:** Rust (stable), FFmpeg, macOS recommended (software fallback on Linux)

## Learn

- [`examples/`](examples/README.md) — guided learning path from hello world to custom shaders
- [`SKILL.md`](SKILL.md) — complete manifest reference (layer types, expressions, CLI)
- [`docs/`](docs/) — architecture, determinism spec, ASCII tools, and more

## CLI

```
vcr build              Render a manifest to ProRes .mov video
vcr check              Validate a manifest without rendering
vcr lint               Check for unreachable layers and warnings
vcr preview            Render a quick scaled-down preview
vcr render-frame       Render a single frame to PNG
vcr watch              Re-render on file change (live reload)
vcr play               Open a live playback window
vcr doctor             Check system dependencies (FFmpeg, GPU)
vcr explain            Show how expressions and layers resolve
vcr params             List declared params and their defaults
vcr dump               Print resolved layer state at a given frame
vcr prompt             Translate natural language into a VCR manifest
vcr ascii              ASCII art capture and rendering commands
```

Run `vcr --help` for full details on any command.

## Runtime Overrides

Override manifest params without editing the file:

```bash
vcr build scene.vcr -o out.mov --set speed=2.2 --set accent_color=#4FE1B8
```

## Advanced Topics

See the [`docs/`](docs/) folder for:

- [Architecture](docs/ARCHITECTURE.md)
- [Determinism Spec](docs/DETERMINISM_SPEC.md)
- [ASCII Stage](docs/ASCII_STAGE.md) & [ASCII Capture](docs/ASCII_CAPTURE.md)
- [Params & Overrides](docs/PARAMS.md)
- [Playground Presets](docs/PLAYGROUND.md)
- [Agent Error Contract](docs/EXIT_CODES.md)
- [WGSL Standard Library](docs/STDLIB_ALPHA.md)
- [Figma Workflow](docs/SKILLS_PROTOCOL.md)

## License

[MIT License](LICENSE)

## Author

Colton Batts ([@coltonbatts](https://github.com/coltonbatts))
