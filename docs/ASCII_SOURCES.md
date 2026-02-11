# ASCII Sources (`vcr ascii sources`)

`vcr ascii sources` prints a static, curated registry of known-good animated ASCII source references.

- Discoverability only.
- No runtime network fetches.
- No scraping.
- Output is stable text for copy/paste.

## Command

```bash
vcr ascii sources
```

## Included Source Types

1. `stream`
2. `pack`
3. `tool`

## Minimum References Included

1. `ascii.live` endpoints from the curated ASCII Art Collection gist:
   - `forrest`
   - `parrot`
   - `clock`
   - `can-you-hear-me`
   - `donut`
   - `earth`
2. `16colo.rs` archive reference (manual download + local conversion workflow for now).
3. `ansilove` reference and pipeline example.
4. `chafa` reference and direct `chafa:<path>` usage example.

## Notes

- `vcr ascii capture` currently accepts `--source ascii-live:<stream>`, `--source library:<id>`, and `--source chafa:<path>`.
- `ascii-live` stream ids in this registry are wired into `vcr ascii capture`.
- `library:<id>` sources are built-in offline generators for dev-mode fallback.
