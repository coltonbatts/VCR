# Contributing to VCR

First off, thank you for considering contributing to VCR! Every little bit helps.

## How to Contribute

### Reporting Bugs

If you find a bug, please search the issue tracker to see if it has already been reported. If not, open a new issue and include:

- A clear, descriptive title.
- Steps to reproduce the issue.
- Your OS and environment details.
- Any relevant logs or error messages.

### Suggesting Enhancements

Feature requests are welcome! Please open an issue and describe the enhancement you'd like to see and why it would be useful.

### Pull Requests

1. Fork the repository.
2. Create a new branch for your changes.
3. Make your changes and add tests if applicable.
4. Ensure all tests pass: `cargo test`
5. Submit a pull request with a clear description of your changes.

## Development Setup

VCR is built in Rust. You'll need:

- Rust (stable)
- FFmpeg (for video output)
- macOS (recommended for GPU features), though Linux/Windows are supported via software rendering.

```bash
git clone https://github.com/coltonbatts/VCR.git
cd VCR
cargo build
```

## Community

Please be respectful and follow our [Code of Conduct](CODE_OF_CONDUCT.md).
