# Deletions and Rationale

## Removed (Proposed)

| Item | Rationale |
|------|-----------|
| `cmd/init-db/` | Go module. No critical document purpose. DB init is experimental tooling, not core render. |
| `cmd/vcr-tui/` | Go TUI. Not part of core. Collapse to Rust-only. |
| `skills/video-gen/` | Go agent that calls LM Studio + vcr. Experimental, not documented as critical. |
| `internal/db/` | SQLite schema for Go modules. Goes with cmd/ removal. |
| `go.mod`, `go.sum` | No Go modules remain. |
| `experiments/` as Cargo bins | `character_density_analysis`, `luminance_to_char_mapping_test`, `frame_to_ascii_pipeline_prototype`—dev probes, not shipped. Move to optional dev tools or remove from release. |
| `archive/` | Archived scenes and exports. Dead weight. |
| `src/bin/ascii_explore.rs` | Experimental binary. Demote or gate behind feature. |

## Demoted (Not Deleted)

| Item | Action |
|------|--------|
| `src/workflow/` | Moves to `integrations/` or behind feature flag. Core does not depend on network. |
| `figma-vcr-workflow` | Integration binary. Keep but document as optional, requires tokens. |

## Retained

- Core render, schema, timeline, post_process
- CLI (main.rs)
- ASCII stage/capture (experimental but useful)
- Chat render
- Examples, manifests, assets
- Tests (determinism, cli_contract, etc.)
