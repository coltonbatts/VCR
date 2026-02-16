# VCR Library

VCR includes a local, deterministic asset registry under `library/`.

## Layout

```text
library/
  library.json
  items/
    <id>/
      source.<ext>
```

`library.json` stores stable IDs, pinned SHA-256 hashes, and expected media specs.

## Registry Schema

`library/library.json`:

```json
{
  "version": 1,
  "items": [
    {
      "id": "example-id",
      "type": "video",
      "path": "library/items/example-id/source.mov",
      "sha256": "<64-char hex>",
      "spec": {
        "width": 1080,
        "height": 1920,
        "fps": 24.0,
        "frames": 144,
        "duration_seconds": 6.0,
        "has_alpha": true,
        "pixel_format": "yuva444p10le"
      },
      "tags": ["trailer"],
      "provenance": {
        "source_url": "https://...",
        "retrieved_at": "2026-02-16T00:00:00Z",
        "license": "CC-BY-4.0"
      }
    }
  ]
}
```

Fields:
- `id`: kebab-case stable ID.
- `type`: `video | image | ascii | frames`.
- `path`: workspace-relative path.
- `sha256`: pinned hash.
- `spec`: expected media spec.
- `tags`: optional.
- `provenance`: optional.

## Commands

Add:

```bash
vcr library add <path> --id <id> [--type video|image|ascii|frames] [--normalize trailer]
```

`--normalize trailer` currently supports `type=video` and produces canonical trailer source:
- 1080x1920
- 24 fps
- 6s duration (looped/trimmed)
- ProRes 4444 (`-profile:v 4`) + `yuva444p10le`

Verify:

```bash
vcr library verify
```

Checks:
- file/directory exists
- SHA-256 matches registry
- probed media spec matches pinned `spec`

List:

```bash
vcr library list [--tag <tag>] [--type video|image|ascii|frames]
```

## Manifest Integration

Supported source references:

```yaml
source: "library:<id>"
```

or

```yaml
source:
  kind: library
  id: "<id>"
```

`source` is normalized to the layer's asset path (`image.path`, `ascii.path`, `sequence.path`, or `source_path`).

## Trailer Enforcement

For manifests under `manifests/trailer/`:
- raw asset paths are rejected by default
- `library:<id>` is required for external assets

Opt-out flag:

```bash
vcr --allow-raw-path-sources render manifests/trailer/foo.vcr -o renders/foo.mov
```

## Determinism Guarantees

- Registry writes are stable-sorted by `id` to reduce diff noise.
- Asset bytes are pinned by SHA-256.
- Media specs are validated during `verify` and at manifest library resolution.
- Runtime network access is not required.
