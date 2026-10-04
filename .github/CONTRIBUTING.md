# Contributing to VCR

Thanks for helping move VCR forward. The project picked up active maintenance again in **2026**; this guide is the short path from clone to a merge-ready PR.

## Where things are headed

- **Product vision and phases:** [docs/PRD.md](../docs/PRD.md) — read this for roadmap alignment before proposing large features.
- **Agents and automation:** [AGENTS.md](../AGENTS.md) — prompt gate, pack contact sheets, and validation order.
- **Human onboarding:** [docs/user_onboarding.md](../docs/user_onboarding.md) and the root [README.md](../README.md).

## Reporting bugs

Search existing issues first. For a new bug report, include:

- A clear title and what you expected vs what happened.
- **Reproduction steps** (minimal `.vcr` manifest or CLI flags if possible).
- Environment: OS, `vcr --version` (or `cargo run -- --version` from source), `rustc --version`, and whether you used GPU or software backend.
- Relevant logs; with agents, note if `VCR_AGENT_MODE=1` was set.

## Suggesting enhancements

Feature requests are welcome. Tie suggestions to a **user or agent workflow** (e.g. “trailer pipeline,” “tape deck,” “deterministic CI”). If it’s a major direction change, skim the PRD Phase 2/3 sections first.

## Pull requests

1. Fork the repo and create a focused branch.
2. Make changes with tests where it matters (parser, determinism, CLI contract).
3. Run the checks below.
4. Open a PR with a **short summary** and, if applicable, `Fixes #123`.

### Validation (from repo root)

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

After a release or `cargo install` build, manifests should pass:

```bash
vcr check path/to/scene.vcr
vcr lint path/to/scene.vcr
```

Optional feature matrices:

```bash
cargo build --release --features play
cargo build --release --features workflow
```

### Other components

- **Tape Deck (Go):** see [vhs-tape-deck/README.md](../vhs-tape-deck/README.md) for build and `go test` in that module.
- **Three.js sidecar:** `threejs_renderer/` — see `package.json` for scripts (e.g. `npm test`).

### PR hygiene

- Keep diffs scoped; unrelated refactors belong in separate PRs.
- Update docs when you change CLI behavior, manifest schema, or contributor-facing setup.
- Visual or motion changes: attach a short clip, contact sheet, or key frames when practical (see [VCR_SOP.md](../VCR_SOP.md) for library elements).

## Community

Follow the [Code of Conduct](CODE_OF_CONDUCT.md). For security-sensitive reports, use [SECURITY.md](SECURITY.md).
