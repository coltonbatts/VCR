# ASCII Animation Assets

Directory layout:

```
assets/animations/<animation-name>/
  metadata.json
  0001.txt
  0002.txt
  ...
```

Frame files:

- Supported extensions: `.txt`, `.ans`
- Numbered filenames are recommended (`0001.txt`, `0002.txt`, ...)
- The loader sorts numerically by filename prefix, then lexicographically

`metadata.json` schema:

```json
{
  "title": "string",
  "artist": "string",
  "artist_url": "https://...",
  "source_url": "https://...",
  "license": "string",
  "tags": ["tag", "tag"],
  "credit": "optional explicit credit line"
}
```

Credit/tagging strategy:

- `artist` + `source_url` identify original creator and source page
- `license` captures redistribution/use rights
- `tags` enable grouping/search in tooling (e.g. `retro`, `character`, `loop`)
- `credit` can override generated credit text for strict attribution requirements
