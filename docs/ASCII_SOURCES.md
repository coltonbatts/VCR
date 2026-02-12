# ASCII Sources (`vcr ascii sources`)

`vcr ascii sources` prints a static, curated registry of known-good animated ASCII source references.

- Discoverability only.
- No runtime network fetches by the registry itself.
- Output is stable text for copy/paste into capture commands.

## Command

```bash
vcr ascii sources
```

## Categories

### 🔴 Live Streaming Sources

| Source | URL / Endpoint | Content |
|--------|----------------|---------|
| **ascii.live** | `ascii.live/<stream>` | `parrot`, `nyan`, `forrest`, `clock`, `donut`, `earth`, `bnr`, `knot`, `batman`, `coin`, `hes`, `spidyswing`, `maxwell`, `kitty`, `batman-running`, `dvd`, `torus-knot`, `purdue`, `bomb`, `india`, `can-you-hear-me`, `playstation`, `rick`, `as` |
| **hugomd/ascii-live** | `curl -s https://ascii.live/parrot` | Extends various terminal animations. Use `ascii.live/list` for dynamic updates. |

### 🌐 Websites with ASCII Animations

- [ascii.co.uk/animated](https://ascii.co.uk/animated) - Large collection of text-based animations.
- [asciiart.eu/animations](https://www.asciiart.eu/animations) - Classic animations like Kaleidoscope, Matrix Rain.
- [animasci.com](https://animasci.com) - Modern text animations and emoticons.

### 📦 GitHub Repositories

| Repo | Description |
|------|-------------|
| [doctorfree/Asciiville](https://github.com/doctorfree/Asciiville) | ~1,000 ASCII/ANSI art works, galleries, and fractals. Includes `asciiart` command for random loops. |
| [wang0618/ascii-art](https://github.com/wang0618/ascii-art) | Collection of text-based terminal animations. |
| [brandon-rhodes/scrawler](https://github.com/brandon-rhodes/scrawler) | Python library for ASCII animations. |
| [Leivmox/ASCII-ART](https://github.com/Leivmox/ASCII-ART) | Personal collection for Steam and forums. |
| [NNBnh/ansi](https://github.com/NNBnh/ansi) | Personal ANSI art gallery. |
| [moul/awesome-ascii-art](https://github.com/moul/awesome-ascii-art) | The definitive curated list of ASCII resources. |

### 🔧 Tools to Generate ASCII Assets

- **chafa** - Image/Video to ASCII converter (native to VCR).
- **Asciiville** - Full suite: conversion (`asciiville` command), galleries, and interactive animations.
- **ansilove** - ANSI to PNG/other format converter. Best for preserving `.ans` file legacy colors.

### 💾 Potential GIF/Video Sources (Convert with `chafa`)

- **archive.org** - Public domain video footage. Use `chafa <video_url>` or download + `chafa <file>`.
- **GIPHY / Tenor** - API-driven GIF sourcing. Use `vcr capture --source chafa:<giphy_url>`.

## Usage in VCR

- `vcr ascii capture --source ascii-live:<stream_id>`
- `vcr ascii capture --source chafa:<path_to_file_or_url>`
- `vcr ascii capture --source library:<id>` (built-in fallback)

## Recommended Workflows

### Converting Archive.org Clips

1. Find a short public domain clip on [archive.org](https://archive.org/details/movies).
2. Grab the MP4/WebM direct URL.
3. Run: `vcr capture --source chafa:https://archive.org/download/.../file.mp4`

### Asciiville Gallery Browsing

1. Install Asciiville: `brew install Asciiville` (on macOS).
2. Run `asciiville -V Art` to browse the curated gallery.
3. Identify favorites and copy them to your local library for capture.

## Notes

- `ascii-live` IDs listed here are tested and compatible.
- Local files (`.txt`, `.ans`, `.asc`) can be captured directly.
- For `archive.org` or GIFs, download first then use the `chafa` source.
