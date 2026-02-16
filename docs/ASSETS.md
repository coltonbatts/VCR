# Assets (Library + Packs)

VCR now has a unified asset catalog view across:
- core library (`library:` refs)
- local packs (`pack:` refs)

## 5-minute Quickstart

### 1) Add an asset

```bash
# Add to core library (id auto-suggested)
vcr add ./assets/logo.png

# Add to a pack
vcr add ./assets/lower_third.png --pack social-kit

# Force a specific id + trailer profile
vcr add ./clips/intro.mov --id intro-hero --profile trailer
```

`vcr add` auto-detects asset type (`video`, `image`, `ascii`, `frames`).

### 2) List everything

```bash
vcr assets
```

### 3) Search

```bash
vcr assets search logo
```

### 4) Inspect one asset

```bash
vcr assets info library:intro-hero
vcr assets info pack:social-kit/lower-third
```

### 5) Render using an asset reference

Use either kind of source ref in manifests:

```yaml
layers:
  - id: logo
    source: "library:brand-logo"

  - id: lower-third
    source: "pack:social-kit/lower-third"
```

Render normally:

```bash
vcr render manifests/trailer/title_card_vcr.vcr -o renders/title_card.mov
```

## Related Commands

```bash
vcr library add <path> --id <id> [--type ...] [--normalize trailer]
vcr library list
vcr library verify
```

`vcr library ...` remains supported for explicit registry operations.
