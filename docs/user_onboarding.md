# VCR Onboarding Guide

Welcome to **VCR** (Video Component Renderer).

Think of VCR as **"After Effects for code."**

It's a terminal-driven motion graphics engine that lets you create professional, high-quality video animations without touching a complex GUI or paying for a subscription.

Instead of clicking around a timeline, you write simple text files (YAML manifests) describing your scene's layers, shapes, text, animations, and video clips. VCR parses these files and deterministically renders them into pristine video files (like ProRes 4444 with native transparent backgrounds).

Because your animations are just code, you can:

- **Version control them** with Git alongside your project.
- **Get the exact same pixel-perfect render** every single time, on any machine.
- **Automate video generation** via scripts, CI/CD, or AI agents.

## Getting Started

### 1. Install VCR

The fastest way to get VCR and the Tape Deck interactive UI is via our one-liner install script.
*(Requires [FFmpeg](https://ffmpeg.org/download.html) to be installed on your system).*

```bash
curl -fsSL https://raw.githubusercontent.com/coltonbatts/VCR/main/scripts/install.sh | bash
```

*(Make sure `~/.local/bin` is in your shell's PATH after installation!)*

### 2. Open the Deck

VCR uses a **Tapes-First Workflow**. You treat your animation manifests as immutable recipes ("Tapes"), VCR as the render engine, and the "Tape Deck" as your interactive controller UI.

Instead of memorizing long rendering commands to manage your files, just open the Deck in your terminal:

```bash
vcr deck
```

From the Deck interface, you can effortlessly:

- **Initialize** your project's `tapes.yaml` environment automatically.
- **Create** new tapes from templates.
- **Preview & Play** your animations instantly.

That's it! You're now generating deterministic motion graphics direct from your terminal. Enjoy!
