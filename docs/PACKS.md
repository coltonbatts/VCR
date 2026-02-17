# Packs

Packs let you group reusable assets under a shared ID namespace.

A pack asset reference looks like:

```text
pack:<pack-id>/<asset-id>
```

## Directory Format

```text
packs/
  <pack-id>/
    pack.json
    items/
      <asset-id>/
        source.<ext>
        # or source/ (for frames)
```

## `pack.json` Format

```json
{
  "version": 1,
  "pack_id": "social-kit",
  "items": [
    {
      "id": "lower-third",
      "type": "image",
      "path": "items/lower-third/source.png",
      "sha256": "<64-char hex>",
      "spec": {
        "width": 1920,
        "height": 1080,
        "fps": null,
        "frames": null,
        "duration_seconds": null,
        "has_alpha": true,
        "pixel_format": "rgba8"
      },
      "tags": ["brand"]
    }
  ]
}
```

Notes:
- `pack_id` must match the folder name.
- `items[].id` uses kebab-case.
- `items[].path` must stay under `items/<asset-id>/...`.
- hashes/specs are used for deterministic verification at resolve time.

## Make a Pack Quickly

```bash
# Create/update pack + item metadata automatically
vcr add ./assets/lower_third.png --pack social-kit --id lower-third
vcr add ./assets/logo.png --pack social-kit --id logo
```

Inspect:

```bash
vcr assets search social-kit
vcr assets info pack:social-kit/lower-third
```

Generate a labeled contact sheet (recommended before picking item IDs to animate):

```bash
scripts/pack_contact_sheet.sh \
  --pack packs/social-kit \
  --out renders/social-kit/contact_sheet.png \
  --index-out renders/social-kit/contact_sheet.index.tsv
```

Output:
- `contact_sheet.png` with item ID + dimensions on each tile.
- `contact_sheet.index.tsv` with `id`, `width`, `height`, and source path.

## Use in Manifests

```yaml
layers:
  - id: lower-third
    source: "pack:social-kit/lower-third"
```

Or shorthand mapping:

```yaml
layers:
  - id: lower-third
    source:
      kind: pack
      pack: social-kit
      id: lower-third
```

## Sharing Packs

Share the `packs/<pack-id>/` folder (or commit it to git). Consumers can render without network access as long as the pack files are present.
