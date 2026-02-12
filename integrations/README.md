# VCR Integrations

Optional workflow layers that require network or external APIs. Core render does not depend on these.

## Figma → VCR

Binary: `figma-vcr-workflow` (built with `cargo build --release --bin figma-vcr-workflow`)

- Requires: `FIGMA_TOKEN`, optional `ANTHROPIC_API_KEY`
- Converts Figma file + description into VCR-ready manifests and assets
- Source: `src/bin/figma-vcr-workflow.rs`, `src/workflow/`

## Scope

Integrations are out of scope for deterministic offline rendering. Use when preparing content for VCR; the resulting manifests render offline.
